use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::gc;

/// Object-safe allocator trait implemented by both `GcHeap` (backup collector)
/// and `BumpArena` (ephemeral per-system allocator).
pub(crate) trait Allocator {
    fn alloc_object(&mut self, obj: Object) -> *mut Object;
    fn pointer_tag(&self) -> u64 {
        0
    }
}

/// Allocator for persistent ECS world values.
///
/// Backing objects are created as `Arc<Object>` and encoded in `Value` with a
/// dedicated persistent pointer tag. They are not traced by VM GC.
pub(crate) struct PersistentStore;

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
        if let Some(x) = self.as_float() {
            return if x.fract() == 0.0 && x.is_finite() {
                write!(f, "{:.1}", x)
            } else {
                write!(f, "{}", x)
            };
        }
        match self.as_object() {
            Some(Object::BigInt(n)) => write!(f, "{}", n),
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
            Some(Object::EntityId(id)) => write!(f, "{}", id),
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
    pub func: crate::ffi::NativeFnPtr,
    pub arity: u32,
}

pub enum Object {
    BigInt(i64),
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
    EntityId(u32),
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
            (Object::NativeFn(a), Object::NativeFn(b)) => a.func as usize == b.func as usize,
            (Object::EntityId(a), Object::EntityId(b)) => a == b,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Print,
    Len,
    TypeOf,
    VariantOf,
    SysArgs,
    Str,
    Int,
    Float,
    Abs,
    Sign,
    Min,
    Max,
    Unwrap,
    Expect,
    Push,
    Pop,
    PopLast,
    DropLast,
    Sort,
    Reverse,
    Slice,
    Map,
    Filter,
    Reduce,
    Range,
    Get,
    Lookup,
    LookupAll,
    Set,
    Has,
    Spawn,
    GetEntity,
    RequireEntity,
    Remove,
    Despawn,
    Entities,
    GetResource,
    SetResource,
    Transition,
    Keys,
    Contains,
    Format,
    Entries,
    Merge,
    RemoveKey,
    GroupBy,
    Split,
    Join,
    Trim,
    Replace,
    StartsWith,
    EndsWith,
    Append,
    Extend,
    Zip,
    FlatMap,
    TryInt,
    TryFloat,
    Chr,
    Ord,
    Chars,
    ToUpper,
    ToLower,
    Values,
    ReadFile,
    WriteFile,
    HttpGet,
    RegexIsMatch,
    RegexFind,
    NowUnixS,
    NowUnixMs,
    RandInt,
    RandFloat,
    RandBool,
    RandSeed,
    GenInt,
    GenFloat,
    GenStr,
    GenBool,
    GenList,
    Input,
    Readline,
    Assert,
    AssertEq,
    IntDiv,
    SortBy,
    UnwrapOr,
    IsSome,
    IsNone,
    Require,
    RequireAll,
    MapOr,
    LoadExtension,
    GcCollect,
    Eprint,
    WriteStdout,
    WriteStderr,
    ReadStdinAll,
    FlushStdout,
    SleepMs,
    NameOf,
    IdOf,
    AppendFile,
    FileExists,
    RemoveFile,
    ListDir,
    CreateDir,
    RemoveDir,
    ReadFileBytes,
    WriteFileBytes,
    HttpPost,
    HttpPostJson,
    HttpRequest,
    TcpConnect,
    TcpListen,
    TcpAccept,
    TcpAcceptTimeout,
    TcpRead,
    TcpWrite,
    TcpClose,
    UdpBind,
    UdpRecvFrom,
    UdpRecvFromTimeout,
    UdpRecvFromBytes,
    UdpRecvFromBytesTimeout,
    UdpRecvByteBuf,
    UdpRecvByteBufTimeout,
    UdpSendTo,
    UdpSendToBytes,
    UdpSendByteBuf,
    UdpClose,
    QueryWhere,
    QueryMap,
    QueryCount,
    WithField,
    Log,
    Metric,
    TraceId,
    FlushEvents,
    ByteAt,
    SubstringBytes,
    ByteLen,
    BitsetNew,
    BitsetSet,
    BitsetHas,
    BitsetClear,
    BufferNew,
    BufferAppend,
    BufferToStr,
    ByteBufNew,
    ByteBufLen,
    ByteBufGet,
    ByteBufSetU8,
    ByteBufSetU32Le,
    ByteBufSetI32Le,
    ByteBufGetU32Le,
    ByteBufGetI32Le,
    ByteBufToList,
    ByteBufFromList,
    Fork,
    Simulate,
    Commit,
    Clock,
    Peek,
    PeekResource,
    DebugTrace,
    FormatValue,
    Enumerate,
    Find,
    MaxBy,
    MinBy,
    Round,
    Floor,
    Ceil,
    Sqrt,
    Pow,
    ToFixed,
    JsonStringify,
    JsonParse,
    SimulatePar,
    SandboxRun,
    SandboxInput,
    SandboxOutput,
    SandboxLastOutput,
    SandboxLastFuel,
    SimulateMany,
    SimulateSeeded,
    ForkWith,
    ForkSeed,
    Diff,
    AssertOnlyChanged,
    Why,
    WhyResource,
    SaveWorld,
    LoadWorld,
    TryLoadWorld,
    WorldDigest,
    SchemaDigest,
    MergeForks,
    MergeForksWith,
    ForkToBytes,
    ForkFromBytes,
    ForkDelta,
    ForkApply,
    Popcount,
    Ctz,
    Shl,
    Shr,
    Filled,
    SetAt,
    Res,
    Sum,
    Product,
    GetOr,
    Clamp,
    IndexOf,
    Any,
    All,
    DropFirst,
    RecentEvents,
}

impl Builtin {
    pub const ALL: [Builtin; 220] = [
        Builtin::GetOr,
        Builtin::Clamp,
        Builtin::IndexOf,
        Builtin::Any,
        Builtin::All,
        Builtin::DropFirst,
        Builtin::RecentEvents,
        Builtin::Popcount,
        Builtin::Ctz,
        Builtin::Shl,
        Builtin::Shr,
        Builtin::Filled,
        Builtin::SetAt,
        Builtin::Res,
        Builtin::Sum,
        Builtin::Product,
        Builtin::Print,
        Builtin::Len,
        Builtin::TypeOf,
        Builtin::VariantOf,
        Builtin::SysArgs,
        Builtin::Str,
        Builtin::Int,
        Builtin::Float,
        Builtin::Abs,
        Builtin::Min,
        Builtin::Max,
        Builtin::Unwrap,
        Builtin::Expect,
        Builtin::Push,
        Builtin::Pop,
        Builtin::PopLast,
        Builtin::DropLast,
        Builtin::Sort,
        Builtin::Reverse,
        Builtin::Slice,
        Builtin::Map,
        Builtin::Filter,
        Builtin::Reduce,
        Builtin::Range,
        Builtin::Get,
        Builtin::Lookup,
        Builtin::LookupAll,
        Builtin::Set,
        Builtin::Has,
        Builtin::Spawn,
        Builtin::GetEntity,
        Builtin::RequireEntity,
        Builtin::Sign,
        Builtin::PeekResource,
        Builtin::Remove,
        Builtin::Despawn,
        Builtin::Entities,
        Builtin::GetResource,
        Builtin::SetResource,
        Builtin::Transition,
        Builtin::Keys,
        Builtin::Contains,
        Builtin::Format,
        Builtin::Entries,
        Builtin::Merge,
        Builtin::RemoveKey,
        Builtin::GroupBy,
        Builtin::Split,
        Builtin::Join,
        Builtin::Trim,
        Builtin::Replace,
        Builtin::StartsWith,
        Builtin::EndsWith,
        Builtin::Append,
        Builtin::Extend,
        Builtin::Zip,
        Builtin::FlatMap,
        Builtin::TryInt,
        Builtin::TryFloat,
        Builtin::Chr,
        Builtin::Ord,
        Builtin::Chars,
        Builtin::ToUpper,
        Builtin::ToLower,
        Builtin::Values,
        Builtin::ReadFile,
        Builtin::WriteFile,
        Builtin::HttpGet,
        Builtin::RegexIsMatch,
        Builtin::RegexFind,
        Builtin::NowUnixS,
        Builtin::NowUnixMs,
        Builtin::RandInt,
        Builtin::RandFloat,
        Builtin::RandBool,
        Builtin::RandSeed,
        Builtin::GenInt,
        Builtin::GenFloat,
        Builtin::GenStr,
        Builtin::GenBool,
        Builtin::GenList,
        Builtin::Input,
        Builtin::Readline,
        Builtin::Assert,
        Builtin::AssertEq,
        Builtin::IntDiv,
        Builtin::SortBy,
        Builtin::UnwrapOr,
        Builtin::IsSome,
        Builtin::IsNone,
        Builtin::Require,
        Builtin::RequireAll,
        Builtin::MapOr,
        Builtin::LoadExtension,
        Builtin::GcCollect,
        Builtin::Eprint,
        Builtin::WriteStdout,
        Builtin::WriteStderr,
        Builtin::ReadStdinAll,
        Builtin::FlushStdout,
        Builtin::SleepMs,
        Builtin::NameOf,
        Builtin::IdOf,
        Builtin::AppendFile,
        Builtin::FileExists,
        Builtin::RemoveFile,
        Builtin::ListDir,
        Builtin::CreateDir,
        Builtin::RemoveDir,
        Builtin::ReadFileBytes,
        Builtin::WriteFileBytes,
        Builtin::HttpPost,
        Builtin::HttpPostJson,
        Builtin::HttpRequest,
        Builtin::TcpConnect,
        Builtin::TcpListen,
        Builtin::TcpAccept,
        Builtin::TcpAcceptTimeout,
        Builtin::TcpRead,
        Builtin::TcpWrite,
        Builtin::TcpClose,
        Builtin::UdpBind,
        Builtin::UdpRecvFrom,
        Builtin::UdpRecvFromTimeout,
        Builtin::UdpRecvFromBytes,
        Builtin::UdpRecvFromBytesTimeout,
        Builtin::UdpRecvByteBuf,
        Builtin::UdpRecvByteBufTimeout,
        Builtin::UdpSendTo,
        Builtin::UdpSendToBytes,
        Builtin::UdpSendByteBuf,
        Builtin::UdpClose,
        Builtin::QueryWhere,
        Builtin::QueryMap,
        Builtin::QueryCount,
        Builtin::WithField,
        Builtin::Log,
        Builtin::Metric,
        Builtin::TraceId,
        Builtin::FlushEvents,
        Builtin::ByteAt,
        Builtin::SubstringBytes,
        Builtin::ByteLen,
        Builtin::BitsetNew,
        Builtin::BitsetSet,
        Builtin::BitsetHas,
        Builtin::BitsetClear,
        Builtin::BufferNew,
        Builtin::BufferAppend,
        Builtin::BufferToStr,
        Builtin::ByteBufNew,
        Builtin::ByteBufLen,
        Builtin::ByteBufGet,
        Builtin::ByteBufSetU8,
        Builtin::ByteBufSetU32Le,
        Builtin::ByteBufSetI32Le,
        Builtin::ByteBufGetU32Le,
        Builtin::ByteBufGetI32Le,
        Builtin::ByteBufToList,
        Builtin::ByteBufFromList,
        Builtin::Fork,
        Builtin::Simulate,
        Builtin::Commit,
        Builtin::Clock,
        Builtin::Peek,
        Builtin::DebugTrace,
        Builtin::FormatValue,
        Builtin::Enumerate,
        Builtin::Find,
        Builtin::MaxBy,
        Builtin::MinBy,
        Builtin::Round,
        Builtin::Floor,
        Builtin::Ceil,
        Builtin::Sqrt,
        Builtin::Pow,
        Builtin::ToFixed,
        Builtin::JsonStringify,
        Builtin::JsonParse,
        Builtin::SimulatePar,
        Builtin::SandboxRun,
        Builtin::SandboxInput,
        Builtin::SandboxOutput,
        Builtin::SandboxLastOutput,
        Builtin::SandboxLastFuel,
        Builtin::SimulateMany,
        Builtin::SimulateSeeded,
        Builtin::ForkWith,
        Builtin::ForkSeed,
        Builtin::Diff,
        Builtin::AssertOnlyChanged,
        Builtin::Why,
        Builtin::WhyResource,
        Builtin::SaveWorld,
        Builtin::LoadWorld,
        Builtin::TryLoadWorld,
        Builtin::WorldDigest,
        Builtin::SchemaDigest,
        Builtin::MergeForks,
        Builtin::MergeForksWith,
        Builtin::ForkToBytes,
        Builtin::ForkFromBytes,
        Builtin::ForkDelta,
        Builtin::ForkApply,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::Print => "print",
            Builtin::Len => "len",
            Builtin::TypeOf => "typeof",
            Builtin::VariantOf => "variant_of",
            Builtin::SysArgs => "sys_args",
            Builtin::Str => "str",
            Builtin::Int => "int",
            Builtin::Float => "float",
            Builtin::Abs => "abs",
            Builtin::Sign => "sign",
            Builtin::Popcount => "popcount",
            Builtin::Ctz => "ctz",
            Builtin::Shl => "shl",
            Builtin::Shr => "shr",
            Builtin::Filled => "filled",
            Builtin::SetAt => "set_at",
            Builtin::Res => "res",
            Builtin::Sum => "sum",
            Builtin::Product => "product",
            Builtin::GetOr => "get_or",
            Builtin::Clamp => "clamp",
            Builtin::IndexOf => "index_of",
            Builtin::Any => "any",
            Builtin::All => "all",
            Builtin::DropFirst => "drop_first",
            Builtin::RecentEvents => "recent_events",
            Builtin::Min => "min",
            Builtin::Max => "max",
            Builtin::Unwrap => "unwrap",
            Builtin::Expect => "expect",
            Builtin::Push => "push",
            Builtin::Pop => "pop",
            Builtin::PopLast => "pop_last",
            Builtin::DropLast => "drop_last",
            Builtin::Sort => "sort",
            Builtin::Reverse => "reverse",
            Builtin::Slice => "slice",
            Builtin::Map => "map",
            Builtin::Filter => "filter",
            Builtin::Reduce => "reduce",
            Builtin::Range => "range",
            Builtin::Get => "get",
            Builtin::Lookup => "lookup",
            Builtin::LookupAll => "lookup_all",
            Builtin::Set => "set",
            Builtin::Has => "has",
            Builtin::Spawn => "spawn",
            Builtin::GetEntity => "get_entity",
            Builtin::RequireEntity => "require_entity",
            Builtin::Remove => "remove",
            Builtin::Despawn => "despawn",
            Builtin::Entities => "entities",
            Builtin::GetResource => "get_resource",
            Builtin::SetResource => "set_resource",
            Builtin::Transition => "transition",
            Builtin::Keys => "keys",
            Builtin::Contains => "contains",
            Builtin::Format => "format",
            Builtin::Entries => "entries",
            Builtin::Merge => "merge",
            Builtin::GroupBy => "group_by",
            Builtin::Split => "split",
            Builtin::Join => "join",
            Builtin::Trim => "trim",
            Builtin::Replace => "replace",
            Builtin::StartsWith => "starts_with",
            Builtin::EndsWith => "ends_with",
            Builtin::Append => "append",
            Builtin::Extend => "extend",
            Builtin::Zip => "zip",
            Builtin::FlatMap => "flat_map",
            Builtin::TryInt => "try_int",
            Builtin::TryFloat => "try_float",
            Builtin::Chr => "chr",
            Builtin::Ord => "ord",
            Builtin::Chars => "chars",
            Builtin::ToUpper => "to_upper",
            Builtin::ToLower => "to_lower",
            Builtin::Values => "values",
            Builtin::ReadFile => "read_file",
            Builtin::WriteFile => "write_file",
            Builtin::HttpGet => "http_get",
            Builtin::RegexIsMatch => "regex_is_match",
            Builtin::RegexFind => "regex_find",
            Builtin::NowUnixS => "now_unix_s",
            Builtin::NowUnixMs => "now_unix_ms",
            Builtin::RandInt => "rand_int",
            Builtin::RandFloat => "rand_float",
            Builtin::RandBool => "rand_bool",
            Builtin::RandSeed => "rand_seed",
            Builtin::GenInt => "gen_int",
            Builtin::GenFloat => "gen_float",
            Builtin::GenStr => "gen_str",
            Builtin::GenBool => "gen_bool",
            Builtin::GenList => "gen_list",
            Builtin::Input => "input",
            Builtin::Readline => "readline",
            Builtin::Assert => "assert",
            Builtin::AssertEq => "assert_eq",
            Builtin::IntDiv => "int_div",
            Builtin::SortBy => "sort_by",
            Builtin::UnwrapOr => "unwrap_or",
            Builtin::IsSome => "is_some",
            Builtin::IsNone => "is_none",
            Builtin::Require => "require",
            Builtin::RequireAll => "require_all",
            Builtin::MapOr => "map_or",
            Builtin::LoadExtension => "load_extension",
            Builtin::GcCollect => "gc_collect",
            Builtin::Eprint => "eprint",
            Builtin::WriteStdout => "write_stdout",
            Builtin::WriteStderr => "write_stderr",
            Builtin::ReadStdinAll => "read_stdin_all",
            Builtin::FlushStdout => "flush_stdout",
            Builtin::SleepMs => "sleep_ms",
            Builtin::NameOf => "name_of",
            Builtin::IdOf => "id_of",
            Builtin::AppendFile => "append_file",
            Builtin::FileExists => "file_exists",
            Builtin::RemoveFile => "remove_file",
            Builtin::ListDir => "list_dir",
            Builtin::CreateDir => "create_dir",
            Builtin::RemoveDir => "remove_dir",
            Builtin::ReadFileBytes => "read_file_bytes",
            Builtin::WriteFileBytes => "write_file_bytes",
            Builtin::HttpPost => "http_post",
            Builtin::HttpPostJson => "http_post_json",
            Builtin::HttpRequest => "http_request",
            Builtin::TcpConnect => "tcp_connect",
            Builtin::TcpListen => "tcp_listen",
            Builtin::TcpAccept => "tcp_accept",
            Builtin::TcpAcceptTimeout => "tcp_accept_timeout",
            Builtin::TcpRead => "tcp_read",
            Builtin::TcpWrite => "tcp_write",
            Builtin::TcpClose => "tcp_close",
            Builtin::UdpBind => "udp_bind",
            Builtin::UdpRecvFrom => "udp_recv_from",
            Builtin::UdpRecvFromTimeout => "udp_recv_from_timeout",
            Builtin::UdpRecvFromBytes => "udp_recv_from_bytes",
            Builtin::UdpRecvFromBytesTimeout => "udp_recv_from_bytes_timeout",
            Builtin::UdpRecvByteBuf => "udp_recv_bytebuf",
            Builtin::UdpRecvByteBufTimeout => "udp_recv_bytebuf_timeout",
            Builtin::UdpSendTo => "udp_send_to",
            Builtin::UdpSendToBytes => "udp_send_to_bytes",
            Builtin::UdpSendByteBuf => "udp_send_bytebuf",
            Builtin::UdpClose => "udp_close",
            Builtin::QueryWhere => "query_where",
            Builtin::QueryMap => "query_map",
            Builtin::QueryCount => "query_count",
            Builtin::WithField => "with_field",
            Builtin::RemoveKey => "remove_key",
            Builtin::Log => "log",
            Builtin::Metric => "metric",
            Builtin::TraceId => "trace_id",
            Builtin::FlushEvents => "flush_events",
            Builtin::ByteAt => "byte_at",
            Builtin::SubstringBytes => "substring_bytes",
            Builtin::ByteLen => "byte_len",
            Builtin::BitsetNew => "bitset_new",
            Builtin::BitsetSet => "bitset_set",
            Builtin::BitsetHas => "bitset_has",
            Builtin::BitsetClear => "bitset_clear",
            Builtin::BufferNew => "buffer_new",
            Builtin::BufferAppend => "buffer_append",
            Builtin::BufferToStr => "buffer_to_str",
            Builtin::ByteBufNew => "bytebuf_new",
            Builtin::ByteBufLen => "bytebuf_len",
            Builtin::ByteBufGet => "bytebuf_get",
            Builtin::ByteBufSetU8 => "bytebuf_set_u8",
            Builtin::ByteBufSetU32Le => "bytebuf_set_u32_le",
            Builtin::ByteBufSetI32Le => "bytebuf_set_i32_le",
            Builtin::ByteBufGetU32Le => "bytebuf_get_u32_le",
            Builtin::ByteBufGetI32Le => "bytebuf_get_i32_le",
            Builtin::ByteBufToList => "bytebuf_to_list",
            Builtin::ByteBufFromList => "bytebuf_from_list",
            Builtin::Fork => "fork",
            Builtin::Simulate => "simulate",
            Builtin::Commit => "commit",
            Builtin::Clock => "clock",
            Builtin::Peek => "peek",
            Builtin::PeekResource => "peek_resource",
            Builtin::DebugTrace => "debug_trace",
            Builtin::FormatValue => "format_value",
            Builtin::Enumerate => "enumerate",
            Builtin::Find => "find",
            Builtin::MaxBy => "max_by",
            Builtin::MinBy => "min_by",
            Builtin::Round => "round",
            Builtin::Floor => "floor",
            Builtin::Ceil => "ceil",
            Builtin::Sqrt => "sqrt",
            Builtin::Pow => "pow",
            Builtin::ToFixed => "to_fixed",
            Builtin::JsonStringify => "json_stringify",
            Builtin::JsonParse => "json_parse",
            Builtin::SimulatePar => "simulate_par",
            Builtin::SandboxRun => "sandbox_run",
            Builtin::SandboxInput => "sandbox_input",
            Builtin::SandboxOutput => "sandbox_output",
            Builtin::SandboxLastOutput => "sandbox_last_output",
            Builtin::SandboxLastFuel => "sandbox_last_fuel",
            Builtin::SimulateMany => "simulate_many",
            Builtin::SimulateSeeded => "simulate_seeded",
            Builtin::ForkWith => "fork_with",
            Builtin::ForkSeed => "fork_seed",
            Builtin::Diff => "diff",
            Builtin::AssertOnlyChanged => "assert_only_changed",
            Builtin::Why => "why",
            Builtin::WhyResource => "why_resource",
            Builtin::SaveWorld => "save_world",
            Builtin::WorldDigest => "world_digest",
            Builtin::SchemaDigest => "schema_digest",
            Builtin::LoadWorld => "load_world",
            Builtin::TryLoadWorld => "try_load_world",
            Builtin::MergeForks => "merge_forks",
            Builtin::MergeForksWith => "merge_forks_with",
            Builtin::ForkToBytes => "fork_to_bytes",
            Builtin::ForkFromBytes => "fork_from_bytes",
            Builtin::ForkDelta => "fork_delta",
            Builtin::ForkApply => "fork_apply",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentData {
    pub type_name: String,
    pub layout: Arc<Vec<String>>,
    pub(crate) values: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateInst {
    pub machine: String,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SumTypeInst {
    pub type_name: String,
    pub variant: String,
    pub(crate) fields: HashMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnValue {
    pub name: String,
    pub arity: u8,
    pub chunk_id: usize,
}

/// A closure captures zero or more `CaptureCell` pointers.
///
/// Each pointer is a GC-managed raw pointer to a `CaptureCell`.
/// Multiple closures may share the same cell (aliased raw pointers;
/// the GC keeps cells alive as long as any closure referencing them
/// is reachable).
#[derive(Clone, Debug)]
pub struct ClosureValue {
    pub name: String,
    pub arity: u8,
    pub chunk_id: usize,
    pub captures: Vec<*mut gc::CaptureCell>,
}

unsafe impl Send for ClosureValue {}
unsafe impl Sync for ClosureValue {}

impl PartialEq for ClosureValue {
    fn eq(&self, other: &Self) -> bool {
        self.chunk_id == other.chunk_id
            && self.arity == other.arity
            && self.captures.len() == other.captures.len()
            && self
                .captures
                .iter()
                .zip(other.captures.iter())
                .all(|(&a, &b)| unsafe { (*a).get() == (*b).get() })
    }
}

impl Eq for ClosureValue {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineOp {
    Map,
    Filter,
}

impl fmt::Display for ComponentData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {{", display_type_name(&self.type_name))?;
        if !self.layout.is_empty() {
            write!(f, " ")?;
            let mut first = true;
            for (k, v) in self.layout.iter().zip(self.values.iter()) {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                write!(f, "{}: {}", k, v)?;
            }
            write!(f, " ")?;
        }
        write!(f, "}}")
    }
}

#[cfg(test)]
mod tests {
    use super::{Builtin, Value, ValueTag, CANONICAL_FLOAT_NAN};
    use std::collections::HashSet;

    #[test]
    fn builtin_all_has_unique_names() {
        let mut seen = HashSet::new();
        for builtin in Builtin::ALL {
            assert!(
                seen.insert(builtin.name()),
                "duplicate builtin registration: {}",
                builtin.name()
            );
        }
    }

    #[test]
    fn every_float_nan_is_canonical_and_never_object_tagged() {
        let reserved_and_ieee_patterns = [
            0x7FF0_0000_0000_0001,
            0x7FF8_0000_0000_0000,
            0x7FFC_0000_0000_0000,
            0x7FFF_FFFF_FFFF_FFFF,
            0xFFF0_0000_0000_0001,
            0xFFF8_0000_0000_0000,
            0xFFFC_0000_0000_0000,
            0xFFFC_0000_0000_0001,
            0xFFFC_7FFF_FFFF_FFFF,
            0xFFFC_8000_0000_0000,
            0xFFFF_FFFF_FFFF_FFFF,
        ];

        for bits in reserved_and_ieee_patterns {
            let input = f64::from_bits(bits);
            assert!(input.is_nan(), "test pattern {bits:#018x} must be NaN");
            let value = Value::from_float(input);
            assert_eq!(value.to_raw(), CANONICAL_FLOAT_NAN, "input {bits:#018x}");
            assert_eq!(value.tag(), ValueTag::Float, "input {bits:#018x}");
            assert!(value.is_float(), "input {bits:#018x}");
            assert!(!value.is_heap_object_tag(), "input {bits:#018x}");
            assert!(value.as_float().is_some_and(f64::is_nan));
        }

        let zero = std::hint::black_box(0.0_f64);
        for arithmetic_nan in [
            zero / zero,
            f64::INFINITY - f64::INFINITY,
            (-1.0_f64).sqrt(),
        ] {
            let value = Value::from_float(arithmetic_nan);
            assert_eq!(value.to_raw(), CANONICAL_FLOAT_NAN);
            assert_eq!(value.tag(), ValueTag::Float);
        }
    }
}
