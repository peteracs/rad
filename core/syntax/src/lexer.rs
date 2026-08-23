use std::fmt;

use crate::source_bundle::SourceLayout;

mod decl;
mod expr;
mod stmt;

pub use decl::reserved_keyword_rename_hints;
// The token engine shares the lexer's private cursor and diagnostic state.
include!("lexer/engine.rs");
include!("lexer/tests.rs");
