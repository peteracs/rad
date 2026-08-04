mod causal;
mod declarations;
mod diagnostics;
mod reachability;
mod resolve;
mod scope;
mod typeck;

#[cfg(test)]
mod tests;
use crate::ast::*;
use crate::builtins;
use crate::simulate_syntax::{self, SystemsListForm};
use crate::types::*;
use crate::visitor::{walk_call_expr, walk_schedule_stmt, AstVisitor};
use std::collections::{HashMap, HashSet};
// Lexical sections preserve one private semantic namespace.
include!("mod/model.rs");
include!("mod/lifecycle.rs");
