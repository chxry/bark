pub mod app;
pub mod assets;
pub mod ecs;
pub mod gfx;

use std::any::{self, Any, TypeId};
use std::fmt;
use std::hash::{Hash, Hasher};

pub use app::App;

#[derive(Eq, Copy, Clone)]
pub struct TypeKey {
    id: TypeId,
    name: &'static str,
}

impl TypeKey {
    fn of<T: Any + ?Sized>() -> Self {
        Self {
            id: TypeId::of::<T>(),
            name: any::type_name::<T>(),
        }
    }
}

impl Hash for TypeKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for TypeKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl fmt::Debug for TypeKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.name.fmt(f)
    }
}
