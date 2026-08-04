use super::*;
// Lexical sections preserve one private semantic namespace.
include!("decl/declaration_dispatch.rs");
include!("decl/data_declarations.rs");
include!("decl/callable_declarations.rs");
