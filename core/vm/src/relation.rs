//! First-class relation language, storage, and derived-fact evaluation.
//!
//! The dependency direction is intentional:
//!
//! ```text
//! frontend -> sealed plans/manifests
//! runtime  -> authoritative facts
//! derivation(frontend plans + runtime facts) -> derived facts
//! ```

pub mod derivation;
pub use rad_relation::frontend;
pub mod runtime;
