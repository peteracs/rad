use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::value::{ComponentData, Value};

type ArchetypeId = u32;
type TypeId = u32;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct IndexKey {
    type_name: String,
    field_name: String,
    value: IndexValue,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum IndexValue {
    Int(i64),
    Str(String),
    Bool(bool),
    Entity(u32),
    Float(u64),
}
// Lexical sections preserve one private semantic namespace.
include!("world/storage.rs");
include!("world/entity_allocator.rs");
include!("world/world_operations.rs");
include!("world/snapshot_model.rs");
include!("world/snapshot_encoding.rs");
include!("world/tests.rs");
