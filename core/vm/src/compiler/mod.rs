mod causal;
mod decl;
mod egraph;
mod emit;
mod escape;
mod expr;
mod layout_analysis;
mod materialization;
mod pipeline;
mod stmt;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::ast::*;
use crate::gc::GcHeap;
use crate::opcode::{Chunk, Op};
use crate::types::{
    CheckerOutput, ComponentType, Effect, EffectSet, ForIterKind, ResourceType, SumTypeDef,
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
// Lexical sections preserve one private semantic namespace.
include!("mod/model.rs");
include!("mod/lifecycle.rs");
