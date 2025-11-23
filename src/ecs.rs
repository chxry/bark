use crate::{TypeIdNamed, catch_panic};
use std::any::{self, Any};
use std::cell::UnsafeCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use tracing::{debug, error, trace};

// todo check the unsafecells

#[derive(Default)]
pub struct World {
    entity_id: EntityId,
    components: HashMap<TypeIdNamed, Box<UnsafeCell<dyn Any>>>,
    resources: HashMap<TypeIdNamed, Box<UnsafeCell<dyn Any>>>,
    systems: HashMap<TypeIdNamed, SystemInfo>,
    systems_queue: VecDeque<TypeIdNamed>,
    event_handlers: HashMap<TypeIdNamed, Box<dyn Any>>,
}

impl World {
    pub fn spawn(&mut self) -> EntityHandle<'_> {
        self.entity_id.0 += 1;
        debug!("spawn entity {:?}", self.entity_id);
        EntityHandle {
            id: self.entity_id,
            world: self,
        }
    }

    fn insert_component<T: Any>(&mut self, entity_id: EntityId, data: T) {
        let type_id = TypeIdNamed::of::<T>();
        let storage = self
            .components
            .entry(type_id)
            .or_insert_with(|| Box::new(UnsafeCell::new(DenseStorage::<T>::new())))
            .get_mut();
        let storage = storage.downcast_mut::<DenseStorage<_>>().unwrap();
        storage.insert(entity_id, data);
    }

    pub fn get<T: Any>(&self) -> impl Iterator<Item = (EntityId, &T)> {
        self.components
            .get(&TypeIdNamed::of::<T>())
            .map(|s| {
                unsafe { s.as_ref_unchecked() }
                    .downcast_ref::<DenseStorage<T>>()
                    .unwrap()
                    .iter()
            })
            .into_iter()
            .flatten()
    }

    pub fn get_mut<T: Any>(&self) -> impl Iterator<Item = (EntityId, &mut T)> {
        self.components
            .get(&TypeIdNamed::of::<T>())
            .map(|s| {
                unsafe { s.as_mut_unchecked() }
                    .downcast_mut::<DenseStorage<T>>()
                    .unwrap()
                    .iter_mut()
            })
            .into_iter()
            .flatten()
    }

    pub fn insert_resource<T: Any>(&mut self, data: T) {
        let id = TypeIdNamed::of::<T>();
        trace!("insert resource {:?}", id);
        self.resources.insert(id, Box::new(UnsafeCell::new(data)));
    }

    pub fn get_resource<T: Any>(&self) -> Option<&T> {
        self.resources
            .get(&TypeIdNamed::of::<T>())
            .map(|r| unsafe { r.as_ref_unchecked() }.downcast_ref().unwrap())
    }

    pub fn get_resource_mut<T: Any>(&self) -> Option<&mut T> {
        self.resources
            .get(&TypeIdNamed::of::<T>())
            .map(|r| unsafe { r.as_mut_unchecked() }.downcast_mut().unwrap())
    }

    pub fn remove_resource<T: Any>(&mut self) -> Option<T> {
        self.resources.remove(&TypeIdNamed::of::<T>()).map(|r| {
            *unsafe { Box::from_raw(Box::into_raw(r).as_mut().unwrap().get()) }
                .downcast()
                .unwrap()
        })
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
                error!("system cycle on {:?}", id);
                return;
            }
            None => {
                visited.insert(id, false);
            }
        }

        let Some(info) = self.systems.get(&id).cloned() else {
            error!("unknown system {:?}", id);
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
                    trace!("run system {:?}", id);
                    catch_panic(|| (info.sys)(self), id.name);
                }
                None => {
                    error!("unknown system {:?}", id);
                }
            }
        }
    }

    pub fn queue_and_run<S: System>(&mut self, sys: S) {
        self.queue_system(sys);
        self.run_queued();
    }

    pub fn add_event_handler<F: EventHandler<T>, T: Any>(&mut self, handler: F) {
        let id = TypeIdNamed::of::<T>();
        let handlers = self.event_handlers.entry(id).or_insert_with(|| {
            debug!("new event type {:?}", id);
            Box::<EventHandlerStorage<T>>::new(vec![])
        });
        handlers
            .downcast_mut::<EventHandlerStorage<T>>()
            .unwrap()
            .push(Rc::new(handler));
    }

    pub fn handle_event<T: Any>(&mut self, event: T) {
        let id = TypeIdNamed::of::<T>();
        trace!("handle event {:?}", id);
        if let Some(handlers) = self.event_handlers.get(&id) {
            let handlers = handlers
                .downcast_ref::<EventHandlerStorage<T>>()
                .unwrap()
                .clone();
            for h in handlers {
                catch_panic(|| h(self, &event), id.name);
            }
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct EntityId(u64);

pub struct EntityHandle<'w> {
    pub id: EntityId,
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
// vecs should be sorted by entityid
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

    fn iter_mut(&mut self) -> impl Iterator<Item = (EntityId, &mut T)> {
        self.entities
            .iter()
            .zip(self.data.iter_mut())
            .map(|(&e, c)| (e, c))
    }
}

pub trait System = Fn(&mut World) + 'static;

pub trait EventHandler<T: Any> = Fn(&mut World, &T) + 'static;

type EventHandlerStorage<T> = Vec<Rc<dyn EventHandler<T>>>;

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
