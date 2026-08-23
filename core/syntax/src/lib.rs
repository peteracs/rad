//! Canonical RAD source frontend.
//!
//! This crate owns the source grammar boundary: tokens, the AST, and parsing.
//! It deliberately has no checker, compiler, runtime, IO, or world dependency,
//! so changing runtime semantics cannot create a frontend dependency cycle.

pub mod ast;
pub mod lexer;
pub mod materialized_view;
pub mod native_types;
pub mod parser;
pub mod simulate_syntax;
pub mod source_bundle;
