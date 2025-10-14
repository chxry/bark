pub mod ecs;
use crate::ecs::{System, World};
use std::any::{self, Any, TypeId};
use std::hash::{Hash, Hasher};

pub fn init(_: &mut World) {}

#[derive(Default)]
pub struct App {
    world: World,
}

impl App {
    pub fn insert_resource<T: Any>(mut self, data: T) -> Self {
        self.world.insert_resource(data);
        self
    }

    pub fn insert_system_with<S: System, T: System>(mut self, with: S, sys: T) -> Self {
        self.world.insert_system_with(with, sys);
        self
    }

    pub fn insert_system_before<S: System, T: System>(mut self, before: S, sys: T) -> Self {
        self.world.insert_system_before(before, sys);
        self
    }

    pub fn run(mut self) {
        self.world.run_system(init);
    }
}

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
