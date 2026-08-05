//! Bounded full-recompute evaluation for RFC-0003 derived relations.
//!
//! The front end is the only producer of executable rule plans. This module
//! consumes those immutable artifacts and authoritative assertions, producing
//! a complete derived state or one typed failure without mutating the world.

mod encoding;
mod evaluator;
mod explanation;
mod meter;
mod model;

pub use evaluator::derive_all;
pub(crate) use explanation::explain_fact;
pub use model::{
    DerivationError, DerivationLimits, DerivationResult, DerivationStats, DerivedRelationState,
    ProofAlternative, SupportRef,
};

#[cfg(test)]
mod tests;
