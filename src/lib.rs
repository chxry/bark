#![feature(unsafe_cell_access, trait_alias, mpmc_channel)]
pub mod app;
pub mod assets;
pub mod bark3d;
pub mod ecs;
pub mod gfx;
pub mod job;

use std::any::{self, Any, TypeId};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::iter::Peekable;
use std::{fmt, mem, panic, slice};
use tracing::error;

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

pub fn intersect<K: Eq + Ord, T, U, A: Iterator<Item = (K, T)>, B: Iterator<Item = (K, U)>>(
    a: A,
    b: B,
) -> Intersect<K, T, U, A, B> {
    Intersect {
        a: a.peekable(),
        b: b.peekable(),
    }
}

pub struct Intersect<K: Eq + Ord, T, U, A: Iterator<Item = (K, T)>, B: Iterator<Item = (K, U)>> {
    a: Peekable<A>,
    b: Peekable<B>,
}

impl<K: Eq + Ord, T, U, A: Iterator<Item = (K, T)>, B: Iterator<Item = (K, U)>> Iterator
    for Intersect<K, T, U, A, B>
{
    type Item = (K, (T, U));

    fn next(&mut self) -> Option<Self::Item> {
        while let (Some((ka, _)), Some((kb, _))) = (self.a.peek(), self.b.peek()) {
            match ka.cmp(kb) {
                Ordering::Less => {
                    self.a.next();
                }
                Ordering::Greater => {
                    self.b.next();
                }
                Ordering::Equal => {
                    let (k, t) = self.a.next().unwrap();
                    let (_, u) = self.b.next().unwrap();
                    return Some((k, (t, u)));
                }
            }
        }
        None
    }
}

pub fn catch_panic<F: FnOnce()>(f: F, name: &'static str) {
    panic::set_hook(Box::new(move |info| {
        error!("{:?} panicked:\n{}", name, info);
    }));
    f();
    let _ = panic::take_hook();
}

pub fn cast_bytes_slice<T>(t: &[T]) -> &[u8] {
    // safety: u8 is always valid
    unsafe { slice::from_raw_parts(t.as_ptr() as _, mem::size_of_val(t)) }
}

pub fn cast_bytes<T>(t: &T) -> &[u8] {
    cast_bytes_slice(slice::from_ref(t))
}
