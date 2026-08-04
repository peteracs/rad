use super::*;
use crate::gc::GcHeap;
use crate::value::{Builtin, PipelineOp};

pub enum VectorizableBody<'a> {
    Expr(&'a Expr),
    IfElse {
        cond: &'a Expr,
        then_expr: &'a Expr,
        else_expr: &'a Expr,
    },
}
// Lexical sections preserve one private semantic namespace.
include!("expr/constant_folding.rs");
include!("expr/expression_lowering.rs");
include!("expr/queries_and_vectors.rs");
