use std::collections::HashMap;

use super::diagnostics::{
    ignored_immutable_transform_name, is_builtin, is_impure_builtin, is_readonly_builtin,
    suggest_did_you_mean, suggest_type_fix,
};
use super::is_cross_file;
use super::*;
use crate::ast::*;
use crate::simulate_syntax::{self, SystemsListForm};
use crate::types::*;
use crate::value::Builtin;
// Lexical sections preserve one private semantic namespace.
include!("typeck/diagnostics_and_declarations.rs");
include!("typeck/statements_and_assignments.rs");
include!("typeck/control_flow.rs");
include!("typeck/matches_and_expressions.rs");
include!("typeck/check_expr_operations.rs");
include!("typeck/check_expr_access_and_construction.rs");
include!("typeck/functions_and_operators.rs");
include!("typeck/calls_and_builtins.rs");
include!("typeck/collections_and_queries.rs");
include!("typeck/world_and_patterns.rs");
