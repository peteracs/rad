//! Resource meters used only while RFC-0002 constraints execute.
//!
//! Keeping these meters separate from the ordinary VM sandbox budget is
//! important: one constraint invocation must not consume another one's
//! semantic allowance, and retained rejection data must be bounded while it
//! is collected rather than after the fact.
//!
//! Native fuel is a deterministic semantic cost unit, not a count of CPU
//! instructions or wall-clock time. Constant-time audited calls cost one
//! native unit; input-sized calls cost a conservative number of units derived
//! from the checked input plan. Heap quotes separately bound peak temporary
//! plus retained allocation before the call executes.

use crate::constraint_types::{ConstraintEvaluationFailure, ConstraintViolation};
use crate::value::{Builtin, Object, Value};

#[derive(Clone, Copy, Debug)]
pub(crate) struct BuiltinResourceCharge {
    pub(crate) fuel: u64,
    pub(crate) heap: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeProofClass {
    Fixed,
    Filled,
    Range,
    Bitset,
    Replace,
    TextScan,
    TypeName,
    FactLookup,
}

#[derive(Clone, Copy, Debug)]
struct NativeContract {
    builtin: Builtin,
    proof_id: &'static str,
    proof_class: NativeProofClass,
}
// Lexical sections preserve one private semantic namespace.
include!("constraint_runtime/metering.rs");
include!("constraint_runtime/tests.rs");
