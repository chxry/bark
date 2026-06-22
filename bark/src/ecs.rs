// todo: use of raw pointers is evil, atleast consider nonnull or unsafecell. casting raw pointers to usize to avoid !Send + !Sync restrictions is also scary
use crate::TypeKey;
use std::any::{self, Any};
use std::collections::{HashMap, HashSet, VecDeque};
use std::iter::Peekable;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{mem, ops, ptr, slice};
use tracing::trace;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct EntityId(u64);

pub struct World {
    entity_id: AtomicU64,
    component_stores: HashMap<TypeKey, Box<dyn ErasedComponentStore>>,
    event_stores: HashMap<TypeKey, Box<dyn ErasedEventStore>>,
    resources: HashMap<TypeKey, Box<dyn Any + Send + Sync>>,
    schedules: HashMap<TypeKey, Schedule>,
}

// todo: refactor all .downcast_xxx() calls to panic with typeid info instead of silenting returning none
impl World {
    pub fn new() -> Self {
        Self {
            entity_id: AtomicU64::new(0),
            component_stores: HashMap::new(),
            event_stores: HashMap::new(),
            resources: HashMap::new(),
            schedules: HashMap::new(),
        }
    }

    pub fn get_component_store<T: Any + Send + Sync>(&self) -> Option<&ComponentStore<T>> {
        (self.component_stores.get(&TypeKey::of::<T>())?.as_ref() as &dyn Any).downcast_ref()
    }

    pub fn get_component_store_mut<T: Any + Send + Sync>(
        &mut self,
    ) -> Option<&mut ComponentStore<T>> {
        (self.component_stores.get_mut(&TypeKey::of::<T>())?.as_mut() as &mut dyn Any)
            .downcast_mut()
    }

    pub fn create_component_store<T: Any + Send + Sync>(&mut self) -> &mut ComponentStore<T> {
        (self
            .component_stores
            .entry(TypeKey::of::<T>())
            .or_insert_with(|| Box::new(ComponentStore::<T>::new()))
            .as_mut() as &mut dyn Any)
            .downcast_mut()
            .unwrap()
    }

    pub fn get_event_store<T: Any + Send + Sync>(&self) -> Option<&EventStore<T>> {
        (self.event_stores.get(&TypeKey::of::<T>())?.as_ref() as &dyn Any).downcast_ref()
    }

    pub fn get_event_store_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut EventStore<T>> {
        (self.event_stores.get_mut(&TypeKey::of::<T>())?.as_mut() as &mut dyn Any).downcast_mut()
    }

    pub fn create_event_store<T: Any + Send + Sync>(&mut self) -> &mut EventStore<T> {
        (self
            .event_stores
            .entry(TypeKey::of::<T>())
            .or_insert_with(|| Box::new(EventStore::<T>::new()))
            .as_mut() as &mut dyn Any)
            .downcast_mut()
            .unwrap()
    }

    pub fn insert_resource<T: Any + Send + Sync>(&mut self, data: T) {
        let id = TypeKey::of::<T>();
        trace!("insert resource {:?}", id);

        self.resources.insert(id, Box::new(data));
    }

    pub fn get_resource<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.resources.get(&TypeKey::of::<T>())?.downcast_ref()
    }

    pub fn get_resource_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut T> {
        self.resources.get_mut(&TypeKey::of::<T>())?.downcast_mut()
    }

    pub fn take_resource<T: Any + Send + Sync>(&mut self) -> Option<T> {
        self.resources
            .remove(&TypeKey::of::<T>())
            .map(|x| *x.downcast().unwrap())
    }

    pub fn insert_system<T: Any, S: IntoSystem<P>, P>(&mut self, _: T, sys: S) {
        let id = TypeKey::of::<T>();
        self.schedules
            .entry(id)
            .or_insert_with(Schedule::new)
            .add(sys.into_system());
    }

    pub fn run_schedule<T: Any>(&mut self, _: T) {
        let id = TypeKey::of::<T>();
        trace!("run phase {:?}", id);
        if let Some(mut schedule) = self.schedules.remove(&id) {
            schedule.run(self);
            // todo this is kinda weird
            self.schedules.insert(id, schedule);
        }
    }

    pub fn spawn(&self) -> EntityId {
        let id = self.entity_id.fetch_add(1, Ordering::Relaxed);
        EntityId(id)
    }

    pub fn despawn(&mut self, entity: EntityId) {
        for store in self.component_stores.values_mut() {
            store.despawn(entity);
        }
    }

    pub fn insert_component<T: Any + Send + Sync>(&mut self, entity: EntityId, data: T) {
        self.create_component_store().insert(entity, data);
    }

    pub fn remove_component<T: Any + Send + Sync>(&mut self, entity: EntityId) -> Option<T> {
        self.get_component_store_mut()?.remove(entity)
    }

    pub fn clear_events(&mut self) {
        for store in self.event_stores.values_mut() {
            store.swap_buffers();
        }
    }

    pub fn queue_event<T: Any + Send + Sync>(&mut self, event: T) {
        self.create_event_store::<T>().queue(event);
    }
}

trait ErasedComponentStore: Any + Send + Sync {
    fn despawn(&mut self, entity: EntityId);
}

pub struct ComponentStore<T> {
    // must be sorted
    entities: Vec<EntityId>,
    data: Vec<T>,
}

impl<T> ComponentStore<T> {
    fn new() -> Self {
        trace!("new component type {:?}", any::type_name::<T>());
        Self {
            entities: vec![],
            data: vec![],
        }
    }

    pub fn insert(&mut self, entity: EntityId, data: T) {
        match self.entities.binary_search(&entity) {
            Ok(i) => {
                self.data[i] = data;
            }
            Err(i) => {
                self.entities.insert(i, entity);
                self.data.insert(i, data);
            }
        }
    }

    pub fn remove(&mut self, entity: EntityId) -> Option<T> {
        let i = self.entities.binary_search(&entity).ok()?;
        self.entities.remove(i);
        Some(self.data.remove(i))
    }

    pub fn get(&self, entity: EntityId) -> Option<&T> {
        let i = self.entities.binary_search(&entity).ok()?;
        Some(&self.data[i])
    }

    pub fn get_mut(&mut self, entity: EntityId) -> Option<&mut T> {
        let i = self.entities.binary_search(&entity).ok()?;
        Some(&mut self.data[i])
    }

    pub fn contains(&self, entity: EntityId) -> bool {
        self.entities.binary_search(&entity).is_ok()
    }

    pub fn entities(&self) -> &[EntityId] {
        &self.entities
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &T)> {
        self.entities.iter().copied().zip(self.data.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (EntityId, &mut T)> {
        self.entities.iter().copied().zip(self.data.iter_mut())
    }
}

impl<T: Any + Send + Sync> ErasedComponentStore for ComponentStore<T> {
    fn despawn(&mut self, entity: EntityId) {
        self.remove(entity);
    }
}

trait ErasedEventStore: Any + Send + Sync {
    fn swap_buffers(&mut self);
}

pub struct EventStore<T> {
    current: Vec<T>,
    queued: Vec<T>,
}

impl<T> EventStore<T> {
    fn new() -> Self {
        trace!("new event type {:?}", any::type_name::<T>());
        Self {
            current: vec![],
            queued: vec![],
        }
    }

    fn queue(&mut self, event: T) {
        self.queued.push(event);
    }

    fn current(&self) -> &[T] {
        &self.current
    }
}

impl<T: Any + Send + Sync> ErasedEventStore for EventStore<T> {
    fn swap_buffers(&mut self) {
        self.current.clear();
        mem::swap(&mut self.current, &mut self.queued);
    }
}

struct Schedule {
    systems: Vec<Box<dyn System>>,
    layers: Option<Vec<Vec<usize>>>,
}

impl Schedule {
    fn new() -> Self {
        Self {
            systems: vec![],
            layers: None,
        }
    }

    pub fn add(&mut self, system: Box<dyn System>) {
        self.systems.push(system);
        self.layers = None;
    }

    pub fn run(&mut self, world: &mut World) {
        if self.layers.is_none() {
            self.layers = Some(self.build_layers());
        }

        for layer in self.layers.as_ref().unwrap() {
            // safety: we trust layers to be setup correctly to ensure safe access
            unsafe {
                if layer.len() == 1 {
                    self.systems[layer[0]].run(ptr::from_mut(world));
                } else {
                    rayon::scope(|s| {
                        let world_ptr = ptr::from_mut(world) as usize;
                        for &i in layer {
                            let sys = &raw mut self.systems[i] as usize;
                            s.spawn(move |_| (*(sys as *mut Box<dyn System>)).run(world_ptr as _));
                        }
                    });
                }
            }

            // todo: batch world operations, insertions and removals are costly
            for &i in layer {
                self.systems[i].flush(world);
            }
        }
    }

    fn build_layers(&self) -> Vec<Vec<usize>> {
        let n = self.systems.len();

        let id_to_idx: HashMap<_, _> = self
            .systems
            .iter()
            .enumerate()
            .map(|(i, s)| (s.get_meta().type_id, i))
            .collect();

        let mut edges = HashSet::new();

        for i in 0..n {
            let meta = self.systems[i].get_meta();
            for (id, ordering) in &meta.constraints {
                if let Some(&j) = id_to_idx.get(id) {
                    match ordering {
                        SystemOrder::Before => {
                            edges.insert((i, j));
                        }
                        SystemOrder::After => {
                            edges.insert((j, i));
                        }
                    }
                }
            }
        }

        for i in 0..n {
            for j in (i + 1)..n {
                if self.systems[i]
                    .get_meta()
                    .conflicts(self.systems[j].get_meta())
                    && !edges.contains(&(i, j))
                    && !edges.contains(&(j, i))
                {
                    edges.insert((i, j));
                }
            }
        }

        let mut successors = vec![vec![]; n];
        let mut in_degree = vec![0; n];
        for (a, b) in &edges {
            successors[*a].push(*b);
            in_degree[*b] += 1;
        }
        // todo: consider why we split the dag into layers. a single long running system can unnecessarily block an entire layer

        let mut layer_of = vec![0; n];
        let mut queue: VecDeque<_> = (0..n).filter(|i| in_degree[*i] == 0).collect();

        let mut processed = 0;
        while let Some(i) = queue.pop_front() {
            processed += 1;
            for &j in &successors[i] {
                layer_of[j] = layer_of[j].max(layer_of[i] + 1);
                in_degree[j] -= 1;
                if in_degree[j] == 0 {
                    queue.push_back(j);
                }
            }
        }

        assert_eq!(processed, n, "cycle detected in system ordering");

        let max = *layer_of.iter().max().unwrap_or(&0);
        let mut layers = vec![vec![]; max + 1];
        for (i, l) in layer_of.iter().enumerate() {
            layers[*l].push(i);
        }

        // let mut dot = String::new();
        // dot.push_str("digraph schedule {\n");
        // for (n, l) in layers.iter().enumerate() {
        //     dot.push_str(&format!(
        //         "subgraph cluster_{} {{\nlabel=\"layer {0}\";\nlabeljust=l;\n",
        //         n
        //     ));
        //     for &i in l.iter() {
        //         dot.push_str(&format!(
        //             "n{i} [label=\"{}\"];\n",
        //             self.systems[i].get_meta().type_id.name
        //         ));
        //     }
        //     dot.push_str("}\n");
        // }
        // for &(a, b) in &edges {
        //     dot.push_str(&format!("n{a} -> n{b};\n"));
        // }
        // dot.push_str("}");
        // println!("{}", dot);

        layers
    }
}

pub trait System: Send + Sync {
    fn get_meta(&self) -> &SystemMeta;
    fn get_meta_mut(&mut self) -> &mut SystemMeta;
    /// safety: must only be called according to `SystemMeta`
    unsafe fn run(&mut self, world: *mut World);
    fn flush(&mut self, world: &mut World);
}

pub trait IntoSystem<P>: Sized {
    fn into_system(self) -> Box<dyn System>;

    fn before<S: Any>(self, _: S) -> Box<dyn System> {
        let mut sys = self.into_system();
        let meta = sys.get_meta_mut();
        if let Some(SystemOrder::After) = meta
            .constraints
            .insert(TypeKey::of::<S>(), SystemOrder::Before)
        {
            panic!("conflicting ordering constraint for {:?}", meta.type_id);
        }
        sys
    }

    fn after<S: Any>(self, _: S) -> Box<dyn System> {
        let mut sys = self.into_system();
        let meta = sys.get_meta_mut();
        if let Some(SystemOrder::Before) = meta
            .constraints
            .insert(TypeKey::of::<S>(), SystemOrder::After)
        {
            panic!("conflicting ordering constraint for {:?}", meta.type_id);
        }
        sys
    }
}

impl IntoSystem<()> for Box<dyn System> {
    fn into_system(self) -> Box<dyn System> {
        self
    }
}

// enum inside the sets could shrink memory usage
pub struct SystemMeta {
    type_id: TypeKey,
    component_access: HashMap<TypeKey, Access>,
    resource_access: HashMap<TypeKey, Access>,
    constraints: HashMap<TypeKey, SystemOrder>,
    exclusive: bool,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Access {
    Read,
    Write,
}

impl Access {
    fn conflicts(self, other: Self) -> bool {
        self != Access::Read || other != Access::Read
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum SystemOrder {
    Before,
    After,
}

impl SystemMeta {
    fn new(type_id: TypeKey) -> Self {
        Self {
            type_id,
            component_access: HashMap::new(),
            resource_access: HashMap::new(),
            constraints: HashMap::new(),
            exclusive: false,
        }
    }

    fn conflicts(&self, other: &Self) -> bool {
        self.exclusive
            || other.exclusive
            || self.component_access.iter().any(|(id, access)| {
                other
                    .component_access
                    .get(id)
                    .is_some_and(|other| access.conflicts(*other))
            })
            || self.resource_access.iter().any(|(id, access)| {
                other
                    .resource_access
                    .get(id)
                    .is_some_and(|other| access.conflicts(*other))
            })
    }
}

struct SystemStorage<F, S, P> {
    f: F,
    meta: SystemMeta,
    state: S,
    params: PhantomData<P>,
}

trait SystemParam: Sized {
    type State: Send + Sync;

    fn init(meta: &mut SystemMeta) -> Self::State;
    /// safety: must only be called according to `init`
    /// todo: whatever the `SystemParam`s do with these pointers can leak. safety is guaranteed during the system, however users of `SystemParam` and descendents are currently trusted to not do anything silly
    unsafe fn fetch(world: *mut World, state: *mut Self::State) -> Self;
    fn flush(_: &mut World, _: &mut Self::State) {}
}

macro_rules! impl_system {
    ($(($n:tt, $P:ident)),*) => {
        impl<F: Fn($($P),*) + Any + Send + Sync, $($P: SystemParam + Any + Send + Sync),*> System for SystemStorage<F, ($($P::State,)*), ($($P,)*)> {
            fn get_meta(&self) -> &SystemMeta { &self.meta }
            fn get_meta_mut(&mut self) -> &mut SystemMeta { &mut self.meta }

            #[allow(unused)]
            unsafe fn run(&mut self, world: *mut World) {
                // safety: responsibility is on the caller
                unsafe {
                    $(
                        let $P = $P::fetch(world, ptr::from_mut(&mut self.state.$n));
                    )*
                    trace!("run system {:?}", self.meta.type_id.name);
                    (self.f)($($P),*);
                }
            }

            #[allow(unused)]
            fn flush(&mut self, world: &mut World) {
                $(
                    let $P = $P::flush(world, &mut self.state.$n);
                )*
            }
        }

        impl<F: Fn($($P),*) + Any + Send + Sync, $($P: SystemParam + Any + Send + Sync),*> IntoSystem<($($P,)*)> for F {
            fn into_system(self) -> Box<dyn System> {
                #[allow(unused_mut)]
                let mut meta = SystemMeta::new(TypeKey::of::<F>());
                let state = ($($P::init(&mut meta),)*);
                Box::new(SystemStorage {
                    f: self,
                    meta,
                    state,
                    params: PhantomData
                })
            }
        }
    }
}

variadics_please::all_tuples_enumerated!(impl_system, 0, 16, P);

pub struct Res<'w, T>(&'w T);

impl<T: Any + Send + Sync> SystemParam for Res<'_, T> {
    type State = ();

    fn init(meta: &mut SystemMeta) {
        let id = TypeKey::of::<T>();
        if let Some(Access::Write) = meta.resource_access.get(&id) {
            panic!(
                "conflicting resource access for {:?} in system {:?}",
                id, meta.type_id.name
            );
        }
        meta.resource_access.insert(id, Access::Read);
    }

    unsafe fn fetch(world: *mut World, _: *mut Self::State) -> Self {
        // safety: we requested this in `init`
        Self(unsafe { (*world).get_resource() }.unwrap())
    }
}

impl<T> ops::Deref for Res<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<T> AsRef<T> for Res<'_, T> {
    fn as_ref(&self) -> &T {
        self.0
    }
}

pub struct ResMut<'w, T>(&'w mut T);

impl<T: Any + Send + Sync> SystemParam for ResMut<'_, T> {
    type State = ();

    fn init(meta: &mut SystemMeta) {
        let id = TypeKey::of::<T>();
        if meta.resource_access.contains_key(&id) {
            panic!(
                "conflicting resource access for {:?} in system {:?}",
                id, meta.type_id.name
            );
        }
        meta.resource_access.insert(id, Access::Write);
    }

    unsafe fn fetch(world: *mut World, _: *mut Self::State) -> Self {
        // safety: we requested this in `init`
        Self(unsafe { (*world).get_resource_mut() }.unwrap())
    }
}

impl<T> ops::Deref for ResMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<T> ops::DerefMut for ResMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

impl<T> AsMut<T> for ResMut<'_, T> {
    fn as_mut(&mut self) -> &mut T {
        self.0
    }
}

pub struct MainThread;

impl SystemParam for MainThread {
    type State = ();

    fn init(meta: &mut SystemMeta) {
        meta.exclusive = true;
    }

    unsafe fn fetch(_: *mut World, _: *mut Self::State) -> Self {
        Self
    }
}

pub struct Query<P> {
    world: *mut World,
    params: PhantomData<P>,
}

// safety: ill do what i want with raw pointers
unsafe impl<P> Send for Query<P> {}
unsafe impl<P> Sync for Query<P> {}

pub trait QueryData: Sized {
    fn declare_access(meta: &mut SystemMeta);
    /// safety: must only be used according to `declare_access`
    /// todo: we hand out this iterator with no lifetimes linking it to the World! probably scary
    unsafe fn get_iter(world: *mut World) -> impl Iterator<Item = (EntityId, Self)>;
}

pub struct QueryIter<S, P> {
    iters: S,
    params: PhantomData<P>,
}

macro_rules! impl_query {
    ($(($n:tt, $P:ident, $I:ident)),*) => {
        impl<$($P: QueryData),*> SystemParam for Query<($($P,)*)> {
            type State = ();

            fn init(meta: &mut SystemMeta)  {
                $(
                    $P::declare_access(meta);
                )*
            }

            unsafe fn fetch(world: *mut World,  _: *mut Self::State) -> Self {
                Self {
                    world,
                    params: PhantomData
                }
            }
        }

        impl<$($P: QueryData),*> Query<($($P,)*)> {
            pub fn iter(&mut self) -> QueryIter<($(Peekable<impl Iterator<Item = (EntityId, $P)>>,)*), ($($P,)*)> {
                // safety: &mut gives `QueryIter` exclusive access over what we requested in `declare_access`
                unsafe {
                    QueryIter {
                        iters: ($($P::get_iter(self.world).peekable(),)*),
                        params: PhantomData
                    }
                }
            }
        }

        impl<$($I: Iterator<Item = (EntityId, $P)>),*, $($P: QueryData),*> Iterator for QueryIter<($(Peekable<$I>,)*), ($($P,)*)> {
            type Item = (EntityId, ($($P,)*));

            #[allow(unused_assignments)]
            fn next(&mut self) -> Option<Self::Item> {
                let mut max = *[$(self.iters.$n.peek()?.0),*].iter().max().unwrap();
                loop {
                    let mut matched = true;
                    $(
                        // todo: advance using `partiton_point`?
                        loop {
                            match self.iters.$n.peek()? {
                                &(e, _) if e >= max => {
                                    max = e;
                                    break;
                                },
                                _ => {
                                    self.iters.$n.next();
                                    matched = false;
                                }
                            }
                        }
                    )*
                    if matched {
                        break Some((max, ($(self.iters.$n.next().unwrap().1,)*)));
                    }
                }
            }
        }
    };
}

variadics_please::all_tuples_enumerated!(impl_query, 1, 16, P, I);

impl<T: Any + Send + Sync> QueryData for &T {
    fn declare_access(meta: &mut SystemMeta) {
        let id = TypeKey::of::<T>();
        if let Some(Access::Write) = meta.component_access.get(&id) {
            panic!(
                "conflicting component access for {:?} in system {:?}",
                id, meta.type_id.name
            );
        }
        meta.component_access.insert(id, Access::Read);
    }

    unsafe fn get_iter(world: *mut World) -> impl Iterator<Item = (EntityId, Self)> {
        // safety: we requested this in `declare_access`
        unsafe { (*world).get_component_store() }.unwrap().iter()
    }
}

impl<T: Any + Send + Sync> QueryData for &mut T {
    fn declare_access(meta: &mut SystemMeta) {
        let id = TypeKey::of::<T>();
        if meta.component_access.contains_key(&id) {
            panic!(
                "conflicting component access for {:?} in system {:?}",
                id, meta.type_id.name
            );
        }
        meta.component_access.insert(id, Access::Write);
    }

    unsafe fn get_iter(world: *mut World) -> impl Iterator<Item = (EntityId, Self)> {
        // safety: we requested this in `declare_access`
        unsafe { (*world).get_component_store_mut() }
            .unwrap()
            .iter_mut()
    }
}

type CommandBuffer = Vec<Box<dyn Command>>;

pub struct Commands<'w>(&'w World, &'w mut CommandBuffer);

impl Commands<'_> {
    pub fn spawn(&mut self) -> EntityCommands<'_> {
        EntityCommands(self.1, self.0.spawn())
    }

    pub fn entity(&mut self, entity: EntityId) -> EntityCommands<'_> {
        EntityCommands(self.1, entity)
    }

    pub fn insert_resource<T: Any + Send + Sync>(&mut self, data: T) {
        self.1.push(Box::new(InsertResourceCommand(data)));
    }

    pub fn queue_event<T: Any + Send + Sync>(&mut self, event: T) {
        self.1.push(Box::new(QueueEventCommand(event)));
    }
}

impl SystemParam for Commands<'_> {
    type State = CommandBuffer;

    fn init(_: &mut SystemMeta) -> Self::State {
        CommandBuffer::new()
    }

    unsafe fn fetch(world: *mut World, state: *mut Self::State) -> Self {
        // safety: 'w is bs, see trait commment. we only use `World.entity_id`
        unsafe { Self(&*world, &mut *state) }
    }

    fn flush(world: &mut World, state: &mut Self::State) {
        for cmd in state.drain(..) {
            cmd.apply(world);
        }
    }
}

pub struct EntityCommands<'w>(&'w mut CommandBuffer, EntityId);

impl EntityCommands<'_> {
    pub fn insert<T: Any + Send + Sync>(&mut self, data: T) {
        self.0.push(Box::new(InsertComponentCommand(data, self.1)));
    }

    pub fn remove<T: Any + Send + Sync>(&mut self) {
        self.0
            .push(Box::new(RemoveComponentCommand(PhantomData::<T>, self.1)));
    }

    pub fn despawn(self) {
        self.0.push(Box::new(DespawnCommand(self.1)));
    }
}

trait Command: Send + Sync {
    fn apply(self: Box<Self>, world: &mut World);
}

struct InsertComponentCommand<T>(T, EntityId);

impl<T: Any + Send + Sync> Command for InsertComponentCommand<T> {
    fn apply(self: Box<Self>, world: &mut World) {
        world.insert_component(self.1, self.0);
    }
}

struct RemoveComponentCommand<T>(PhantomData<T>, EntityId);

impl<T: Any + Send + Sync> Command for RemoveComponentCommand<T> {
    fn apply(self: Box<Self>, world: &mut World) {
        world.remove_component::<T>(self.1);
    }
}

struct DespawnCommand(EntityId);

impl Command for DespawnCommand {
    fn apply(self: Box<Self>, world: &mut World) {
        world.despawn(self.0)
    }
}

struct InsertResourceCommand<T>(T);

impl<T: Any + Send + Sync> Command for InsertResourceCommand<T> {
    fn apply(self: Box<Self>, world: &mut World) {
        world.insert_resource(self.0)
    }
}

struct QueueEventCommand<T>(T);

impl<T: Any + Send + Sync> Command for QueueEventCommand<T> {
    fn apply(self: Box<Self>, world: &mut World) {
        world.queue_event(self.0)
    }
}

pub struct Events<'w, T>(Option<&'w [T]>);

impl<'w, T> Events<'w, T> {
    pub fn iter(&self) -> slice::Iter<'w, T> {
        self.0.unwrap_or(&[]).iter()
    }
}

impl<T: Any + Send + Sync> SystemParam for Events<'_, T> {
    type State = ();

    fn init(_: &mut SystemMeta) {}

    unsafe fn fetch(world: *mut World, _: *mut Self::State) -> Self {
        // safety: event stores are safe to access
        Self(unsafe { (*world).get_event_store() }.map(|x| x.current()))
    }
}
