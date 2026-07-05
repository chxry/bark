use crate::App;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::any::{self, Any};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, Weak};
use tracing::{debug, trace_span};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const CACHE_DIR: &str = ".bark-cache";

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

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle(self.0.clone())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Manifest(pub BTreeMap<String, ManifestEntry>);

#[derive(Serialize, Deserialize)]
pub struct ManifestEntry {
    pub ty: String,
    #[serde(
        serialize_with = "serialize_hash",
        deserialize_with = "deserialize_hash"
    )]
    pub hash: u64,
    pub options: serde_json::Value,
}

fn serialize_hash<S: Serializer>(data: &u64, se: S) -> Result<S::Ok, S::Error> {
    hex::serialize(data.to_be_bytes(), se)
}

fn deserialize_hash<'de, D: Deserializer<'de>>(de: D) -> Result<u64, D::Error> {
    hex::deserialize(de).map(u64::from_be_bytes)
}

pub fn hash_to_string(hash: u64) -> String {
    hex::encode(hash.to_be_bytes())
}

pub trait AssetProcessor: Send + Sync {
    type Options: Serialize + DeserializeOwned;

    fn process<R: Read, W: Write>(&self, src: R, out: W, opts: Self::Options);
}
