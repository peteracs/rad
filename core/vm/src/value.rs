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
// Lexical sections preserve one private semantic namespace.
include!("value/storage.rs");
include!("value/representation.rs");
include!("value/objects.rs");
include!("value/builtins_and_tests.rs");
