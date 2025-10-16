use crate::{TypeIdNamed, init};
use std::any::{self, Any};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use tracing::{debug, error, trace};

#[derive(Default)]
pub struct World {
    entity_id: u64,
    components: HashMap<TypeIdNamed, Box<dyn Any>>,
    resources: HashMap<TypeIdNamed, Box<dyn Any>>,
    systems: HashMap<TypeIdNamed, SystemInfo>,
    systems_queue: VecDeque<TypeIdNamed>,
}

type Storage<T> = DenseStorage<T>;

impl World {
    pub fn spawn(&mut self) -> EntityHandle<'_> {
        self.entity_id += 1;
        debug!("spawn entity {}", self.entity_id);
        EntityHandle {
            id: EntityId(self.entity_id),
            world: self,
        }
    }

    fn insert_component<T: Any>(&mut self, entity_id: EntityId, data: T) {
        let type_id = TypeIdNamed::of::<T>();
        let storage = self
            .components
            .entry(type_id)
            .or_insert_with(|| Box::new(Storage::<T>::new()));
        let storage = storage.downcast_mut::<Storage<_>>().unwrap();
        storage.insert(entity_id, data);
    }

    pub fn get<T: Any>(&self) -> impl Iterator<Item = (EntityId, &T)> {
        self.components
            .get(&TypeIdNamed::of::<T>())
            .map(|s| s.downcast_ref::<Storage<_>>().unwrap().iter())
            .into_iter()
            .flatten()
    }

    pub fn insert_resource<T: Any>(&mut self, data: T) {
        self.resources
            .insert(TypeIdNamed::of::<T>(), Box::new(data));
    }

    pub fn get_resource<T: Any>(&self) -> Option<&T> {
        self.resources
            .get(&TypeIdNamed::of::<T>())
            .map(|r| r.downcast_ref().unwrap())
    }

    pub fn get_resource_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.resources
            .get_mut(&TypeIdNamed::of::<T>())
            .map(|r| r.downcast_mut().unwrap())
    }

    pub fn remove_resource<T: Any>(&mut self) -> Option<T> {
        self.resources
            .remove(&TypeIdNamed::of::<T>())
            .map(|r| *r.downcast().unwrap())
    }

    fn insert_system<S: System>(&mut self, sys: S) -> &mut SystemInfo {
        self.systems
            .entry(TypeIdNamed::of::<S>())
            .or_insert(SystemInfo::new(sys))
    }

    pub fn insert_system_with<S: System, T: System>(&mut self, with: S, sys: T) {
        self.insert_system(with).with.push(TypeIdNamed::of::<T>());
        self.insert_system(sys);
    }

    pub fn insert_system_before<S: System, T: System>(&mut self, before: S, sys: T) {
        self.insert_system(before)
            .depends
            .push(TypeIdNamed::of::<T>());
        self.insert_system(sys);
    }

    pub fn queue_system<S: System>(&mut self, sys: S) {
        let id = TypeIdNamed::of::<S>();
        let mut visited = HashMap::new();
        self.insert_system(sys);
        self.queue_deps(id, &mut visited);
    }

    pub fn queue_system_once<S: System>(&mut self, sys: S) {
        let id = TypeIdNamed::of::<S>();
        if !self.systems_queue.contains(&id) {
            self.queue_system(sys);
        }
    }

    fn queue_deps(&mut self, id: TypeIdNamed, visited: &mut HashMap<TypeIdNamed, bool>) {
        match visited.get(&id) {
            Some(true) => return,
            Some(false) => {
                error!("system cycle on {:?}", id.name);
                return;
            }
            None => {
                visited.insert(id, false);
            }
        }

        let Some(info) = self.systems.get(&id).cloned() else {
            error!("unknown system {:?}", id.name);
            return;
        };

        for dep in &info.depends {
            self.queue_deps(*dep, visited);
        }

        self.systems_queue.push_back(id);
        visited.insert(id, true);

        for with in &info.with {
            self.queue_deps(*with, visited);
        }
    }

    pub fn run_queued(&mut self) {
        while let Some(id) = self.systems_queue.pop_front() {
            match self.systems.get(&id).cloned() {
                Some(info) => {
                    trace!("run system {:?}", id.name);
                    info.sys.run(self);
                }
                None => {
                    error!("unknown system {:?}", id.name);
                }
            }
        }
    }

    pub fn queue_and_run<S: System>(&mut self, sys: S) {
        self.queue_system(sys);
        self.run_queued();
    }

    pub fn run(mut self) {
        self.queue_and_run(init);
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct EntityId(u64);

pub struct EntityHandle<'w> {
    id: EntityId,
    world: &'w mut World,
}

impl EntityHandle<'_> {
    pub fn insert<T: Any>(&mut self, component: T) -> &mut Self {
        self.world.insert_component(self.id, component);
        self
    }
}

// struct SparseStorage<T: Any> {
//     data: HashMap<EntityId, T>,
// }

// impl<T: Any> SparseStorage<T> {
//     fn new() -> Self {
//         debug!("new component type (sparse) {:?}", any::type_name::<T>());
//         Self {
//             data: HashMap::new(),
//         }
//     }

//     fn insert(&mut self, id: EntityId, comp: T) {
//         self.data.insert(id, comp);
//     }

//     fn iter(&self) -> impl Iterator<Item = (EntityId, &T)> {
//         self.data.iter().map(|(&e, c)| (e, c))
//     }
// }

// todo benchmark
struct DenseStorage<T: Any> {
    entities: Vec<EntityId>,
    data: Vec<T>,
    index: HashMap<EntityId, usize>,
}

impl<T: Any> DenseStorage<T> {
    fn new() -> Self {
        debug!("new component type (dense) {:?}", any::type_name::<T>());
        Self {
            entities: vec![],
            data: vec![],
            index: HashMap::new(),
        }
    }

    fn insert(&mut self, entity: EntityId, comp: T) {
        if let Some(&i) = self.index.get(&entity) {
            self.data[i] = comp;
        } else {
            let i = self.data.len();
            self.entities.push(entity);
            self.data.push(comp);
            self.index.insert(entity, i);
        }
    }

    fn iter(&self) -> impl Iterator<Item = (EntityId, &T)> {
        self.entities
            .iter()
            .zip(self.data.iter())
            .map(|(&e, c)| (e, c))
    }
}

pub trait System: 'static {
    fn run(&self, world: &mut World);
}

impl<F: Fn(&mut World) + 'static> System for F {
    fn run(&self, world: &mut World) {
        (self)(world);
    }
}

#[derive(Clone)]
struct SystemInfo {
    sys: Rc<dyn System>,
    with: Vec<TypeIdNamed>,
    depends: Vec<TypeIdNamed>,
}

impl SystemInfo {
    fn new<S: System>(sys: S) -> Self {
        Self {
            sys: Rc::new(sys),
            with: vec![],
            depends: vec![],
        }
    }
}
