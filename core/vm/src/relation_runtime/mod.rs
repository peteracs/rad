//! RFC-0003 authoritative relation runtime.
//!
//! This layer deliberately stops before derived evaluation. It installs the
//! immutable front-end manifest, stores authoritative assertion lifetimes,
//! and constructs relation candidates without mutating the live world.

mod candidate;
mod encoding;
mod manifest;
mod store;
mod value;

pub use candidate::{
    CandidateEntityState, PendingComponentWrite, PendingDespawn, PendingFactKey,
    PendingRelationOperation, PendingRelationValue, PendingSpawn, RelationCandidate,
    RelationTransaction,
};
pub use manifest::{RelationRuntimeManifest, RuntimeRelationSchema};
pub use store::{
    AuthoritativeRelationState, FactAssertion, FactChange, FactChangeKind, LogicalFactRow,
    UniqueIndexKey,
};
pub use value::{
    EntityOperand, EntityRef, FactKey, FactValue, OperationMetadata, RelationRuntimeError,
    RelationRuntimeResult,
};

#[cfg(test)]
mod tests;
