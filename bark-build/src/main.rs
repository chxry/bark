mod gltf;
mod mesh;
mod texture;
mod wesl;

use bark::assets::{self, hash_to_string};
use rayon::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::{env, process};
use twox_hash::XxHash3_64;

fn main() {
    match env::args().nth(1) {
        Some(dir) => {
            let mut assets = AssetProcessors::new(dir);
            assets.register("texture", texture::TextureProcessor);
            assets.register("mesh", mesh::MeshProcessor);
            assets.register("wesl", wesl::WeslProcessor);
            assets.register("gltf", gltf::GltfProcessor);
            assets.run();
        }
        None => {
            eprintln!("usage: bark-build <assets-dir>");
            process::exit(1);
        }
    }
}

struct AssetProcessors {
    manifest: assets::Manifest,
    assets_dir: PathBuf,
    cache_dir: PathBuf,
    processors: HashMap<String, Box<dyn ErasedAssetProcessor>>,
}

impl AssetProcessors {
    fn new<P: Into<PathBuf>>(assets_dir: P) -> Self {
        let assets_dir = assets_dir.into();
        let manifest_path = assets_dir.join(assets::MANIFEST_FILE);
        let cache_dir = assets_dir.join(assets::CACHE_DIR);

        let manifest = serde_json::from_reader(File::open(&manifest_path).unwrap()).unwrap();
        Self {
            manifest,
            assets_dir,
            cache_dir,
            processors: HashMap::new(),
        }
    }

    fn register<T: AssetProcessor + 'static>(&mut self, ty: &str, processor: T) {
        self.processors.insert(ty.to_owned(), Box::new(processor));
    }

    fn run(mut self) {
        fs::create_dir_all(&self.cache_dir).unwrap();
        let hashes = self
            .manifest
            .0
            .par_iter()
            .map(|(id, entry)| {
                let src_path = self.assets_dir.join(id);
                let src_data = fs::read(&src_path).unwrap();

                let hash = hash_asset(&src_data, &entry.options);
                let cache_path = self.cache_dir.join(hash_to_string(hash));

                if entry.hash == hash && cache_path.exists() {
                    return None;
                }

                build_log("Processing", &format!("{} ({})", id, entry.ty));
                let processor = self.processors.get(&entry.ty).unwrap();
                processor.process_erased(
                    AssetProcessorContext {
                        src_data: &src_data,
                        src_path: &src_path,
                        out_path: &cache_path,
                    },
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

        let manifest_path = self.assets_dir.join(assets::MANIFEST_FILE);
        serde_json::to_writer_pretty(&File::create(&manifest_path).unwrap(), &self.manifest)
            .unwrap();
    }
}

pub struct AssetProcessorContext<'a> {
    pub src_data: &'a [u8],
    pub src_path: &'a Path,
    pub out_path: &'a Path,
}

impl AssetProcessorContext<'_> {
    pub fn emit_main(&self) -> File {
        File::create(self.out_path).unwrap()
    }

    pub fn emit_sub(&self, name: &str) -> File {
        File::create(self.out_path.parent().unwrap().join(format!(
            "{}#{}",
            self.out_path.file_name().unwrap().display(),
            name
        )))
        .unwrap()
    }
}

pub trait AssetProcessor: Send + Sync {
    type Options: Serialize + DeserializeOwned;

    fn process(&self, ctx: AssetProcessorContext, opts: Self::Options);
}

trait ErasedAssetProcessor: Send + Sync {
    fn process_erased(&self, ctx: AssetProcessorContext, opts: &serde_json::Value);
}

impl<T: AssetProcessor> ErasedAssetProcessor for T {
    fn process_erased(&self, ctx: AssetProcessorContext, opts: &serde_json::Value) {
        let opts = serde_json::from_value(opts.clone()).unwrap();
        self.process(ctx, opts);
    }
}

fn hash_asset(data: &[u8], opts: &serde_json::Value) -> u64 {
    let mut hasher = XxHash3_64::new();
    data.hash(&mut hasher);
    opts.hash(&mut hasher);
    hasher.finish()
}

fn build_log(status: &str, msg: &str) {
    println!(
        // "cargo::warning=\r\x1b[K\x1b[1;32m{:>12}\x1b[0m {}",
        "\x1b[1;32m{:>12}\x1b[0m {}",
        status, msg
    );
}
