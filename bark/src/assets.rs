use crate::App;
use rayon::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::any::{self, Any};
use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, Weak};
use tracing::{debug, trace_span};
use twox_hash::XxHash3_64;

const MANIFEST_FILE: &str = "manifest.json";
const CACHE_DIR: &str = ".bark-cache";

pub fn init<P: Into<PathBuf>>(app: &mut App, assets_dir: P) {
    app.world.insert_resource(Assets::new(assets_dir.into()))
}

pub struct Assets {
    manifest: Manifest,
    cache_dir: PathBuf,
    thread_pool: rayon::ThreadPool,
    storage: HashMap<String, Weak<dyn Any + Send + Sync>>,
}

impl Assets {
    fn new(assets_dir: PathBuf) -> Self {
        let manifest_path = assets_dir.join(MANIFEST_FILE);
        let cache_dir = assets_dir.join(CACHE_DIR);
        let manifest = serde_json::from_reader(File::open(&manifest_path).unwrap()).unwrap();
        debug!("loaded manifest from {:?}", manifest_path);

        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .thread_name(|i| format!("asset-io-{i}"))
            .build()
            .unwrap();
        debug!(
            "using {} asset loader threads",
            thread_pool.current_num_threads()
        );

        Self {
            manifest,
            cache_dir,
            thread_pool,
            storage: HashMap::new(),
        }
    }

    pub fn load<T: Asset>(&mut self, id: &str) -> Handle<T> {
        match self.storage.get(id).and_then(|h| h.upgrade()) {
            Some(h) => Handle(h.downcast().unwrap()),
            None => {
                debug!("load asset {:?}", id);
                let entry = self.manifest.0.get(id).unwrap();
                let path = self.cache_dir.join(hex::encode(entry.hash.to_be_bytes()));
                let handle = Handle(Arc::new((id.to_owned(), OnceLock::new())));

                let handle2 = handle.clone();
                self.thread_pool.spawn(move || {
                    let _span =
                        trace_span!("load asset", id = handle2.id(), type = any::type_name::<T>());
                    handle2.set(T::read(File::open(path).unwrap()));
                });

                self.storage.insert(
                    id.to_owned(),
                    Arc::downgrade(&handle.0) as Weak<dyn Any + Send + Sync>,
                );
                handle
            }
        }
    }
}

pub trait Asset: Any + Send + Sync {
    fn read<R: Read>(reader: R) -> Self;
}

pub struct Handle<T>(Arc<(String, OnceLock<T>)>);

impl<T> Handle<T> {
    pub fn id(&self) -> &str {
        &self.0.0
    }

    pub fn loaded(&self) -> bool {
        self.0.1.get().is_some()
    }

    pub fn try_get(&self) -> Option<&T> {
        self.0.1.get()
    }

    pub fn get(&self) -> &T {
        self.try_get().unwrap()
    }

    fn set(&self, data: T) {
        let _ = self.0.1.set(data);
    }
}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state)
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle(self.0.clone())
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
        self.processors.insert(ty.to_owned(), Box::new(processor));
    }

    pub fn run(mut self) {
        fs::create_dir_all(&self.cache_dir).unwrap();
        let hashes = self
            .manifest
            .0
            .par_iter()
            .map(|(id, entry)| {
                let src_path = self.assets_dir.join(id);
                let bytes = fs::read(&src_path).unwrap();

                let hash = hash_asset(&bytes, &entry.options);
                let cache_path = self.cache_dir.join(hex::encode(hash.to_be_bytes()));

                if entry.hash == hash && cache_path.exists() {
                    return None;
                }

                build_log("Processing", &format!("{} ({})", id, entry.ty));
                let processor = self.processors.get(&entry.ty).unwrap();
                processor.process_erased(
                    File::open(&src_path).unwrap(),
                    File::create(&cache_path).unwrap(),
                    &entry.options,
                );
                Some(hash)
            })
            .collect::<Vec<_>>();

        let mut n_processed = 0;
        for (entry, out) in self.manifest.0.values_mut().zip(hashes) {
            if let Some(hash) = out {
                entry.hash = hash;
                n_processed += 1;
            }
        }
        build_log(
            "Processed",
            &format!(
                "{} assets, {} cached",
                n_processed,
                self.manifest.0.len() - n_processed
            ),
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
        "\x1b[1;32m{:>12}\x1b[0m {}",
        status, msg
    );
}
