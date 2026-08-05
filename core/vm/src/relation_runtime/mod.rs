//! RFC-0003 authoritative relation runtime.
//!
//! It installs the immutable front-end manifest, stores authoritative
//! assertion lifetimes, and constructs relation candidates without mutating
//! the live world. Candidate adoption triggers the separate derivation layer.

mod candidate;
mod encoding;
mod manifest;
mod store;
mod transaction_profile;
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
pub use transaction_profile::{
    BoundedRelationTransaction, BoundedRelationTransactionBuilder, RelationTransactionProfile,
};
pub(crate) use value::canonical_fact_key;
pub use value::{
    EntityOperand, EntityRef, FactKey, FactValue, OperationMetadata, RelationRuntimeError,
    RelationRuntimeResult,
};

#[cfg(test)]
mod tests;
