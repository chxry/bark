pub mod app;
pub mod ecs;
pub mod gfx;

use crate::ecs::World;
use std::any::{self, Any, TypeId};
use std::fmt;
use std::hash::{Hash, Hasher};

pub fn init(_: &mut World) {}

#[derive(Eq, Copy, Clone)]
pub struct TypeIdNamed {
    id: TypeId,
    name: &'static str,
}

impl TypeIdNamed {
    fn of<T: Any + ?Sized>() -> Self {
        Self {
            id: TypeId::of::<T>(),
            name: any::type_name::<T>(),
        }
    }
}

impl Hash for TypeIdNamed {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl PartialEq for TypeIdNamed {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl fmt::Debug for TypeIdNamed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.name.fmt(f)
    }
}
