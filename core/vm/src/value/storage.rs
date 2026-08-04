

impl Allocator for PersistentStore {
    // `Object` is deliberately not `Send`/`Sync`: the store is confined to one
    // VM thread and the `Arc` here provides refcounting, not cross-thread sharing.
    #[allow(clippy::arc_with_non_send_sync)]
    fn alloc_object(&mut self, obj: Object) -> *mut Object {
        let ptr = Arc::into_raw(Arc::new(obj)) as *mut Object;
        persist_audit::on_alloc(ptr as usize);
        ptr
    }

    fn pointer_tag(&self) -> u64 {
        PERSISTENT_PTR_TAG
    }
}

/// Opt-in debugging aid for persistent-store refcount bugs: run with
/// `RAD_PERSIST_AUDIT=1` to shadow every live persistent allocation. In audit
/// mode a release that would free the object LEAKS it instead (recording the
/// releasing backtrace), so a later touch of the dead object cannot fault —
/// it aborts deterministically, reporting both the touching and the freeing
/// stack. Off (a single atomic load) unless the environment variable is set.
mod persist_audit {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    enum Entry {
        Live(usize),
        Dead(String),
    }

    fn registry() -> Option<&'static Mutex<HashMap<usize, Entry>>> {
        static REGISTRY: OnceLock<Option<Mutex<HashMap<usize, Entry>>>> = OnceLock::new();
        REGISTRY
            .get_or_init(|| {
                if std::env::var_os("RAD_PERSIST_AUDIT").is_some_and(|v| v == "1") {
                    Some(Mutex::new(HashMap::new()))
                } else {
                    None
                }
            })
            .as_ref()
    }

    pub(super) fn on_alloc(ptr: usize) {
        if let Some(reg) = registry() {
            reg.lock().unwrap().insert(ptr, Entry::Live(1));
        }
    }

    pub(super) fn on_retain(ptr: usize) {
        if let Some(reg) = registry() {
            let mut live = reg.lock().unwrap();
            match live.get_mut(&ptr) {
                Some(Entry::Live(n)) => *n += 1,
                Some(Entry::Dead(bt)) => die("retain", ptr, bt),
                None => die("retain", ptr, "<never allocated during audit>"),
            }
        }
    }

    /// Returns true when the caller should really decrement the Arc count.
    /// In audit mode the final decrement is skipped (the object leaks), so
    /// use-after-free becomes a deterministic report instead of an AV.
    pub(super) fn on_release(ptr: usize) -> bool {
        let Some(reg) = registry() else {
            return true;
        };
        let mut live = reg.lock().unwrap();
        match live.get_mut(&ptr) {
            Some(Entry::Live(1)) => {
                let bt = format!("{}", std::backtrace::Backtrace::force_capture());
                live.insert(ptr, Entry::Dead(bt));
                false
            }
            Some(Entry::Live(n)) => {
                *n -= 1;
                true
            }
            Some(Entry::Dead(bt)) => die("release", ptr, bt),
            None => die("release", ptr, "<never allocated during audit>"),
        }
    }

    pub(super) fn on_read(ptr: usize) {
        if let Some(reg) = registry() {
            let live = reg.lock().unwrap();
            match live.get(&ptr) {
                Some(Entry::Live(_)) => {}
                Some(Entry::Dead(bt)) => die("read", ptr, bt),
                None => die("read", ptr, "<never allocated during audit>"),
            }
        }
    }

    fn die(op: &str, ptr: usize, death: &str) -> ! {
        eprintln!(
            "persistent-store audit: {op} of DEAD persistent object {ptr:#x}\n\
             --- backtrace of the release that killed it ---\n{death}\n\
             --- current backtrace follows in the panic ---"
        );
        panic!("persistent-store audit: {op} of dead persistent object {ptr:#x}");
    }
}

pub(crate) fn display_type_name(name: &str) -> &str {
    if let Some(rest) = name.strip_prefix("__mod_") {
        if let Some(pos) = rest.find("__") {
            return &rest[pos + 2..];
        }
    }
    name
}

thread_local! {
    static PROFILE_COPIES_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PROFILE_COPIES_LINE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub fn set_profile_copy_context(enabled: bool, line: u32) {
    PROFILE_COPIES_ENABLED.with(|flag| flag.set(enabled));
    PROFILE_COPIES_LINE.with(|current_line| current_line.set(line));
}

fn maybe_emit_profile_copy(list_len: usize, strong_count: usize) {
    PROFILE_COPIES_ENABLED.with(|enabled| {
        if !enabled.get() || strong_count <= 1 {
            return;
        }
        let line = PROFILE_COPIES_LINE.with(|current_line| current_line.get());
        eprintln!(
            "[profile-copy] list<{} elements> cloned (Arc refcount={}) at line {}",
            list_len, strong_count, line
        );
    });
}

/// List backed by a contiguous `Vec<Value>` inside an `Arc`.
///
/// Cloning the list is O(1) (reference count). Mutations use `Arc::make_mut`:
/// when the list is uniquely owned, updates happen in place; when shared, the
/// backing `Vec` is copied once.
#[derive(Clone, Debug)]
pub struct RadList(Arc<Vec<Value>>);

impl RadList {
    pub fn new(items: Vec<Value>) -> Self {
        RadList(Arc::new(items))
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, idx: usize) -> Option<&Value> {
        self.0.get(idx)
    }

    pub fn to_vec(&self) -> Vec<Value> {
        self.0.as_ref().clone()
    }

    pub fn iter(&self) -> RadListIter<'_> {
        RadListIter {
            inner: self.0.iter(),
        }
    }

    pub fn into_vec(self) -> Vec<Value> {
        match Arc::try_unwrap(self.0) {
            Ok(v) => v,
            Err(arc) => arc.as_ref().clone(),
        }
    }

    pub fn set(&mut self, idx: usize, val: Value) -> Result<(), String> {
        if idx >= self.0.len() {
            return Err(format!("List index {} out of bounds", idx));
        }
        let strong_count = Arc::strong_count(&self.0);
        maybe_emit_profile_copy(self.0.len(), strong_count);
        let v = Arc::make_mut(&mut self.0);
        v[idx] = val;
        Ok(())
    }

    pub fn push(&mut self, val: Value) {
        let strong_count = Arc::strong_count(&self.0);
        maybe_emit_profile_copy(self.0.len(), strong_count);
        Arc::make_mut(&mut self.0).push(val);
    }

    pub fn last(&self) -> Option<&Value> {
        self.0.last()
    }

    pub fn contains(&self, val: &Value) -> bool {
        self.0.iter().any(|v| v == val)
    }

    pub fn slice(&self, start: usize, end: usize) -> Vec<Value> {
        let end = end.min(self.len());
        let start = start.min(end);
        self.0[start..end].to_vec()
    }

    pub fn into_slice(self, start: usize, end: usize) -> Vec<Value> {
        let end = end.min(self.len());
        let start = start.min(end);
        let vec = self.into_vec();
        vec.into_iter().skip(start).take(end - start).collect()
    }

    pub fn pop(&mut self) -> Option<Value> {
        Arc::make_mut(&mut self.0).pop()
    }

    pub fn extend_from(&mut self, other: &RadList) {
        let strong_count = Arc::strong_count(&self.0);
        maybe_emit_profile_copy(self.0.len(), strong_count);
        Arc::make_mut(&mut self.0).extend_from_slice(other.0.as_slice());
    }

    pub fn as_slice(&self) -> &[Value] {
        self.0.as_slice()
    }
}

impl PartialEq for RadList {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_ref() == other.0.as_ref()
    }
}

impl Eq for RadList {}

pub struct RadListIter<'a> {
    inner: std::slice::Iter<'a, Value>,
}

impl<'a> Iterator for RadListIter<'a> {
    type Item = &'a Value;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a> ExactSizeIterator for RadListIter<'a> {}

/// A hashable, ordered key for use in Rad maps.
///
/// Only value types with well-defined equality semantics are allowed:
/// `int`, `str`, `bool`, and `entity`. Mutable/compound types (list, map,
/// fn, float) are intentionally excluded.
#[derive(Clone, Debug)]
pub enum MapKey {
    Int(i64),
    Str(String),
    Bool(bool),
    Entity(u32),
    /// Tuples of valid key types (floats stay excluded recursively) —
    /// the grid-coordinate key `(x, y)` every pathfinder wants.
    Tuple(Vec<MapKey>),
}

impl MapKey {
    pub fn from_value(v: &Value) -> Result<MapKey, String> {
        if let Some(i) = v.as_int() {
            Ok(MapKey::Int(i))
        } else if let Some(s) = v.as_str() {
            Ok(MapKey::Str(s.to_string()))
        } else if let Some(b) = v.as_bool() {
            Ok(MapKey::Bool(b))
        } else if let Some(e) = v.as_entity_id() {
            Ok(MapKey::Entity(e))
        } else if let Some(items) = v.as_tuple() {
            let keys: Result<Vec<MapKey>, String> = items.iter().map(MapKey::from_value).collect();
            keys.map(MapKey::Tuple)
                .map_err(|e| format!("Tuple map key contains an invalid element: {}", e))
        } else {
            Err(format!(
                "Type '{}' cannot be used as a map key",
                v.type_name()
            ))
        }
    }

    pub fn to_value(&self, gc: &mut crate::gc::GcHeap) -> Value {
        match self {
            MapKey::Int(i) => Value::from_int(gc, *i),
            MapKey::Str(s) => Value::from_string(gc, s.clone()),
            MapKey::Bool(b) => Value::from_bool(*b),
            MapKey::Entity(e) => Value::from_entity_id(gc, *e),
            MapKey::Tuple(items) => {
                let vals: Vec<Value> = items.iter().map(|k| k.to_value(gc)).collect();
                Value::tuple(gc, vals)
            }
        }
    }
}

impl PartialEq for MapKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (MapKey::Int(a), MapKey::Int(b)) => a == b,
            (MapKey::Str(a), MapKey::Str(b)) => a == b,
            (MapKey::Bool(a), MapKey::Bool(b)) => a == b,
            (MapKey::Entity(a), MapKey::Entity(b)) => a == b,
            (MapKey::Tuple(a), MapKey::Tuple(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for MapKey {}

impl Hash for MapKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            MapKey::Int(i) => i.hash(state),
            MapKey::Str(s) => s.hash(state),
            MapKey::Bool(b) => b.hash(state),
            MapKey::Entity(e) => e.hash(state),
            MapKey::Tuple(items) => items.hash(state),
        }
    }
}

impl PartialOrd for MapKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MapKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        fn discriminant_rank(k: &MapKey) -> u8 {
            match k {
                MapKey::Bool(_) => 0,
                MapKey::Int(_) => 1,
                MapKey::Entity(_) => 2,
                MapKey::Str(_) => 3,
                MapKey::Tuple(_) => 4,
            }
        }
        let rank_a = discriminant_rank(self);
        let rank_b = discriminant_rank(other);
        if rank_a != rank_b {
            return rank_a.cmp(&rank_b);
        }
        match (self, other) {
            (MapKey::Int(a), MapKey::Int(b)) => a.cmp(b),
            (MapKey::Str(a), MapKey::Str(b)) => a.cmp(b),
            (MapKey::Bool(a), MapKey::Bool(b)) => a.cmp(b),
            (MapKey::Entity(a), MapKey::Entity(b)) => a.cmp(b),
            (MapKey::Tuple(a), MapKey::Tuple(b)) => a.cmp(b),
            _ => std::cmp::Ordering::Equal,
        }
    }
}

impl fmt::Display for MapKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapKey::Int(i) => write!(f, "{}", i),
            MapKey::Str(s) => write!(f, "\"{}\"", s),
            MapKey::Bool(b) => write!(f, "{}", b),
            MapKey::Entity(e) => write!(f, "entity({})", e),
            MapKey::Tuple(items) => {
                write!(f, "(")?;
                for (i, k) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", k)?;
                }
                write!(f, ")")
            }
        }
    }
}

pub type MapStorage = im::HashMap<MapKey, Value>;

/// NaN-boxed runtime value for the Rad VM.
///
/// Every value fits in a single `u64`, using IEEE-754 NaN space to encode
/// non-float types.  This makes `Value` `Copy` — cloning is a trivial
/// bit-copy with zero overhead.
///
/// Layout:
///   Float:   any f64 whose bits don't match the QNAN|SIGN pattern below
///   Nil:     QNAN | TAG_NIL
///   Bool:    QNAN | TAG_BOOL | (0 or 1)
///   Inline Int: SIGN_BIT | QNAN | INT_TAG_BIT | (47-bit payload)
///   Heap Object: SIGN_BIT | QNAN | (raw pointer, bit 47 = 0)
///
/// Heap objects are managed by the GC (`gc::GcHeap`).  The GC is the sole
/// owner of every heap allocation.  `Value` is `Copy` — no refcounts, no
/// drop glue.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Value(u64);

const QNAN: u64 = 0x7FFC_0000_0000_0000;
const SIGN_BIT: u64 = 0x8000_0000_0000_0000;
const TAG_NIL: u64 = 1;
const TAG_FALSE: u64 = 2;
const TAG_TRUE: u64 = 3;

const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
/// LSB tag set for pointers that come from `PersistentStore`.
///
/// `Object` allocations are naturally aligned, so bit 0 is always available.
const PERSISTENT_PTR_TAG: u64 = 1;

/// Bit 47 of the 48-bit payload distinguishes inline integers from heap
/// object pointers (userspace pointers always have bit 47 = 0).
const INT_TAG_BIT: u64 = 1u64 << 47;
/// 47-bit payload mask (bits 0-46).
const INT_PAYLOAD_MASK: u64 = INT_TAG_BIT - 1; // 0x00007FFF_FFFFFFFF
const INLINE_INT_MIN: i64 = -(1i64 << 46); // -70_368_744_177_664
const INLINE_INT_MAX: i64 = (1i64 << 46) - 1; //  70_368_744_177_663

/// A floating NaN representation that is outside every RAD boxed-tag family.
///
/// `Value::from_float` canonicalizes all NaNs to this exact pattern.  That is
/// part of the representation's safety boundary: arbitrary IEEE NaN payloads
/// must never be mistaken for GC pointers merely because their high bits
/// overlap RAD's private NaN-box tags.
const CANONICAL_FLOAT_NAN: u64 = 0x7FF8_0000_0000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueTag {
    Float,
    Nil,
    Bool,
    Int,
    Object,
}