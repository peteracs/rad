use std::fmt;

use crate::source_bundle::SourceLayout;

mod decl;
mod expr;
mod stmt;

pub use decl::reserved_keyword_rename_hints;
// Lexical sections preserve one private semantic namespace.
include!("lexer_sections/tokens_and_lexer.rs");
include!("lexer_sections/tests.rs");
