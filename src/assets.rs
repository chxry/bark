use crate::TypeIdNamed;
use crate::ecs::World;
use crate::job::ThreadPool;
use std::any::{self, Any};
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::sync::{Arc, OnceLock, Weak};
use tracing::{debug, trace};

pub fn init(world: &mut World) {
    world.insert_resource(Assets::new());
}

pub struct Assets {
    thread_pool: ThreadPool,
    types: HashMap<TypeIdNamed, Box<dyn Any>>,
}

impl Assets {
    pub fn new() -> Self {
        Self {
            thread_pool: ThreadPool::new(),
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
                            data: OnceLock::new(),
                        }));

                        let loader = ty.loader.clone();
                        let handle2 = handle.clone();
                        self.thread_pool.execute(move || {
                            trace!("loading {:?} as {:?}", handle2.id(), any::type_name::<T>());
                            let asset = (loader)(Box::new(File::open(handle2.id()).unwrap()));
                            let _ = handle2.0.data.set(asset);
                        });

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

pub trait AssetLoader<T: Any> = Fn(Box<dyn Read>) -> T + Send + Sync + 'static;

pub struct Handle<T>(Arc<HandleData<T>>);

impl<T> Handle<T> {
    pub fn loaded(&self) -> bool {
        self.0.data.get().is_some()
    }
    pub fn try_get(&self) -> Option<&T> {
        self.0.data.get()
    }

    pub fn get(&self) -> &T {
        self.try_get().unwrap()
    }

    pub fn id(&self) -> &AssetId {
        &self.0.id
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
    data: OnceLock<T>,
}

struct AssetType<T: Any> {
    loader: Arc<dyn AssetLoader<T>>,
    storage: HashMap<AssetId, Weak<HandleData<T>>>,
}

impl<T: Any> AssetType<T> {
    fn new<F: AssetLoader<T>>(loader: F) -> Self {
        Self {
            loader: Arc::new(loader),
            storage: HashMap::new(),
        }
    }
}

type AssetId = String;
