//! Syntax tree composition root.
//!
//! Source locations, module identity, declarations, statements, and
//! expressions are separate responsibilities but share one public AST
//! namespace so parser/checker/compiler consumers retain stable paths.

include!("ast/source.rs");
include!("ast/modules.rs");
include!("ast/declarations.rs");
include!("ast/statements.rs");
include!("ast/expressions.rs");
