use crate::TypeIdNamed;
use std::any::Any;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::VecDeque;
use std::collections::hash_map::{Entry, HashMap};
use std::rc::Rc;
use tracing::trace;

#[derive(Default)]
pub struct World {
    entity_id: u64,
    components: HashMap<(TypeIdNamed, EntityId), Component>,
    component_index: HashMap<TypeIdNamed, Vec<(EntityId, Component)>>,
    // entity_index: HashMap<EntityId, Vec<(TypeIdNamed, Component)>>,
    resources: HashMap<TypeIdNamed, Component>,
    systems: HashMap<TypeIdNamed, SystemInfo>,
    // system_queue: VecDeque<Rc<dyn System>>,
}

impl World {
    pub fn spawn(&mut self) -> EntityHandle<'_> {
        self.entity_id += 1;
        trace!("spawn {}", self.entity_id);
        EntityHandle {
            id: EntityId(self.entity_id),
            world: self,
        }
    }

    fn insert_component<T: Any>(&mut self, entity_id: EntityId, data: T) {
        let id = TypeIdNamed::of::<T>();
        match self.components.entry((id, entity_id)) {
            Entry::Occupied(e) => *e.get().borrow_mut() = data,
            Entry::Vacant(e) => {
                let component = Component::new(data);
                e.insert(component.clone());
                self.component_index
                    .entry(id)
                    .or_default()
                    .push((entity_id, component.clone()));
                // self.entity_index
                //     .entry(entity_id)
                //     .or_default()
                //     .push((id, component));
            }
        }
    }

    pub fn get<T: Any>(&mut self) -> Vec<(EntityId, Ref<'_, T>)> {
        self.component_index
            .get(&TypeIdNamed::of::<T>())
            .into_iter()
            .flatten()
            .map(|(e, c)| (*e, c.borrow()))
            .collect()
    }

    pub fn get_on<T: Any>(&mut self, entity_id: EntityId) -> Option<Ref<'_, T>> {
        self.components
            .get(&(TypeIdNamed::of::<T>(), entity_id))
            .map(|c| c.borrow())
    }

    pub fn insert_resource<T: Any>(&mut self, data: T) {
        self.resources
            .insert(TypeIdNamed::of::<T>(), Component::new(data));
    }

    pub fn get_resource<T: Any>(&mut self) -> Option<Ref<'_, T>> {
        self.resources
            .get(&TypeIdNamed::of::<T>())
            .map(|r| r.borrow())
    }

    pub fn remove_resource<T: Any>(&mut self) {
        self.resources.remove(&TypeIdNamed::of::<T>());
    }

    fn insert_system<S: System>(&mut self, sys: S) -> &mut SystemInfo {
        self.systems.entry(sys.id()).or_insert(SystemInfo::new(sys))
    }

    pub fn insert_system_with<S: System, T: System>(&mut self, with: S, sys: T) {
        self.insert_system(with).with.push(sys.id());
        self.insert_system(sys);
    }

    pub fn insert_system_before<S: System, T: System>(&mut self, before: S, sys: T) {
        self.insert_system(before).depends.push(sys.id());
        self.insert_system(sys);
    }

    pub fn run_system<S: System>(&mut self, sys: S) {
        let info = self
            .systems
            .entry(sys.id())
            .or_insert(SystemInfo::new(sys))
            .clone();
        let mut systems = vec![];
        self.queue_systems(&mut systems, info);
        for s in systems.iter().rev() {
            trace!("run system {}", s.id().name);
            s.run(self);
        }
    }

    fn queue_systems(&mut self, systems: &mut Vec<Rc<dyn System>>, info: SystemInfo) {
        systems.push(info.sys);
        for s in info.with.into_iter().chain(info.depends) {
            self.queue_systems(systems, self.systems.get(&s).unwrap().clone());
        }
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

#[derive(Clone)]
struct Component(Rc<RefCell<dyn Any>>);

impl Component {
    fn new<T: Any>(data: T) -> Self {
        Self(Rc::new(RefCell::new(data)))
    }

    fn borrow<T: Any>(&self) -> Ref<'_, T> {
        Ref::map(self.0.borrow(), |x| x.downcast_ref().unwrap())
    }

    fn borrow_mut<T: Any>(&self) -> RefMut<'_, T> {
        RefMut::map(self.0.borrow_mut(), |x| x.downcast_mut().unwrap())
    }
}

pub trait System: 'static {
    fn run(&self, world: &mut World);
    fn id(&self) -> TypeIdNamed {
        TypeIdNamed::of::<Self>()
    }
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
