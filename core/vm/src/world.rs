use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::value::{ComponentData, Value};

type ArchetypeId = u32;
type TypeId = u32;

/// Hash map for dense integer keys on the field-access hot path.
///
/// `resolve_entity_address` probes two of these per field read or write —
/// about 72M probes in the dispatch60 production run — and SipHash accounted
/// for roughly a tenth of that workload's samples. Entity and type ids are
/// internally allocated counters, not untrusted input, so DoS resistance buys
/// nothing here. Every site that iterates these maps sorts first, so encoding
/// and digests stay independent of hasher order.
pub(crate) type FastMap<K, V> = std::collections::HashMap<K, V, rustc_hash::FxBuildHasher>;

/// A field address resolved once for a data kernel. It is valid while the
/// world's append-only type registry and component layout remain installed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedFieldAddress {
    type_id: TypeId,
    field_index: usize,
    indexed: bool,
}

/// One entity's stable row for the duration of a view-kernel callback. A
/// callback that changes membership is detected before this address is reused.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedEntityAddress {
    archetype: ArchetypeId,
    row: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct IndexKey {
    type_name: String,
    field_name: String,
    value: IndexValue,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum IndexValue {
    Min,
    Int(i64),
    Native(crate::native_types::NativeScalarValue),
    Str(String),
    Bool(bool),
    Entity(u32),
    Float(u64),
    Tuple(Vec<IndexValue>),
    Max,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MaterializedViewChange {
    pub revision: u64,
    pub entity: u32,
    pub entered: bool,
    pub reason: String,
}

#[derive(Clone, Debug)]
struct MaterializedViewState {
    dependencies: Vec<String>,
    key: Option<(String, String)>,
    predicate: crate::materialized_view::MaterializedViewPredicate,
    members: BTreeSet<u32>,
    first_member: Option<u32>,
    next_members: HashMap<u32, u32>,
    key_members: BTreeMap<IndexValue, u32>,
    entity_keys: HashMap<u32, IndexValue>,
    revision: u64,
    changes: Vec<MaterializedViewChange>,
    reasons: HashMap<u32, String>,
    #[cfg(test)]
    evaluation_count: u64,
}

/// Portable operational state for one compiled materialized view. View
/// definitions never cross a trust boundary: the receiving program installs
/// its own definition, recomputes the derived membership from authoritative
/// rows, and admits this history only after the two agree.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterializedViewTransport {
    revision: u64,
    members: BTreeSet<u32>,
    entity_keys: BTreeMap<u32, IndexValue>,
    changes: Vec<MaterializedViewChange>,
    reasons: BTreeMap<u32, String>,
}

impl MaterializedViewState {
    fn insert_member(&mut self, entity: u32) -> bool {
        use std::ops::Bound::{Excluded, Unbounded};
        if self.members.contains(&entity) {
            return false;
        }
        let predecessor = self.members.range(..entity).next_back().copied();
        let successor = self
            .members
            .range((Excluded(entity), Unbounded))
            .next()
            .copied();
        if let Some(predecessor) = predecessor {
            self.next_members.insert(predecessor, entity);
        } else {
            self.first_member = Some(entity);
        }
        if let Some(successor) = successor {
            self.next_members.insert(entity, successor);
        } else {
            self.next_members.remove(&entity);
        }
        self.members.insert(entity)
    }

    fn remove_member(&mut self, entity: u32) -> bool {
        if !self.members.remove(&entity) {
            return false;
        }
        let successor = self.next_members.remove(&entity);
        let predecessor = self.members.range(..entity).next_back().copied();
        if let Some(predecessor) = predecessor {
            if let Some(successor) = successor {
                self.next_members.insert(predecessor, successor);
            } else {
                self.next_members.remove(&predecessor);
            }
        } else {
            self.first_member = successor;
        }
        true
    }
}
// Lexical sections preserve one private semantic namespace.
include!("world/storage.rs");
include!("world/entity_allocator.rs");
include!("world/world_operations.rs");
include!("world/materialized_views.rs");
include!("world/indexes.rs");
include!("world/relations.rs");
include!("world/snapshot_model.rs");
include!("world/checkpoint_encoding.rs");
include!("world/snapshot_json.rs");
include!("world/snapshot_diff.rs");
include!("world/snapshot_capture.rs");
include!("world/tests.rs");
