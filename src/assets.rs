use crate::TypeIdNamed;
use crate::ecs::World;
use std::any::{self, Any};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::ops::Deref;
use std::sync::{Arc, OnceLock, Weak};
use std::{fmt, thread};
use tracing::{debug, trace};

pub fn init(world: &mut World) {
    world.insert_resource(Assets::default());
}

#[derive(Default)]
pub struct Assets {
    types: HashMap<TypeIdNamed, Box<dyn Any>>,
}

impl Assets {
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
                let data = match ty.storage.get(path).and_then(|h| h.upgrade()) {
                    Some(h) => h,
                    None => {
                        let handle = Arc::new(OnceLock::new());
                        load_thread(path.to_string(), ty.loader.clone(), handle.clone());
                        // thread::spawn(move || {
                        //     trace!("load {:?} as {:?}", path, id);
                        //     // let asset = (ty.loader)(Box::new(File::open(path).unwrap()));
                        // });
                        ty.storage.insert(path.to_string(), Arc::downgrade(&handle));
                        handle
                    }
                };
                Handle {
                    id: AssetId(path.to_string()),
                    data,
                }
            }
            None => {
                panic!("no asset loader for {:?}", id);
            }
        }
    }
}

fn load_thread<T: Any + Send + Sync>(
    path: String,
    loader: Arc<dyn AssetLoader<T>>,
    handle: Arc<OnceLock<T>>,
) {
    thread::spawn(move || {
        trace!("loading {:?} as {:?}", path, any::type_name::<T>());
        let asset = loader(Box::new(File::open(path).unwrap()));
        let _ = handle.set(asset);
    });
}

pub trait AssetLoader<T: Any> = Fn(Box<dyn Read>) -> T + Send + Sync + 'static;

pub struct Handle<T> {
    id: AssetId,
    data: Arc<OnceLock<T>>,
}

impl<T> Handle<T> {
    pub fn loaded(&self) -> bool {
        self.data.get().is_some()
    }
    pub fn try_get(&self) -> Option<&T> {
        self.data.get()
    }

    pub fn get(&self) -> &T {
        self.try_get().unwrap()
    }

    pub fn id(&self) -> AssetId {
        self.id.clone()
    }
}

// impl<T> Deref for Handle<T> {
//     type Target = T;

//     fn deref(&self) -> &T {
//         self.data.get().unwrap()
//     }
// }

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle {
            id: self.id.clone(),
            data: self.data.clone(),
        }
    }
}

struct AssetType<T: Any> {
    loader: Arc<dyn AssetLoader<T>>,
    storage: HashMap<String, Weak<OnceLock<T>>>,
}

impl<T: Any> AssetType<T> {
    fn new<F: AssetLoader<T>>(loader: F) -> Self {
        Self {
            loader: Arc::new(loader),
            storage: HashMap::new(),
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct AssetId(String);

// impl AssetId {
//     pub fn of<T>(handle: &Handle<T>) -> Self {
//         handle.id.clone()
//     }
// }

impl fmt::Debug for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
