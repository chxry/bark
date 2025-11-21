use crate::TypeIdNamed;
use crate::ecs::World;
use std::any::{self, Any};
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::ops::Deref;
use std::rc::{Rc, Weak};
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

    pub fn load<T: Any>(&mut self, path: &str) -> Handle<T> {
        let id = TypeIdNamed::of::<T>();
        match self.types.get_mut(&id) {
            Some(ty) => {
                let ty = ty.downcast_mut::<AssetType<T>>().unwrap();
                let data = match ty.storage.get(path).and_then(|h| h.upgrade()) {
                    Some(h) => h,
                    None => {
                        trace!("load {:?} as {:?}", path, id);
                        let asset = Rc::new((ty.loader)(Box::new(File::open(path).unwrap())));
                        ty.storage.insert(path.to_string(), Rc::downgrade(&asset));
                        asset
                    }
                };
                Handle {
                    data,
                    id: AssetId(path.to_string()),
                }
            }
            None => {
                panic!("no asset loader for {:?}", id);
            }
        }
    }
}

pub trait AssetLoader<T: Any> = Fn(Box<dyn Read>) -> T + 'static;

pub struct Handle<T> {
    id: AssetId,
    data: Rc<T>,
}

impl<T> Deref for Handle<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle {
            id: self.id.clone(),
            data: self.data.clone(),
        }
    }
}

struct AssetType<T: Any> {
    loader: Box<dyn AssetLoader<T>>,
    storage: HashMap<String, Weak<T>>,
}

impl<T: Any> AssetType<T> {
    fn new<F: AssetLoader<T>>(loader: F) -> Self {
        Self {
            loader: Box::new(loader),
            storage: HashMap::new(),
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct AssetId(String);

impl AssetId {
    pub fn of<T>(handle: &Handle<T>) -> Self {
        handle.id.clone()
    }
}

impl fmt::Debug for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
