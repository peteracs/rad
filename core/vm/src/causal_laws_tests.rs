//! Executable acceptance tests for RFC-0001's vertical slice.

use crate::causality::CausalityLedger;
use crate::checker::{Checker, CheckerOptions};
use crate::compiler::Compiler;
use crate::host_value::FrozenValue;
use crate::lexer::Lexer;
use crate::opcode::{Chunk, Op};
use crate::parser::Parser;
use crate::replay::TraceReplayer;
use crate::sandbox::SandboxCaps;
use crate::settlement_reference::{
    settle_reference, ReferenceComponent, ReferenceProposal, ReferenceResolver, ReferenceValue,
    ReferenceWorld, ReferenceWrite,
};
use crate::value::{FnValue, Value};
use crate::vm::VM;
use crate::CausalValueLimits;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

const FEATURE: &str = "causal_laws";
// Lexical sections preserve one private semantic namespace.
include!("causal_laws_tests/core_settlements.rs");
include!("causal_laws_tests/limits_provenance_and_wire.rs");
