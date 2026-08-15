//! Crate-private integration tests.
//!
//! These suites intentionally remain inside `rad-vm`: they verify seams that
//! require private state without widening the public API for test access.

pub(crate) mod causal_laws;
mod composition;
mod migration;
