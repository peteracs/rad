//! Schema migration tests (list item #5): `save_world()` / `load_world()`
//! and the `migrate X(old) { … }` declaration.

use crate::value::{Builtin, Value};
use crate::vm::VM;

/// Field value of a component by name (layout-order independent).
fn field_of(data: &crate::value::ComponentData, name: &str) -> Value {
    let index = data
        .layout
        .iter()
        .position(|field| field == name)
        .unwrap_or_else(|| panic!("no field '{name}' in {:?}", data.layout));
    data.values[index]
}

// Lexical sections preserve one private semantic namespace.
include!("migration/migration_semantics.rs");
include!("migration/persistence_identity.rs");
include!("migration/allocator_transport.rs");
