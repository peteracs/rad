

impl Value {
    pub const NIL: Value = Value(QNAN | TAG_NIL);
    pub const TRUE: Value = Value(QNAN | TAG_TRUE);
    pub const FALSE: Value = Value(QNAN | TAG_FALSE);

    #[inline(always)]
    pub fn from_float(f: f64) -> Self {
        if f.is_nan() {
            Value(CANONICAL_FLOAT_NAN)
        } else {
            Value(f.to_bits())
        }
    }

    #[inline(always)]
    pub fn from_bool(b: bool) -> Self {
        if b {
            Self::TRUE
        } else {
            Self::FALSE
        }
    }

    /// Inline integer (no heap). For out-of-range values use [`from_int`](Self::from_int).
    #[inline(always)]
    pub fn int(n: i64) -> Self {
        debug_assert!(
            (INLINE_INT_MIN..=INLINE_INT_MAX).contains(&n),
            "Value::int: use from_int(&mut gc, n) for bigint"
        );
        if (INLINE_INT_MIN..=INLINE_INT_MAX).contains(&n) {
            let payload = (n as u64) & INT_PAYLOAD_MASK;
            Value(SIGN_BIT | QNAN | INT_TAG_BIT | payload)
        } else {
            panic!(
                "Value::int: {} out of inline range; use Value::from_int(&mut gc, n)",
                n
            );
        }
    }

    /// Integer value: inline when `n` fits the fast range (same as [`Self::int`]), otherwise a
    /// heap [`Object::BigInt`]. Prefer [`Self::int`] in tests and hot paths when `n` is known to
    /// be in range; use this when `n` may be arbitrary.
    #[inline(always)]
    pub(crate) fn from_int(alloc: &mut dyn Allocator, n: i64) -> Self {
        if (INLINE_INT_MIN..=INLINE_INT_MAX).contains(&n) {
            Self::int(n)
        } else {
            Self::from_object(alloc, Object::BigInt(n))
        }
    }

    pub(crate) fn from_string(alloc: &mut dyn Allocator, s: String) -> Self {
        Self::from_object(alloc, Object::Str(Arc::from(s)))
    }

    pub(crate) fn from_object(alloc: &mut dyn Allocator, obj: Object) -> Self {
        let ptr = alloc.alloc_object(obj) as u64;
        let tag = alloc.pointer_tag();
        debug_assert!(
            tag == 0 || tag == PERSISTENT_PTR_TAG,
            "unexpected object pointer tag: {tag:#x}"
        );
        debug_assert!(ptr & !PTR_MASK == 0, "pointer exceeds 48 bits");
        debug_assert!(ptr & tag == 0, "pointer collides with allocator tag bits");
        Value(SIGN_BIT | QNAN | ((ptr & PTR_MASK) | tag))
    }

    #[inline(always)]
    fn is_inline_int(&self) -> bool {
        self.tag() == ValueTag::Int
    }

    #[inline(always)]
    fn is_object(&self) -> bool {
        self.tag() == ValueTag::Object
    }

    /// Classify a value exclusively from its bits.  This function never
    /// dereferences an object payload and is therefore safe to use at raw
    /// bytecode and wire ingress boundaries.
    #[inline(always)]
    pub(crate) fn tag(&self) -> ValueTag {
        if self.0 == Self::NIL.0 {
            ValueTag::Nil
        } else if self.0 == Self::TRUE.0 || self.0 == Self::FALSE.0 {
            ValueTag::Bool
        } else if (self.0 & (QNAN | SIGN_BIT | INT_TAG_BIT)) == (QNAN | SIGN_BIT | INT_TAG_BIT) {
            ValueTag::Int
        } else if (self.0 & (QNAN | SIGN_BIT)) == (QNAN | SIGN_BIT) {
            ValueTag::Object
        } else {
            ValueTag::Float
        }
    }

    /// Pure tag test for host-supplied constants.  Unlike `as_object`, this
    /// cannot touch a forged or foreign pointer.
    #[inline(always)]
    pub(crate) fn is_heap_object_tag(&self) -> bool {
        self.tag() == ValueTag::Object
    }

    #[inline(always)]
    fn is_persistent_object(&self) -> bool {
        self.is_object() && (self.0 & PERSISTENT_PTR_TAG) != 0
    }

    #[inline(always)]
    fn object_data_ptr(&self) -> *mut Object {
        ((self.0 & PTR_MASK) & !PERSISTENT_PTR_TAG) as *mut Object
    }

    /// Stable identity for one live heap object, including persistent-store
    /// objects. Used only while the owning heap/store is known to remain live.
    #[inline(always)]
    pub(crate) fn object_identity(&self) -> Option<usize> {
        self.is_object().then(|| self.object_data_ptr() as usize)
    }

    /// Increment refcount for persistent `Arc<Object>`-backed values.
    ///
    /// # Safety
    /// Only call for values created by `PersistentStore`.
    #[inline(always)]
    pub(crate) unsafe fn retain_persistent(&self) {
        if self.is_persistent_object() {
            persist_audit::on_retain(self.object_data_ptr() as usize);
            std::sync::Arc::increment_strong_count(self.object_data_ptr() as *const Object);
        }
    }

    /// Decrement refcount for persistent `Arc<Object>`-backed values.
    ///
    /// # Safety
    /// Only call for values created by `PersistentStore`.
    #[inline(always)]
    pub(crate) unsafe fn release_persistent(&self) {
        if self.is_persistent_object() && persist_audit::on_release(self.object_data_ptr() as usize)
        {
            std::sync::Arc::decrement_strong_count(self.object_data_ptr() as *const Object);
        }
    }

    #[inline(always)]
    pub fn is_nil(&self) -> bool {
        self.0 == Self::NIL.0
    }

    #[inline(always)]
    pub fn is_bool(&self) -> bool {
        self.0 == Self::TRUE.0 || self.0 == Self::FALSE.0
    }

    #[inline(always)]
    pub fn is_float(&self) -> bool {
        self.tag() == ValueTag::Float
    }

    #[inline(always)]
    pub fn as_float(&self) -> Option<f64> {
        if self.is_float() {
            Some(f64::from_bits(self.0))
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn as_bool(&self) -> Option<bool> {
        if self.0 == Self::TRUE.0 {
            Some(true)
        } else if self.0 == Self::FALSE.0 {
            Some(false)
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn as_object(&self) -> Option<&Object> {
        if self.is_object() {
            if self.is_persistent_object() {
                persist_audit::on_read(self.object_data_ptr() as usize);
            }
            let ptr = self.object_data_ptr() as *const Object;
            Some(unsafe { &*ptr })
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn as_object_mut(&mut self) -> Option<&mut Object> {
        if self.is_object() {
            let ptr = self.object_data_ptr();
            Some(unsafe { &mut *ptr })
        } else {
            None
        }
    }

    /// Return the raw pointer to the heap Object (for GC tracing).
    #[inline(always)]
    pub(crate) fn object_ptr(&self) -> Option<usize> {
        if self.is_object() && !self.is_persistent_object() {
            Some(self.object_data_ptr() as usize)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn as_int(&self) -> Option<i64> {
        if self.is_inline_int() {
            let raw = self.0 & INT_PAYLOAD_MASK;
            let signed = if raw & (1u64 << 46) != 0 {
                raw | !INT_PAYLOAD_MASK
            } else {
                raw
            };
            Some(signed as i64)
        } else {
            self.as_object().and_then(|o| match o {
                Object::BigInt(n) => Some(*n),
                _ => None,
            })
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        self.as_object().and_then(|o| match o {
            Object::Str(s) => Some(&**s),
            _ => None,
        })
    }

    pub fn as_list(&self) -> Option<&RadList> {
        self.as_object().and_then(|o| match o {
            Object::List(items) => Some(items),
            _ => None,
        })
    }

    pub fn as_component(&self) -> Option<&ComponentData> {
        self.as_object().and_then(|o| match o {
            Object::Component(c) => Some(c),
            _ => None,
        })
    }

    pub fn as_state(&self) -> Option<&StateInst> {
        self.as_object().and_then(|o| match o {
            Object::State(s) => Some(s),
            _ => None,
        })
    }

    pub fn as_sum_type(&self) -> Option<&SumTypeInst> {
        self.as_object().and_then(|o| match o {
            Object::SumType(st) => Some(st),
            _ => None,
        })
    }

    pub fn as_fn(&self) -> Option<&FnValue> {
        self.as_object().and_then(|o| match o {
            Object::Fn(f) => Some(f),
            _ => None,
        })
    }

    pub fn as_closure(&self) -> Option<&ClosureValue> {
        self.as_object().and_then(|o| match o {
            Object::Closure(c) => Some(c),
            _ => None,
        })
    }

    pub fn as_cell(&self) -> Option<*mut gc::CaptureCell> {
        self.as_object().and_then(|o| match o {
            Object::Cell(c) => Some(*c),
            _ => None,
        })
    }

    pub fn as_builtin(&self) -> Option<Builtin> {
        self.as_object().and_then(|o| match o {
            Object::BuiltinFn(b) => Some(*b),
            _ => None,
        })
    }

    pub fn as_native_fn(&self) -> Option<&NativeFnInfo> {
        self.as_object().and_then(|o| match o {
            Object::NativeFn(n) => Some(n),
            _ => None,
        })
    }

    pub fn as_entity_id(&self) -> Option<u32> {
        self.as_object().and_then(|o| match o {
            Object::EntityId(id) => Some(*id),
            _ => None,
        })
    }

    pub fn as_task(&self) -> Option<u64> {
        self.as_object().and_then(|o| match o {
            Object::Task(id) => Some(*id),
            _ => None,
        })
    }

    pub fn as_map(&self) -> Option<&MapStorage> {
        self.as_object().and_then(|o| match o {
            Object::Map(m) => Some(m),
            _ => None,
        })
    }

    pub fn as_bitset(&self) -> Option<&Vec<u64>> {
        self.as_object().and_then(|o| match o {
            Object::BitSet(bs) => Some(bs),
            _ => None,
        })
    }

    pub fn as_map_iter(&self) -> Option<(&MapStorage, &std::cell::Cell<usize>, &Vec<MapKey>)> {
        self.as_object().and_then(|o| match o {
            Object::MapIter(map, idx, keys) => Some((map, idx, keys)),
            _ => None,
        })
    }

    pub(crate) fn tuple(alloc: &mut dyn Allocator, items: Vec<Value>) -> Self {
        Self::from_object(alloc, Object::Tuple(items))
    }

    pub fn as_tuple(&self) -> Option<&Vec<Value>> {
        self.as_object().and_then(|o| match o {
            Object::Tuple(t) => Some(t),
            _ => None,
        })
    }

    pub(crate) fn list(alloc: &mut dyn Allocator, items: Vec<Value>) -> Self {
        Self::from_object(alloc, Object::List(RadList::new(items)))
    }

    pub(crate) fn map(alloc: &mut dyn Allocator, entries: MapStorage) -> Self {
        Self::from_object(alloc, Object::Map(entries))
    }

    pub(crate) fn bitset(alloc: &mut dyn Allocator, words: Vec<u64>) -> Self {
        Self::from_object(alloc, Object::BitSet(words))
    }

    pub(crate) fn buffer(alloc: &mut dyn Allocator, s: String) -> Self {
        Self::from_object(alloc, Object::Buffer(s))
    }

    pub fn as_buffer(&self) -> Option<&String> {
        self.as_object().and_then(|o| match o {
            Object::Buffer(b) => Some(b),
            _ => None,
        })
    }

    pub(crate) fn bytebuf(alloc: &mut dyn Allocator, bytes: Vec<u8>) -> Self {
        Self::from_object(alloc, Object::ByteBuf(bytes))
    }

    pub fn as_bytebuf(&self) -> Option<&Vec<u8>> {
        self.as_object().and_then(|o| match o {
            Object::ByteBuf(bytes) => Some(bytes),
            _ => None,
        })
    }

    pub(crate) fn world_fork(
        alloc: &mut dyn Allocator,
        snapshot: std::sync::Arc<crate::world::WorldSnapshot>,
    ) -> Self {
        Self::from_object(alloc, Object::WorldFork(snapshot))
    }

    /// Canonical (checker-resolved) name of a declared `system`, for `simulate` schedules.
    pub(crate) fn system_ref(alloc: &mut dyn Allocator, resolved_name: String) -> Self {
        Self::from_object(alloc, Object::SystemRef(resolved_name))
    }

    pub fn as_world_fork(&self) -> Option<&std::sync::Arc<crate::world::WorldSnapshot>> {
        self.as_object().and_then(|o| match o {
            Object::WorldFork(snap) => Some(snap),
            _ => None,
        })
    }

    pub fn as_system_ref(&self) -> Option<&str> {
        self.as_object().and_then(|o| match o {
            Object::SystemRef(s) => Some(s.as_str()),
            _ => None,
        })
    }

    pub(crate) fn component(
        alloc: &mut dyn Allocator,
        type_name: String,
        layout: Arc<Vec<String>>,
        values: Vec<Value>,
    ) -> Self {
        Self::from_object(
            alloc,
            Object::Component(ComponentData {
                type_name,
                layout,
                values,
            }),
        )
    }

    pub(crate) fn sum_type(
        alloc: &mut dyn Allocator,
        type_name: String,
        variant: String,
        fields: HashMap<String, Value>,
    ) -> Self {
        Self::from_object(
            alloc,
            Object::SumType(SumTypeInst {
                type_name,
                variant,
                fields,
            }),
        )
    }

    pub(crate) fn from_fn(alloc: &mut dyn Allocator, f: FnValue) -> Self {
        Self::from_object(alloc, Object::Fn(f))
    }

    pub(crate) fn from_closure(alloc: &mut dyn Allocator, c: ClosureValue) -> Self {
        Self::from_object(alloc, Object::Closure(c))
    }

    pub(crate) fn from_cell(alloc: &mut dyn Allocator, cell: *mut gc::CaptureCell) -> Self {
        Self::from_object(alloc, Object::Cell(cell))
    }

    pub(crate) fn from_builtin(alloc: &mut dyn Allocator, b: Builtin) -> Self {
        Self::from_object(alloc, Object::BuiltinFn(b))
    }

    pub(crate) fn from_native_fn(alloc: &mut dyn Allocator, info: NativeFnInfo) -> Self {
        Self::from_object(alloc, Object::NativeFn(info))
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn to_raw(self) -> u64 {
        self.0
    }

    /// Reconstruct a value from its raw ABI representation.
    ///
    /// # Safety
    ///
    /// Object-tagged values contain a raw GC pointer. The caller must prove
    /// that the pointer was produced by RAD, remains live, and belongs to the
    /// heap used by every subsequent operation on the returned value.
    pub unsafe fn from_raw_unchecked(raw: u64) -> Self {
        Self(raw)
    }

    pub(crate) fn from_entity_id(alloc: &mut dyn Allocator, id: u32) -> Self {
        Self::from_object(alloc, Object::EntityId(id))
    }

    pub(crate) fn from_task(alloc: &mut dyn Allocator, id: u64) -> Self {
        Self::from_object(alloc, Object::Task(id))
    }

    pub(crate) fn from_state(alloc: &mut dyn Allocator, machine: String, state: String) -> Self {
        Self::from_object(alloc, Object::State(StateInst { machine, state }))
    }

    pub(crate) fn from_component_data(alloc: &mut dyn Allocator, mut c: ComponentData) -> Self {
        for v in c.values.iter_mut() {
            if v.is_object() {
                *v = v.deep_copy(alloc);
            }
        }
        Self::from_object(alloc, Object::Component(c))
    }

    pub(crate) fn from_rad_list(alloc: &mut dyn Allocator, list: RadList) -> Self {
        Self::from_object(alloc, Object::List(list))
    }

    pub(crate) fn map_iter(
        alloc: &mut dyn Allocator,
        map_storage: MapStorage,
        keys: Vec<MapKey>,
    ) -> Self {
        Self::from_object(
            alloc,
            Object::MapIter(map_storage, std::cell::Cell::new(0), keys),
        )
    }

    /// Extract the list, cloning from the GC-managed Object.
    pub fn into_rad_list(self) -> Option<RadList> {
        self.as_object().and_then(|obj| match obj {
            Object::List(items) => Some(items.clone()),
            _ => None,
        })
    }

    /// Extract the map, cloning from the GC-managed Object.
    /// im::HashMap clone is O(1) via structural sharing.
    pub fn into_map(self) -> Option<MapStorage> {
        self.as_object().and_then(|obj| match obj {
            Object::Map(m) => Some(m.clone()),
            _ => None,
        })
    }

    /// Extract the string, cloning from the GC-managed Object.
    pub fn into_string(self) -> Option<String> {
        self.as_object().and_then(|obj| match obj {
            Object::Str(s) => Some(s.to_string()),
            _ => None,
        })
    }

    /// Extract the component data, cloning from the GC-managed Object.
    pub fn into_component(self) -> Option<ComponentData> {
        self.as_object().and_then(|obj| match obj {
            Object::Component(c) => Some(c.clone()),
            _ => None,
        })
    }

    pub fn into_bitset(self) -> Option<Vec<u64>> {
        self.as_object().and_then(|obj| match obj {
            Object::BitSet(bs) => Some(bs.clone()),
            _ => None,
        })
    }

    pub fn into_buffer(self) -> Option<String> {
        self.as_object().and_then(|obj| match obj {
            Object::Buffer(b) => Some(b.clone()),
            _ => None,
        })
    }

    pub fn into_bytebuf(self) -> Option<Vec<u8>> {
        self.as_object().and_then(|obj| match obj {
            Object::ByteBuf(bytes) => Some(bytes.clone()),
            _ => None,
        })
    }
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        if self.is_nil() {
            return false;
        }
        if let Some(b) = self.as_bool() {
            return b;
        }
        if let Some(i) = self.as_int() {
            return i != 0;
        }
        if let Some(f) = self.as_float() {
            return f != 0.0;
        }
        if let Some(obj) = self.as_object() {
            return match obj {
                Object::BigInt(n) => *n != 0,
                Object::Str(s) => !s.is_empty(),
                Object::SystemRef(s) => !s.is_empty(),
                Object::List(l) => !l.is_empty(),
                Object::Tuple(t) => !t.is_empty(),
                Object::Map(m) => !m.is_empty(),
                _ => true,
            };
        }
        true
    }

    pub fn print_display(&self) -> String {
        if let Some(s) = self.as_str() {
            return s.to_string();
        }
        self.to_string()
    }

    /// Deep-copy a value into the given allocator. Inline values (int, float,
    /// bool, nil) are returned unchanged. Heap objects are recursively cloned
    /// so the returned value is backed entirely by allocations in `target`.
    pub(crate) fn deep_copy(&self, target: &mut dyn Allocator) -> Self {
        if !self.is_object() {
            return *self;
        }
        match self.as_object() {
            Some(Object::Str(s)) => Self::from_object(target, Object::Str(Arc::clone(s))),
            Some(Object::BigInt(n)) => Self::from_int(target, *n),
            Some(Object::List(list)) => {
                let items: Vec<Value> = list.iter().map(|v| v.deep_copy(target)).collect();
                Self::list(target, items)
            }
            Some(Object::Tuple(items)) => {
                let copied: Vec<Value> = items.iter().map(|v| v.deep_copy(target)).collect();
                Self::tuple(target, copied)
            }
            Some(Object::Map(storage)) => {
                let copied: MapStorage = storage
                    .iter()
                    .map(|(k, v)| (k.clone(), v.deep_copy(target)))
                    .collect();
                Self::map(target, copied)
            }
            Some(Object::Component(c)) => {
                let values: Vec<Value> = c.values.iter().map(|v| v.deep_copy(target)).collect();
                Self::component(target, c.type_name.clone(), c.layout.clone(), values)
            }
            Some(Object::SumType(st)) => {
                let fields: HashMap<String, Value> = st
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.deep_copy(target)))
                    .collect();
                Self::sum_type(target, st.type_name.clone(), st.variant.clone(), fields)
            }
            Some(Object::State(s)) => Self::from_state(target, s.machine.clone(), s.state.clone()),
            Some(Object::EntityId(id)) => Self::from_entity_id(target, *id),
            Some(Object::BitSet(words)) => Self::bitset(target, words.clone()),
            Some(Object::Buffer(s)) => Self::buffer(target, s.clone()),
            Some(Object::ByteBuf(bytes)) => Self::bytebuf(target, bytes.clone()),
            Some(Object::SystemRef(s)) => Self::system_ref(target, s.clone()),
            Some(Object::WorldFork(snap)) => Self::world_fork(target, snap.clone()),
            Some(Object::Fn(f)) => Self::from_fn(target, f.clone()),
            Some(Object::Closure(c)) => Self::from_closure(target, c.clone()),
            Some(Object::Cell(ptr)) => Self::from_cell(target, *ptr),
            Some(Object::BuiltinFn(b)) => Self::from_builtin(target, *b),
            Some(Object::NativeFn(info)) => Self::from_native_fn(target, info.clone()),
            Some(Object::Task(id)) => Self::from_task(target, *id),
            Some(Object::MapIter(storage, _idx, keys)) => {
                let copied_storage: MapStorage = storage
                    .iter()
                    .map(|(k, v)| (k.clone(), v.deep_copy(target)))
                    .collect();
                Self::map_iter(target, copied_storage, keys.clone())
            }
            None => *self,
        }
    }

    /// Deep-rewrite every `EntityId` reachable from this value through the
    /// given remap table (world merge, #7). Untouched subtrees are returned
    /// as-is — only paths that actually contain a remapped id are rebuilt
    /// (into `target`). Map keys are rewritten too: `MapKey::Entity` is a
    /// reference like any other.
    pub(crate) fn rewrite_entity_ids(
        &self,
        remap: &HashMap<u32, u32>,
        target: &mut dyn Allocator,
    ) -> Self {
        if remap.is_empty() || !self.is_object() {
            return *self;
        }
        match self.as_object() {
            Some(Object::EntityId(id)) => match remap.get(id) {
                Some(&new_id) => Self::from_entity_id(target, new_id),
                None => *self,
            },
            Some(Object::List(list)) => {
                let items: Vec<Value> = list
                    .iter()
                    .map(|v| v.rewrite_entity_ids(remap, target))
                    .collect();
                if items.iter().zip(list.iter()).all(|(a, b)| a.0 == b.0) {
                    *self
                } else {
                    Self::list(target, items)
                }
            }
            Some(Object::Tuple(items)) => {
                let rewritten: Vec<Value> = items
                    .iter()
                    .map(|v| v.rewrite_entity_ids(remap, target))
                    .collect();
                if rewritten.iter().zip(items.iter()).all(|(a, b)| a.0 == b.0) {
                    *self
                } else {
                    Self::tuple(target, rewritten)
                }
            }
            Some(Object::Map(storage)) => {
                let mut changed = false;
                let rewritten: MapStorage = storage
                    .iter()
                    .map(|(k, v)| {
                        let new_k = match k {
                            MapKey::Entity(id) => match remap.get(id) {
                                Some(&new_id) => {
                                    changed = true;
                                    MapKey::Entity(new_id)
                                }
                                None => k.clone(),
                            },
                            other => other.clone(),
                        };
                        let new_v = v.rewrite_entity_ids(remap, target);
                        if new_v.0 != v.0 {
                            changed = true;
                        }
                        (new_k, new_v)
                    })
                    .collect();
                if changed {
                    Self::map(target, rewritten)
                } else {
                    *self
                }
            }
            Some(Object::Component(c)) => {
                let values: Vec<Value> = c
                    .values
                    .iter()
                    .map(|v| v.rewrite_entity_ids(remap, target))
                    .collect();
                if values.iter().zip(c.values.iter()).all(|(a, b)| a.0 == b.0) {
                    *self
                } else {
                    Self::component(target, c.type_name.clone(), c.layout.clone(), values)
                }
            }
            Some(Object::SumType(st)) => {
                let mut changed = false;
                let fields: HashMap<String, Value> = st
                    .fields
                    .iter()
                    .map(|(k, v)| {
                        let new_v = v.rewrite_entity_ids(remap, target);
                        if new_v.0 != v.0 {
                            changed = true;
                        }
                        (k.clone(), new_v)
                    })
                    .collect();
                if changed {
                    Self::sum_type(target, st.type_name.clone(), st.variant.clone(), fields)
                } else {
                    *self
                }
            }
            // Scalars, strings, functions, forks, … cannot contain entity ids.
            _ => *self,
        }
    }

    /// Rewrite every `EntityId` in a component payload through `remap`.
    /// Rebuilt values are allocated in `target` (typically the VM gc heap;
    /// they are persisted when the payload enters a world), as in the
    /// `load_world` decode path (world merge, #7).
    pub(crate) fn rewrite_component_entity_ids(
        data: &mut ComponentData,
        remap: &HashMap<u32, u32>,
        target: &mut dyn Allocator,
    ) {
        if remap.is_empty() {
            return;
        }
        for v in data.values.iter_mut() {
            *v = v.rewrite_entity_ids(remap, target);
        }
    }

    /// Deep-copy all values in a `ComponentData` into the given allocator.
    pub(crate) fn deep_copy_component_data(data: &mut ComponentData, target: &mut dyn Allocator) {
        for val in data.values.iter_mut() {
            if val.is_object() {
                *val = val.deep_copy(target);
            }
        }
    }

    /// Deep-copy all values in a `ComponentData` into the persistent store.
    /// This is the standard path for values entering the ECS world.
    pub(crate) fn persist_component_data(data: &mut ComponentData) {
        Self::deep_copy_component_data(data, &mut PersistentStore);
    }

    /// Release persistent references held by a component payload.
    pub(crate) fn release_component_data(data: &ComponentData) {
        for v in &data.values {
            unsafe { v.release_persistent() };
        }
    }

    /// Trace this value for GC.  If it is a heap object, insert its pointer
    /// into `marked` and recursively trace any values it contains.
    pub(crate) fn trace(&self, marked: &mut HashSet<usize>) {
        if let Some(ptr) = self.object_ptr() {
            if marked.insert(ptr) {
                if let Some(obj) = self.as_object() {
                    obj.trace(marked);
                }
            }
        }
    }

    pub fn type_name(&self) -> String {
        if self.is_nil() {
            return "nil".to_string();
        }
        if self.is_bool() {
            return "bool".to_string();
        }
        if self.is_inline_int() {
            return "int".to_string();
        }
        if self.is_float() {
            return "float".to_string();
        }
        match self.as_object() {
            Some(Object::BigInt(_)) => "int".to_string(),
            Some(Object::Str(_)) => "str".to_string(),
            Some(Object::List(_)) => "list".to_string(),
            Some(Object::Tuple(_)) => "tuple".to_string(),
            Some(Object::Component(c)) => c.type_name.clone(),
            Some(Object::State(s)) => s.machine.clone(),
            Some(Object::SumType(st)) => st.type_name.clone(),
            Some(Object::Fn(_)) => "function".to_string(),
            Some(Object::Closure(_)) => "closure".to_string(),
            Some(Object::Cell(cell)) => unsafe { (**cell).get().type_name() },
            Some(Object::BuiltinFn(_)) => "builtin".to_string(),
            Some(Object::NativeFn(_)) => "native_fn".to_string(),
            Some(Object::EntityId(_)) => "entity".to_string(),
            Some(Object::Task(_)) => "task".to_string(),
            Some(Object::Map(_)) => "map".to_string(),
            Some(Object::MapIter(_, _, _)) => "map_iter".to_string(),
            Some(Object::BitSet(_)) => "bitset".to_string(),
            Some(Object::Buffer(_)) => "buffer".to_string(),
            Some(Object::ByteBuf(_)) => "bytebuf".to_string(),
            Some(Object::SystemRef(_)) => "system".to_string(),
            Some(Object::WorldFork(_)) => "world_fork".to_string(),
            None => "unknown".to_string(),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if self.0 == other.0 {
            return true;
        }
        if let (Some(a), Some(b)) = (self.as_int(), other.as_int()) {
            return a == b;
        }
        if let (Some(a), Some(b)) = (self.as_float(), other.as_float()) {
            return a == b;
        }
        match (self.as_object(), other.as_object()) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Value({})", self)
    }
}