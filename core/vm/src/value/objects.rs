

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_nil() {
            return write!(f, "nil");
        }
        if let Some(b) = self.as_bool() {
            return write!(f, "{}", b);
        }
        if let Some(i) = self.as_int() {
            return write!(f, "{}", i);
        }
        if let Some(entity) = self.as_entity_id() {
            return write!(f, "{}", entity);
        }
        if let Some(x) = self.as_float() {
            return if x.fract() == 0.0 && x.is_finite() {
                write!(f, "{:.1}", x)
            } else {
                write!(f, "{}", x)
            };
        }
        match self.as_object() {
            Some(Object::BigInt(n)) => write!(f, "{}", n),
            Some(Object::NativeScalar(value)) => {
                if let Some(number) = value.float() {
                    write!(f, "{}({})", display_type_name(&value.type_name), number)
                } else if value.repr.is_signed() {
                    write!(
                        f,
                        "{}({})",
                        display_type_name(&value.type_name),
                        value.signed().unwrap()
                    )
                } else {
                    write!(
                        f,
                        "{}({})",
                        display_type_name(&value.type_name),
                        value.unsigned().unwrap()
                    )
                }
            }
            Some(Object::NativeType(value)) => {
                write!(f, "<native type {}>", display_type_name(&value.name))
            }
            Some(Object::Str(s)) => {
                write!(f, "\"")?;
                for ch in s.chars() {
                    match ch {
                        '"' => write!(f, "\\\"")?,
                        '\\' => write!(f, "\\\\")?,
                        '\n' => write!(f, "\\n")?,
                        '\r' => write!(f, "\\r")?,
                        '\t' => write!(f, "\\t")?,
                        c if c.is_control() => write!(f, "\\u{{{:04x}}}", c as u32)?,
                        c => write!(f, "{}", c)?,
                    }
                }
                write!(f, "\"")
            }
            Some(Object::List(list)) => {
                write!(f, "[")?;
                for (i, v) in list.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Some(Object::Tuple(items)) => {
                write!(f, "(")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                if items.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
            Some(Object::Component(c)) => write!(f, "{}", c),
            Some(Object::State(s)) => write!(f, "{}::{}", display_type_name(&s.machine), s.state),
            Some(Object::SumType(st)) => {
                write!(f, "{}::{}", display_type_name(&st.type_name), st.variant)?;
                if !st.fields.is_empty() {
                    write!(f, " {{")?;
                    // Sorted: st.fields is a hash-seeded HashMap, and unsorted
                    // iteration order here would differ across processes —
                    // a determinism leak that breaks record/replay.
                    let mut sorted_fields: Vec<(&String, &Value)> = st.fields.iter().collect();
                    sorted_fields.sort_by(|a, b| a.0.cmp(b.0));
                    let mut first = true;
                    for (k, v) in sorted_fields {
                        if !first {
                            write!(f, ", ")?;
                        }
                        first = false;
                        write!(f, "{}: {}", k, v)?;
                    }
                    write!(f, " }}")?;
                } else {
                    write!(f, " {{}}")?;
                }
                Ok(())
            }
            Some(Object::Fn(fun)) => write!(f, "<fn {}({})>", fun.name, fun.arity),
            Some(Object::Closure(c)) => write!(f, "<closure {}({})>", c.name, c.arity),
            Some(Object::Cell(cell)) => write!(f, "{}", unsafe { (**cell).get() }),
            Some(Object::BuiltinFn(builtin)) => write!(f, "<builtin {}>", builtin.name()),
            Some(Object::NativeFn(native)) => write!(f, "<native fn {}>", native.name),
            Some(Object::HostHandle(handle)) => {
                write!(f, "<host_handle {}>", display_type_name(handle.type_name()))
            }
            Some(Object::Task(id)) => write!(f, "<task {}>", id),
            Some(Object::Map(m)) => {
                write!(f, "{{")?;
                let mut sorted_keys: Vec<&MapKey> = m.keys().collect();
                sorted_keys.sort();
                let mut first = true;
                for k in sorted_keys {
                    if !first {
                        write!(f, ", ")?;
                    }
                    first = false;
                    write!(f, "{}: {}", k, m[k])?;
                }
                write!(f, "}}")
            }
            Some(Object::MapIter(_, _, _)) => write!(f, "<map_iter>"),
            Some(Object::BitSet(_)) => write!(f, "<bitset>"),
            Some(Object::Buffer(b)) => write!(f, "<buffer len={}>", b.len()),
            Some(Object::ByteBuf(bytes)) => write!(f, "<bytebuf len={}>", bytes.len()),
            Some(Object::SystemRef(s)) => write!(f, "<system {}>", display_type_name(s)),
            Some(Object::WorldFork(_)) => write!(f, "<world_fork>"),
            None => write!(f, "<unknown>"),
        }
    }
}

#[derive(Clone)]
pub struct NativeFnInfo {
    pub name: String,
    pub execution: crate::ffi::NativeExecution,
    pub arity: u32,
    pub effects: crate::ffi::NativeEffectSet,
    pub signature: String,
    pub deterministic: bool,
    pub replayable: bool,
    /// Content-addressed identity of the library implementation that owns
    /// this export. Function pointers are process-local and never enter
    /// portable program or replay identity.
    pub extension: std::sync::Arc<crate::ffi::NativeExtensionManifest>,
}

pub enum Object {
    BigInt(i64),
    NativeScalar(crate::native_types::NativeScalarValue),
    NativeType(crate::native_types::NativeTypeDescriptor),
    Str(Arc<str>),
    List(RadList),
    Component(ComponentData),
    State(StateInst),
    SumType(SumTypeInst),
    Fn(FnValue),
    Closure(ClosureValue),
    Cell(*mut gc::CaptureCell),
    BuiltinFn(Builtin),
    NativeFn(NativeFnInfo),
    HostHandle(crate::ffi::HostHandleToken),
    Task(u64),
    Tuple(Vec<Value>),
    Map(MapStorage),
    MapIter(MapStorage, std::cell::Cell<usize>, Vec<MapKey>),
    BitSet(Vec<u64>),
    Buffer(String),
    ByteBuf(Vec<u8>),
    /// Resolved `system` name (same string the VM uses for `run_system_by_name`).
    SystemRef(String),
    WorldFork(std::sync::Arc<crate::world::WorldSnapshot>),
}

impl Object {
    /// Deterministic retained-size estimate for allocations owned by this
    /// object but not covered by `size_of::<Object>()`. Causal code cannot
    /// mutate these containers in place, so the allocation-time charge stays
    /// valid for an invocation's heap budget.
    pub(crate) fn accounted_heap_bytes(&self) -> usize {
        fn key_bytes(key: &MapKey) -> usize {
            match key {
                MapKey::Int(_) => std::mem::size_of::<i64>(),
                MapKey::Native(value) => 10usize.saturating_add(value.type_name.len()),
                MapKey::Str(value) => value.len(),
                MapKey::Bool(_) => 1,
                MapKey::Entity(_) => std::mem::size_of::<u32>(),
                MapKey::Tuple(values) => values.iter().map(key_bytes).sum(),
            }
        }
        fn text_fields(fields: &HashMap<String, Value>) -> usize {
            fields
                .keys()
                .map(|name| name.len().saturating_add(std::mem::size_of::<Value>()))
                .sum()
        }

        match self {
            Object::BigInt(_) | Object::NativeScalar(_) => 0,
            Object::NativeType(descriptor) => descriptor
                .name
                .len()
                .saturating_add(descriptor.members.iter().map(|(name, _)| name.len() + 8).sum()),
            Object::Str(value) => value.len(),
            Object::List(values) => values.len().saturating_mul(std::mem::size_of::<Value>()),
            Object::Component(component) => component
                .type_name
                .len()
                .saturating_add(
                    component
                        .values
                        .len()
                        .saturating_mul(std::mem::size_of::<Value>()),
                )
                .saturating_add(component.layout.iter().map(|name| name.len()).sum()),
            Object::State(state) => state.machine.len().saturating_add(state.state.len()),
            Object::SumType(sum) => sum
                .type_name
                .len()
                .saturating_add(sum.variant.len())
                .saturating_add(text_fields(&sum.fields)),
            Object::Fn(function) => function.name.len(),
            Object::Closure(closure) => closure
                .captures
                .len()
                .saturating_mul(std::mem::size_of::<*mut gc::CaptureCell>()),
            Object::Cell(_) | Object::BuiltinFn(_) | Object::Task(_) => 0,
            Object::NativeFn(native) => native.name.len(),
            Object::HostHandle(handle) => {
                handle.owner_digest().len().saturating_add(handle.type_name().len())
            }
            Object::Tuple(values) => values.len().saturating_mul(std::mem::size_of::<Value>()),
            Object::Map(map) => map
                .keys()
                .map(|key| key_bytes(key).saturating_add(std::mem::size_of::<Value>()))
                .sum(),
            Object::MapIter(map, _, order) => map
                .keys()
                .map(|key| key_bytes(key).saturating_add(std::mem::size_of::<Value>()))
                .sum::<usize>()
                .saturating_add(order.iter().map(key_bytes).sum()),
            Object::BitSet(words) => words.len().saturating_mul(std::mem::size_of::<u64>()),
            Object::Buffer(value) => value.len(),
            Object::ByteBuf(bytes) => bytes.len(),
            Object::SystemRef(name) => name.len(),
            Object::WorldFork(_) => 0,
        }
    }

    pub fn trace(&self, marked: &mut HashSet<usize>) {
        match self {
            Object::List(list) => {
                for val in list.iter() {
                    val.trace(marked);
                }
            }
            Object::Tuple(items) => {
                for val in items {
                    val.trace(marked);
                }
            }
            Object::Map(map) => {
                for val in map.values() {
                    val.trace(marked);
                }
            }
            Object::MapIter(map, _, _) => {
                for val in map.values() {
                    val.trace(marked);
                }
            }
            Object::Component(comp) => {
                for val in &comp.values {
                    val.trace(marked);
                }
            }
            Object::Closure(closure) => {
                for &cell_ptr in &closure.captures {
                    let ptr = cell_ptr as usize;
                    if marked.insert(ptr) {
                        unsafe { (*cell_ptr).get().trace(marked) };
                    }
                }
            }
            Object::Cell(cell) => {
                let ptr = *cell as usize;
                if marked.insert(ptr) {
                    unsafe { (**cell).get().trace(marked) };
                }
            }
            Object::SumType(sum) => {
                for val in sum.fields.values() {
                    val.trace(marked);
                }
            }
            Object::WorldFork(snap) => {
                snap.trace(marked);
            }
            _ => {}
        }
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Object::BigInt(a), Object::BigInt(b)) => a == b,
            (Object::NativeScalar(a), Object::NativeScalar(b)) => a == b,
            (Object::NativeType(a), Object::NativeType(b)) => a == b,
            (Object::Str(a), Object::Str(b)) => a == b,
            (Object::List(a), Object::List(b)) => a == b,
            (Object::Tuple(a), Object::Tuple(b)) => a == b,
            (Object::Component(a), Object::Component(b)) => a == b,
            (Object::State(a), Object::State(b)) => a == b,
            (Object::SumType(a), Object::SumType(b)) => a == b,
            (Object::Fn(a), Object::Fn(b)) => a == b,
            (Object::Closure(a), Object::Closure(b)) => a == b,
            (Object::Cell(a), Object::Cell(b)) => unsafe { (**a).get() == (**b).get() },
            (Object::BuiltinFn(a), Object::BuiltinFn(b)) => a == b,
            (Object::NativeFn(a), Object::NativeFn(b)) => {
                a.name == b.name && a.extension.digest() == b.extension.digest()
            }
            (Object::HostHandle(a), Object::HostHandle(b)) => a == b,
            (Object::Task(a), Object::Task(b)) => a == b,
            (Object::Map(a), Object::Map(b)) => a == b,
            (Object::BitSet(a), Object::BitSet(b)) => a == b,
            (Object::Buffer(a), Object::Buffer(b)) => a == b,
            (Object::ByteBuf(a), Object::ByteBuf(b)) => a == b,
            (Object::SystemRef(a), Object::SystemRef(b)) => a == b,
            (Object::WorldFork(a), Object::WorldFork(b)) => std::sync::Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for Object {}
