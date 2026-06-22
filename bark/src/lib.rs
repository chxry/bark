pub mod app;
pub mod assets;
pub mod bark3d;
pub mod ecs;
pub mod gfx;

use std::any::{self, Any, TypeId};
use std::hash::{Hash, Hasher};
use std::{fmt, mem, slice};

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

pub fn cast_bytes_slice<T>(x: &[T]) -> &[u8] {
    // safety: u8 is always valid
    unsafe { slice::from_raw_parts(x.as_ptr() as _, mem::size_of_val(x)) }
}

pub fn cast_bytes<T>(x: &T) -> &[u8] {
    cast_bytes_slice(slice::from_ref(x))
}
