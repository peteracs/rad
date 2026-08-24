mod causal;
mod decl;
mod declaration_metadata;
mod emit;
mod escape;
mod expr;
mod expression_optimizer;
mod layout_analysis;
mod materialization;
mod pipeline;
mod stmt;
mod transaction;
mod view_kernel;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::ast::*;
use crate::checker::TypeError;
use crate::gc::GcHeap;
use crate::opcode::{Chunk, Op};
use crate::types::{
    AuthorityReport, CheckerOutput, ComponentType, Effect, EffectSet, EventType, ForIterKind,
    ResourceType, SumTypeDef,
};
use crate::value::{Builtin, FnValue, Value};

#[derive(Debug)]
pub struct CompileError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug)]
pub struct CompileWarning {
    pub message: String,
    pub line: u32,
    pub col: u32,
}
// Compiler state and lifecycle share one private implementation namespace.
include!("state.rs");
include!("lifecycle.rs");
