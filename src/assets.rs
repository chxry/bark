use crate::TypeIdNamed;
use crate::app;
use crate::bark3d::mesh::{process_mesh, save_mesh};
use crate::bark3d::texture::{process_texture, save_texture};
use crate::ecs::World;
use crate::job::ThreadPool;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::any::{self, Any};
use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
// use std::sync::OnceLock;
use std::sync::{Arc, Weak};
use std::time::Duration;
use std::{fmt, thread};
use tracing::debug;

type AssetId = String;

const ASSET_DIR: &str = "assets";
const MANIFEST_PATH: &str = "assets/manifest.json";
const CACHE_DIR: &str = ".bark-cache";

pub fn init(world: &mut World) {
    world.insert_resource(Assets::new());
    world.add_event_handler(exit_save_manifest);
}

pub struct Assets {
    provider: AssetProvider,
    types: HashMap<TypeIdNamed, Box<dyn Any>>,
}

impl Assets {
    pub fn new() -> Self {
        Self {
            provider: AssetProvider::new(),
            types: HashMap::new(),
        }
    }

    pub fn register_loader<F: AssetLoader<T>, T>(&mut self, loader: F) {
        let id = TypeIdNamed::of::<T>();
        debug!("new asset loader {:?} for {:?}", any::type_name::<F>(), id);
        self.types.insert(id, Box::new(AssetType::new(loader)));
    }

    pub fn load<T: Any + Send + Sync>(&mut self, path: &str) -> Handle<T> {
        let id = TypeIdNamed::of::<T>();
        match self.types.get_mut(&id) {
            Some(ty) => {
                let ty = ty.downcast_mut::<AssetType<T>>().unwrap();
                match ty.storage.get(path).and_then(|h| h.upgrade()) {
                    Some(h) => Handle(h),
                    None => {
                        let handle = Handle(Arc::new(HandleData {
                            id: path.to_string(),
                            data: (ty.loader)(&self.provider.get(&path.to_string())),
                        }));

                        ty.storage
                            .insert(handle.id().clone(), Arc::downgrade(&handle.0));
                        handle
                    }
                }
            }
            None => {
                panic!("no asset loader for {:?}", id);
            }
        }
    }
}

pub trait AssetLoader<T: Any> = Fn(&[u8]) -> T + Send + Sync + 'static;

pub struct Handle<T>(Arc<HandleData<T>>);

impl<T> Handle<T> {
    pub fn id(&self) -> &AssetId {
        &self.0.id
    }
}

impl<T> Deref for Handle<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0.data
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.id.hash(state)
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle(self.0.clone())
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.id.fmt(f)
    }
}

struct HandleData<T> {
    id: AssetId,
    data: T,
}

struct AssetType<T: Any> {
    loader: Box<dyn AssetLoader<T>>,
    storage: HashMap<AssetId, Weak<HandleData<T>>>,
}

impl<T: Any> AssetType<T> {
    fn new<F: AssetLoader<T>>(loader: F) -> Self {
        Self {
            loader: Box::new(loader),
            storage: HashMap::new(),
        }
    }
}

type AssetProvider = DebugAssetProvider;

struct DebugAssetProvider {
    asset_server: AssetServer,
}

impl DebugAssetProvider {
    fn new() -> Self {
        Self {
            asset_server: AssetServer::new(),
        }
    }

    fn get(&self, id: &AssetId) -> Vec<u8> {
        self.asset_server.get_asset(id.as_str())
    }
}

struct AssetServer {
    manifest: Manifest,
    thread_pool: ThreadPool,
}

impl AssetServer {
    fn new() -> Self {
        let file = File::open(MANIFEST_PATH).unwrap();
        let manifest: Manifest = serde_json::from_reader(&file).unwrap();
        fs::create_dir_all(CACHE_DIR).unwrap();

        let thread_pool = ThreadPool::new();
        for (path, entry) in manifest.clone() {
            thread_pool.execute(move || {
                let raw_path = format!("{}/{}", ASSET_DIR, path);
                // verify manifest hash
                if !fs::exists(entry.cached_path()).unwrap() {
                    tracing::debug!("process {} asset {:?}", entry.ty, path);
                    let raw = fs::read(raw_path).unwrap();
                    // todo
                    let data = match entry.ty.as_str() {
                        "texture" => save_texture(&process_texture(&raw)),
                        "mesh" => save_mesh(&process_mesh(&raw)),
                        _ => panic!(),
                    };
                    fs::write(entry.cached_path(), data).unwrap();
                }
            });
        }

        Self {
            manifest,
            thread_pool,
        }
    }

    fn get_asset(&self, path: &str) -> Vec<u8> {
        let path = self.manifest.get(path).unwrap().cached_path();
        while !fs::exists(&path).unwrap() {
            thread::sleep(Duration::from_secs(1));
        }
        fs::read(path).unwrap()
    }

    fn save_manifest(&self) {
        let file = File::options()
            .write(true)
            .truncate(true)
            .open(MANIFEST_PATH)
            .unwrap();
        serde_json::to_writer_pretty(&file, &self.manifest).unwrap();
        debug!("saved asset manifest");
    }
}

fn exit_save_manifest(world: &mut World, _: &app::ExitEvent) {
    world
        .get_resource::<Assets>()
        .unwrap()
        .provider
        .asset_server
        .save_manifest();
}

type Manifest = HashMap<String, ManifestEntry>;

#[derive(Serialize, Deserialize, Clone)]
struct ManifestEntry {
    ty: String,
    #[serde(
        serialize_with = "serialize_hash",
        deserialize_with = "deserialize_hash"
    )]
    hash: u64,
}

impl ManifestEntry {
    fn cached_path(&self) -> String {
        format!("{}/{:x}", CACHE_DIR, self.hash)
    }
}

fn serialize_hash<S: Serializer>(data: &u64, se: S) -> Result<S::Ok, S::Error> {
    hex::serialize(data.to_be_bytes(), se)
}

fn deserialize_hash<'de, D: Deserializer<'de>>(de: D) -> Result<u64, D::Error> {
    hex::deserialize(de).map(u64::from_be_bytes)
}
