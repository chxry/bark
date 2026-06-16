use crate::TypeKey;
use std::any::{self, Any};
use std::collections::{HashMap, HashSet, VecDeque};
use std::iter::Peekable;
use std::marker::PhantomData;
use std::{ops, ptr};
use tracing::{debug, trace};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct EntityId(u64);

pub struct World {
    entity_id: EntityId,
    component_stores: HashMap<TypeKey, Box<dyn Any>>,
    resources: HashMap<TypeKey, Box<dyn Any>>,
    schedules: HashMap<TypeKey, Schedule>,
}

impl World {
    pub fn new() -> Self {
        Self {
            entity_id: EntityId(0),
            component_stores: HashMap::new(),
            resources: HashMap::new(),
            schedules: HashMap::new(),
        }
    }

    pub fn spawn(&mut self) -> EntityId {
        self.entity_id.0 += 1;
        trace!("spawn entity {:?}", self.entity_id);
        self.entity_id
    }

    pub fn get_store<T: Any>(&self) -> Option<&ComponentStore<T>> {
        self.component_stores
            .get(&TypeKey::of::<T>())?
            .downcast_ref()
    }

    pub fn get_store_mut<T: Any>(&mut self) -> Option<&mut ComponentStore<T>> {
        self.component_stores
            .get_mut(&TypeKey::of::<T>())?
            .downcast_mut()
    }

    pub fn create_store<T: Any>(&mut self) -> &mut ComponentStore<T> {
        self.component_stores
            .entry(TypeKey::of::<T>())
            .or_insert_with(|| Box::new(ComponentStore::<T>::new()))
            .downcast_mut()
            .unwrap()
    }

    pub fn insert_resource<T: Any>(&mut self, data: T) {
        let id = TypeKey::of::<T>();
        debug!("insert resource {:?}", id);

        self.resources.insert(id, Box::new(data));
    }

    pub fn get_resource<T: Any>(&self) -> Option<&T> {
        self.resources.get(&TypeKey::of::<T>())?.downcast_ref()
    }

    pub fn get_resource_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.resources.get_mut(&TypeKey::of::<T>())?.downcast_mut()
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
        debug!("run phase {:?}", id);
        if let Some(mut schedule) = self.schedules.remove(&id) {
            schedule.run(self);
            // todo this is kinda weird
            self.schedules.insert(id, schedule);
        }
    }
}

pub struct ComponentStore<T> {
    // must be sorted
    entities: Vec<EntityId>,
    data: Vec<T>,
}

impl<T> ComponentStore<T> {
    fn new() -> Self {
        debug!("new component type {:?}", any::type_name::<T>());
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
            // todo: multithread
            for &i in layer {
                // safety: we trust layers to be setup correctly to ensure safe access
                unsafe {
                    self.systems[i].run(ptr::from_mut(world));
                }
            }
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
                        Ordering::Before => {
                            edges.insert((i, j));
                        }
                        Ordering::After => {
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
        layers
    }
}

pub trait System {
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
        if let Some(Ordering::After) = meta
            .constraints
            .insert(TypeKey::of::<S>(), Ordering::Before)
        {
            panic!("conflicting ordering constraint for {:?}", meta.type_id);
        }
        sys
    }

    fn after<S: Any>(self, _: S) -> Box<dyn System> {
        let mut sys = self.into_system();
        let meta = sys.get_meta_mut();
        if let Some(Ordering::Before) = meta.constraints.insert(TypeKey::of::<S>(), Ordering::After)
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
    constraints: HashMap<TypeKey, Ordering>,
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
enum Ordering {
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
        }
    }

    fn conflicts(&self, other: &Self) -> bool {
        self.component_access.iter().any(|(id, access)| {
            other
                .component_access
                .get(id)
                .is_some_and(|other| access.conflicts(*other))
        }) || self.resource_access.iter().any(|(id, access)| {
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
    type State;

    fn init(meta: &mut SystemMeta) -> Self::State;
    /// safety: must only be called according to `init`
    /// todo: whatever the `SystemParam`s do with these pointers can leak. safety is guaranteed during the system, however users of `SystemParam` and descendents are currently trusted to not do anything silly
    unsafe fn fetch(world: *mut World, state: *mut Self::State) -> Self;
    fn flush(_: &mut World, _: &mut Self::State) {}
}

macro_rules! impl_system {
    ($(($n:tt, $P:ident)),*) => {
        impl<F: Fn($($P),*) + Any, $($P: SystemParam + Any),*> System for SystemStorage<F, ($($P::State,)*), ($($P,)*)> {
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

        impl<F: Fn($($P),*) + Any, $($P: SystemParam + Any),*> IntoSystem<($($P,)*)> for F {
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

impl<T: Any> SystemParam for Res<'_, T> {
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

pub struct ResMut<'w, T>(&'w mut T);

impl<T: Any> SystemParam for ResMut<'_, T> {
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

pub struct Query<P> {
    world: *mut World,
    params: PhantomData<P>,
}

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
            type Item = ($($P,)*);

            #[allow(unused_assignments)]
            fn next(&mut self) -> Option<Self::Item> {
                loop {
                    let mut max = *[$(self.iters.$n.peek()?.0),*].iter().max().unwrap();
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
                        break Some(($(self.iters.$n.next().unwrap().1,)*));
                    }
                }
            }
        }
    };
}

variadics_please::all_tuples_enumerated!(impl_query, 1, 16, P, I);

impl<T: Any> QueryData for &T {
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
        unsafe { (*world).get_store() }.unwrap().iter()
    }
}

impl<T: Any> QueryData for &mut T {
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
        unsafe { (*world).get_store_mut() }.unwrap().iter_mut()
    }
}

pub struct Commands<'w>(&'w mut CommandBuffer);

impl Commands<'_> {
    pub fn spawn(&mut self) -> EntityCommands<'_> {
        self.0.entity_id.0 += 1;
        self.0.commands.push(Box::new(SpawnCommand));
        EntityCommands(self.0, self.0.entity_id)
    }

    pub fn insert_resource<T: Any>(&mut self, data: T) {
        self.0
            .commands
            .push(Box::new(InsertResourceCommand { data }));
    }
}

impl SystemParam for Commands<'_> {
    type State = CommandBuffer;

    fn init(_: &mut SystemMeta) -> Self::State {
        CommandBuffer::new()
    }

    unsafe fn fetch(_: *mut World, state: *mut Self::State) -> Self {
        // safety: 'w is bs, see trait commment
        Self(unsafe { &mut *state })
    }

    fn flush(world: &mut World, state: &mut Self::State) {
        let entity_offset = world.entity_id;
        for cmd in state.commands.drain(..) {
            cmd.apply(world, entity_offset);
        }
        state.entity_id = EntityId(0);
    }
}

pub struct EntityCommands<'w>(&'w mut CommandBuffer, EntityId);

impl EntityCommands<'_> {
    pub fn insert<T: Any>(&mut self, data: T) {
        self.0.commands.push(Box::new(InsertCommand {
            entity_id: self.1,
            data,
        }));
    }
}

struct CommandBuffer {
    entity_id: EntityId,
    commands: Vec<Box<dyn Command>>,
}

impl CommandBuffer {
    fn new() -> Self {
        Self {
            entity_id: EntityId(0),
            commands: vec![],
        }
    }
}

trait Command {
    fn apply(self: Box<Self>, world: &mut World, entity_offset: EntityId);
}

struct SpawnCommand;

impl Command for SpawnCommand {
    fn apply(self: Box<Self>, world: &mut World, _: EntityId) {
        world.spawn();
    }
}

struct InsertCommand<T> {
    entity_id: EntityId,
    // todo: optionally dont offset for existing entities
    data: T,
}

impl<T: Any> Command for InsertCommand<T> {
    fn apply(self: Box<Self>, world: &mut World, entity_offset: EntityId) {
        world
            .create_store()
            .insert(EntityId(self.entity_id.0 + entity_offset.0), self.data);
    }
}

struct InsertResourceCommand<T> {
    data: T,
}

impl<T: Any> Command for InsertResourceCommand<T> {
    fn apply(self: Box<Self>, world: &mut World, _: EntityId) {
        world.insert_resource(self.data)
    }
}
