//! Typed, pointer-free contract for RFC-0002 candidate validation.
//!
//! Fixtures, hosts, WASM, attempt replay, and the VM share these bounded
//! semantic results instead of parsing compatibility error strings.

use crate::host_value::FrozenValue;
use crate::{CausalValueError, CausalValueLimits};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Write;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;

/// Version 3 makes `fuel_per_invocation` an every-opcode contract and
/// `max_heap_bytes_per_invocation` a disposable-heap contract.
pub const CONSTRAINT_LIMIT_PROFILE_VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstraintLimitProfile {
    value_limits: CausalValueLimits,
    fuel_per_invocation: u64,
    max_heap_bytes_per_invocation: usize,
    max_violations_per_invocation: usize,
    max_violations_per_settlement: usize,
    max_serialized_outcome_bytes: usize,
    max_aggregate_fuel: u64,
    max_aggregate_heap_bytes: usize,
}
// Lexical sections preserve one private semantic namespace.
include!("constraint_types/profiles_and_candidates.rs");
include!("constraint_types/tests.rs");
