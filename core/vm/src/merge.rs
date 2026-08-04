//! Three-way world merge (list item #7) — `merge(base, ours, theirs)`.
//!
//! Git semantics, applied to program state instead of text — with one move
//! git cannot make. Because the language owns 100% of state, merge operates
//! at **field granularity** (a conflict is the *same field* of the same
//! entity diverging from base in both forks, nothing coarser), and entity
//! identity is handled honestly:
//!
//! - Entity **ids are runtime handles**, not identity. Two forks spawning
//!   different entities that happen to collide on an id is *not* a conflict:
//!   theirs is remapped to a fresh id and **every `EntityId` reference
//!   contributed by theirs is deep-rewritten** (lists, maps — keys included —
//!   tuples, sum types, nested components).
//! - Entity **names are semantic identity**. Two forks claiming the same
//!   name for different entities *is* a conflict.
//!
//! The rewrite happens on theirs' flattened view *before* any comparison,
//! so a theirs-side reference to a colliding spawn can never spuriously
//! compare equal to an ours-side reference: after remapping it points at a
//! fresh id that exists in neither base nor ours.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::value::{Allocator, ComponentData, Value};
use crate::world::{World, WorldSnapshot};

/// One merge conflict — **data, not prose**. Carries the subject (entity or
/// resource), the component/field, and the actual diverging values, so a
/// resolution policy is a `match` in user code rather than string parsing.
/// `Display` renders the human report; the structure is the API.
#[derive(Clone, Debug)]
pub enum MergeConflict {
    /// The same field of the same entity's component diverged in both forks.
    /// Mechanically resolvable with a value — see [`Resolutions`].
    Field {
        entity: u32,
        entity_name: Option<String>,
        component: String,
        field: String,
        base: Value,
        ours: Value,
        theirs: Value,
    },
    /// Component-set conflicts: removed in one fork but modified in the
    /// other, added in both with different values, or layout drift.
    Component {
        entity: u32,
        entity_name: Option<String>,
        component: String,
        detail: String,
    },
    /// Despawned in one fork, modified in the other.
    Despawn {
        entity: u32,
        entity_name: Option<String>,
        detail: String,
    },
    /// Renamed differently in both forks ("" = unnamed).
    Rename {
        entity: u32,
        base: String,
        ours: String,
        theirs: String,
    },
    /// One name claimed by several entities after merge — names are identity.
    NameClaim { name: String, entities: Vec<u32> },
    /// A resource field diverged in both forks. Mechanically resolvable.
    ResourceField {
        resource: String,
        field: String,
        base: Value,
        ours: Value,
        theirs: Value,
    },
    /// Resource-level conflicts (initialized in both forks, layout drift).
    Resource { resource: String, detail: String },
    /// In-flight event queues were consumed or reordered relative to base.
    Events {
        detail: String,
        base: usize,
        ours: usize,
        theirs: usize,
    },
    /// Authoritative relation state changed in at least one branch. RFC-0003
    /// assertion-aware three-way merge is deliberately fail-closed until it
    /// can preserve unique constraints, lifetimes, and provenance.
    Relations { detail: String },
}
// Lexical sections preserve one private semantic namespace.
include!("merge/engine.rs");
include!("merge/tests.rs");
