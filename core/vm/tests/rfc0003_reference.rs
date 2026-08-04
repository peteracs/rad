//! Executable semantic oracle for RFC-0003.
//!
//! This deliberately contains no parser, bytecode, VM, GC, or ECS code. It is
//! a generic typed relation model. Full recomputation defines derivation
//! semantics; the affected-relation projection harness checks invalidation
//! closure and atomicity without claiming an independent incremental engine.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use sha2::{Digest, Sha256};

// The oracle is intentionally one private semantic namespace. These lexical
// parts follow the transaction/derivation pipeline without manufacturing a
// public API between mutually dependent reference phases. Contract tests are
// separate modules grouped by observable behavior.
include!("rfc0003_reference/model.rs");
include!("rfc0003_reference/rule_plan.rs");
include!("rfc0003_reference/derivation.rs");
include!("rfc0003_reference/encoding.rs");
include!("rfc0003_reference/fixtures.rs");

#[path = "rfc0003_reference/tests/authoritative_candidate.rs"]
mod authoritative_candidate;
#[path = "rfc0003_reference/tests/derivation_safety.rs"]
mod derivation_safety;
#[path = "rfc0003_reference/tests/derivation_semantics.rs"]
mod derivation_semantics;
#[path = "rfc0003_reference/tests/finalization_and_work.rs"]
mod finalization_and_work;
#[path = "rfc0003_reference/tests/identity_replay_and_deletion.rs"]
mod identity_replay_and_deletion;
#[path = "rfc0003_reference/tests/layout.rs"]
mod layout;
#[path = "rfc0003_reference/tests/plan_and_decode.rs"]
mod plan_and_decode;
