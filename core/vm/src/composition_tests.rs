//! Composition tests: the seven features as **one machine**.
//!
//! Each of the seven primitives (speculation, record/replay, blast-radius,
//! causality, migration, retro-edit, merge) is tested in its own module.
//! This suite tests the *seams between them* — the places where two features
//! meet and the easy implementation would quietly drop state. The founding
//! member is the in-flight-events-at-merge hole: `fork()` used to capture
//! the world but not the event queue, and `commit()` used to clear it, so
//! any pending events at a fork/commit/merge boundary silently vanished.
//! Events are program state; a snapshot that drops them is not a snapshot.

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::replay::TraceReplayer;
use crate::vm::VM;
// Lexical sections preserve one private semantic namespace.
include!("composition_tests/core_composition.rs");
include!("composition_tests/components_and_causality.rs");
include!("composition_tests/fork_delta_allocator.rs");
