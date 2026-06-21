use crate::App;
use rayon::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::any::Any;
use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Weak;
use tracing::debug;
use twox_hash::XxHash3_64;

const MANIFEST_FILE: &str = "manifest.json";
const CACHE_DIR: &str = ".bark-cache";

pub fn init<P: Into<PathBuf>>(app: &mut App, assets_dir: P) {
    app.world.insert_resource(Assets::new(assets_dir.into()))
}

pub struct Assets {
    manifest: Manifest,
    cache_dir: PathBuf,
    storage: HashMap<String, Weak<dyn Any + Send + Sync>>,
}

impl Assets {
    fn new(assets_dir: PathBuf) -> Self {
        let manifest_path = assets_dir.join(MANIFEST_FILE);
        let cache_dir = assets_dir.join(CACHE_DIR);
        let manifest = serde_json::from_reader(File::open(&manifest_path).unwrap()).unwrap();
        debug!("loaded manifest from {:?}", manifest_path);

        Self {
            manifest,
            cache_dir,
            storage: HashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Manifest(HashMap<String, ManifestEntry>);

#[derive(Serialize, Deserialize)]
struct ManifestEntry {
    ty: String,
    #[serde(
        serialize_with = "serialize_hash",
        deserialize_with = "deserialize_hash"
    )]
    hash: u64,
    options: serde_json::Value,
}

fn serialize_hash<S: Serializer>(data: &u64, se: S) -> Result<S::Ok, S::Error> {
    hex::serialize(data.to_be_bytes(), se)
}

fn deserialize_hash<'de, D: Deserializer<'de>>(de: D) -> Result<u64, D::Error> {
    hex::deserialize(de).map(u64::from_be_bytes)
}

pub struct AssetProcessors {
    manifest: Manifest,
    assets_dir: PathBuf,
    cache_dir: PathBuf,
    processors: HashMap<String, Box<dyn ErasedAssetProcessor>>,
}

impl AssetProcessors {
    pub fn new<P: Into<PathBuf>>(assets_dir: P) -> Self {
        let assets_dir = assets_dir.into();
        let manifest_path = assets_dir.join(MANIFEST_FILE);
        let cache_dir = assets_dir.join(CACHE_DIR);

        let manifest = serde_json::from_reader(File::open(&manifest_path).unwrap()).unwrap();
        Self {
            manifest,
            assets_dir,
            cache_dir,
            processors: HashMap::new(),
        }
    }

    pub fn register<T: AssetProcessor + 'static>(&mut self, ty: &str, processor: T) {
        self.processors.insert(ty.into(), Box::new(processor));
    }

    pub fn run(mut self) {
        fs::create_dir_all(&self.cache_dir).unwrap();
        let hashes = self
            .manifest
            .0
            .par_iter()
            .map(|(path, entry)| {
                let src_path = self.assets_dir.join(path);
                let bytes = fs::read(&src_path).unwrap();

                let hash = hash_asset(&bytes, &entry.options);
                let cache_path = self.cache_dir.join(hex::encode(hash.to_be_bytes()));

                if entry.hash != hash || !cache_path.exists() {
                    build_log("Processing", &format!("{} ({})", path, entry.ty));
                    let processor = self.processors.get(&entry.ty).unwrap();
                    processor.process_erased(
                        File::open(&src_path).unwrap(),
                        File::create(&cache_path).unwrap(),
                        &entry.options,
                    );
                }
                hash
            })
            .collect::<Vec<_>>();

        let mut n_cache_hits = 0;
        for (entry, new_hash) in self.manifest.0.values_mut().zip(hashes) {
            if entry.hash != new_hash {
                entry.hash = new_hash;
            } else {
                n_cache_hits += 1;
            }
        }
        build_log(
            "Processed",
            &format!("{} assets, {} cached", self.manifest.0.len(), n_cache_hits),
        );

        let manifest_path = self.assets_dir.join(MANIFEST_FILE);
        serde_json::to_writer_pretty(&File::create(&manifest_path).unwrap(), &self.manifest)
            .unwrap();
    }
}

fn hash_asset(data: &[u8], opts: &serde_json::Value) -> u64 {
    let mut hasher = XxHash3_64::new();
    data.hash(&mut hasher);
    opts.hash(&mut hasher);
    hasher.finish()
}

pub trait AssetProcessor: Send + Sync {
    type Options: Serialize + DeserializeOwned;

    fn process<R: Read, W: Write>(&self, src: R, out: W, opts: Self::Options);
}

trait ErasedAssetProcessor: Send + Sync {
    fn process_erased(&self, src: File, out: File, opts: &serde_json::Value);
}

impl<T: AssetProcessor> ErasedAssetProcessor for T {
    fn process_erased(&self, src: File, out: File, opts: &serde_json::Value) {
        let opts: T::Options = serde_json::from_value(opts.clone()).unwrap();
        self.process(src, out, opts);
    }
}

fn build_log(status: &str, msg: &str) {
    println!(
        // "cargo::warning=\r\x1b[K\x1b[1;32m{:>12}\x1b[0m {}",
        "\x1b[K\x1b[1;32m{:>12}\x1b[0m {}",
        status, msg
    );
}
