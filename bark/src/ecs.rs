use crate::TypeKey;
use std::any::{self, Any};
use std::collections::{HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::{ops, ptr, slice};
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

    // todo: maybe consider removing
    pub fn create_store<T: Any>(&mut self) -> &mut ComponentStore<T> {
        self.component_stores
            .entry(TypeKey::of::<T>())
            .or_insert_with(|| Box::new(ComponentStore::<T>::new()))
            .downcast_mut()
            .unwrap()
    }

    pub fn insert_resource<T: Any>(&mut self, res: T) {
        let id = TypeKey::of::<T>();
        trace!("insert resource {:?}", id);

        self.resources.insert(id, Box::new(res));
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

    pub fn insert(&mut self, entity: EntityId, value: T) {
        match self.entities.binary_search(&entity) {
            Ok(i) => {
                self.data[i] = value;
            }
            Err(i) => {
                self.entities.insert(i, entity);
                self.data.insert(i, value);
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
            for id in &meta.after {
                if let Some(&j) = id_to_idx.get(id) {
                    edges.insert((j, i));
                }
            }
            for id in &meta.before {
                if let Some(&j) = id_to_idx.get(id) {
                    edges.insert((i, j));
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
    /// todo: raw pointers
    unsafe fn run(&mut self, world: *mut World);
}

pub trait IntoSystem<P>: Sized {
    fn into_system(self) -> Box<dyn System>;

    fn before<T: Any>(self, _: T) -> Box<dyn System> {
        let mut sys = self.into_system();
        sys.get_meta_mut().before.insert(TypeKey::of::<T>());
        sys
    }

    fn after<T: Any>(self, _: T) -> Box<dyn System> {
        let mut sys = self.into_system();
        sys.get_meta_mut().after.insert(TypeKey::of::<T>());
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
    component_reads: HashSet<TypeKey>,
    component_writes: HashSet<TypeKey>,
    resource_reads: HashSet<TypeKey>,
    resource_writes: HashSet<TypeKey>,
    before: HashSet<TypeKey>,
    after: HashSet<TypeKey>,
}

impl SystemMeta {
    fn new(type_id: TypeKey) -> Self {
        Self {
            type_id,
            component_reads: HashSet::new(),
            component_writes: HashSet::new(),
            resource_reads: HashSet::new(),
            resource_writes: HashSet::new(),
            before: HashSet::new(),
            after: HashSet::new(),
        }
    }

    fn conflicts(&self, other: &Self) -> bool {
        !self.component_writes.is_disjoint(&other.component_reads)
            || !self.component_writes.is_disjoint(&other.component_writes)
            || !self.component_reads.is_disjoint(&other.component_writes)
            || !self.resource_writes.is_disjoint(&other.resource_reads)
            || !self.resource_writes.is_disjoint(&other.resource_writes)
            || !self.resource_reads.is_disjoint(&other.resource_writes)
    }
}

struct SystemStorage<F, P> {
    f: F,
    meta: SystemMeta,
    params: PhantomData<P>,
}

trait SystemParam<'w>: Sized {
    fn declare_access(meta: &mut SystemMeta);
    /// safety: must only be called according to `declare_access`
    unsafe fn fetch(world: *mut World) -> Self;
}

macro_rules! impl_system {
    ($($P:ident),*) => {
        impl<'w, F: Fn($($P),*) + Any, $($P: SystemParam<'w> + Any),*> System for SystemStorage<F, ($($P,)*)> {
            fn get_meta(&self) -> &SystemMeta { &self.meta }
            fn get_meta_mut(&mut self) -> &mut SystemMeta { &mut self.meta }

            #[allow(unused)]
            unsafe fn run(&mut self, world: *mut World) {
                // safety: responsibility is on the caller
                unsafe {
                    $(let $P = $P::fetch(world);)*
                    trace!("run system {:?}", self.meta.type_id.name);
                    (self.f)($($P),*);
                }
            }
        }

        impl<'w, F: Fn($($P),*) + Any, $($P: SystemParam<'w> + Any),*> IntoSystem<($($P,)*)> for F {
            fn into_system(self) -> Box<dyn System> {
                #[allow(unused_mut)]
                let mut meta = SystemMeta::new(TypeKey::of::<F>());
                $($P::declare_access(&mut meta);)*
                Box::new(SystemStorage {
                    f: self,
                    meta,
                    params: PhantomData,
                })
            }
        }
    }
}

variadics_please::all_tuples!(impl_system, 0, 16, P);

pub struct Res<'w, T>(&'w T);

impl<'w, T: Any> SystemParam<'w> for Res<'w, T> {
    fn declare_access(meta: &mut SystemMeta) {
        let id = TypeKey::of::<T>();
        if meta.resource_writes.contains(&id) {
            panic!(
                "conflicting resource access for {:?} in system {:?}",
                id, meta.type_id.name
            );
        }
        meta.resource_reads.insert(id);
    }

    unsafe fn fetch(world: *mut World) -> Self {
        // safety: we requested this in declare_access
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

impl<'w, T: Any> SystemParam<'w> for ResMut<'w, T> {
    fn declare_access(meta: &mut SystemMeta) {
        let id = TypeKey::of::<T>();
        if meta.resource_reads.contains(&id) || meta.resource_writes.contains(&id) {
            panic!(
                "conflicting resource access for {:?} in system {:?}",
                id, meta.type_id.name
            );
        }
        meta.resource_writes.insert(id);
    }

    unsafe fn fetch(world: *mut World) -> Self {
        // safety: we requested this in declare_access
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
    // todo: this is an idea?
    world: *mut World,
    params: PhantomData<P>,
}

pub trait QueryData: Sized {
    type Item;
    fn declare_access(meta: &mut SystemMeta);
    /// safety: must only be used according to `declare_access`
    unsafe fn get_store(world: *mut World) -> *mut ComponentStore<Self::Item>;
    /// safety: `store` must be valid
    unsafe fn get_entity(store: *mut ComponentStore<Self::Item>, entity: EntityId) -> Option<Self>;
}

pub struct QueryIter<'w, S, P> {
    entities: slice::Iter<'w, EntityId>,
    stores: S,
    params: PhantomData<P>,
}

macro_rules! impl_query {
    ($(($n:tt, $P:ident)),*) => {
        impl<'w, $($P: QueryData),*> SystemParam<'w> for Query<($($P,)*)> {
            fn declare_access(meta: &mut SystemMeta)  {
                $($P::declare_access(meta);)*
            }

            unsafe fn fetch(world: *mut World) -> Self {
                Self {
                    world,
                    params: PhantomData
                }
            }
        }

        impl<$($P: QueryData),*> Query<($($P,)*)> {
            pub fn iter(&mut self) -> QueryIter<'_, ($(*mut ComponentStore<$P::Item>,)*), ($($P,)*)> {
                // safety: `declare_access` lets us safely call .entities(), &mut gives `QueryIter` exclusive access
                unsafe {
                    QueryIter {
                        entities: [$((*$P::get_store(self.world)).entities(),)*].iter().min_by_key(|x| x.len()).unwrap().iter(),
                        stores: ($($P::get_store(self.world),)*),
                        params: PhantomData
                    }
                }
            }
        }

        impl<$($P: QueryData),*> Iterator for QueryIter<'_, ($(*mut ComponentStore<$P::Item>,)*), ($($P,)*)> {
            type Item = ($($P,)*);

            fn next(&mut self) -> Option<Self::Item> {
                loop {
                    let id = self.entities.next()?;
                    // safety: `Query` requested this in `declare_access`
                    unsafe {
                        match ($($P::get_entity(self.stores.$n, *id),)*) {
                            ($(Some($P),)*) => return Some(($($P,)*)),
                            _ => continue,
                        }
                    }
                }
            }
        }
    };
}

variadics_please::all_tuples_enumerated!(impl_query, 1, 16, P);

impl<T: Any> QueryData for &T {
    type Item = T;

    fn declare_access(meta: &mut SystemMeta) {
        let id = TypeKey::of::<T>();
        if meta.component_writes.contains(&id) {
            panic!(
                "conflicting component access for {:?} in system {:?}",
                id, meta.type_id.name
            );
        }
        meta.component_reads.insert(id);
    }

    unsafe fn get_store(world: *mut World) -> *mut ComponentStore<T> {
        // safety: we requested access to the store in `declare_access`, caller is responsible for mut
        unsafe { &mut *world }.get_store_mut().unwrap()
    }

    unsafe fn get_entity(store: *mut ComponentStore<T>, entity: EntityId) -> Option<Self> {
        // safety: we trust `store`
        unsafe { &*store }.get(entity)
    }
}

impl<T: Any> QueryData for &mut T {
    type Item = T;

    fn declare_access(meta: &mut SystemMeta) {
        let id = TypeKey::of::<T>();
        if meta.component_reads.contains(&id) || meta.component_writes.contains(&id) {
            panic!(
                "conflicting component access for {:?} in system {:?}",
                id, meta.type_id.name
            );
        }
        meta.component_writes.insert(id);
    }

    unsafe fn get_store(world: *mut World) -> *mut ComponentStore<T> {
        // safety: we requested access to the store in `declare_access`, caller is responsible for mut
        unsafe { &mut *world }.get_store_mut().unwrap()
    }

    unsafe fn get_entity(store: *mut ComponentStore<T>, entity: EntityId) -> Option<Self> {
        // safety: we trust `store`
        unsafe { &mut *store }.get_mut(entity)
    }
}
