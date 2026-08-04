use super::*;
use crate::lexer::Lexer;
// Lexical sections preserve one private semantic namespace.
include!("tests/core_syntax.rs");
include!("tests/strings_and_type_regressions.rs");
