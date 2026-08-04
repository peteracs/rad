//! Record & replay (list item #2), Phase 1: the trace recorder.
//!
//! Strategy: record *inputs*, not state. The interpreter is deterministic
//! (enforced by `determinism.rs`), so a log of every value that crosses the
//! determinism boundary — io builtin results, clock reads, the initial RNG
//! seed — is sufficient to reproduce an entire execution bit-for-bit.
//!
//! The effect system already enumerated the boundary for us: every impure
//! builtin is classified in `builtin_effect` (`builtins.rs`), so the recorder
//! interposes at a single chokepoint (`VM::call_builtin`) instead of tracing
//! syscalls. `rand_int` is NOT recorded: it is pure xorshift off the seed.
//! `print`/`eprint`/`log` are NOT recorded: they are deterministic outputs,
//! not inputs.
//!
//! Trace format (JSONL, one object per line):
//!
//! ```text
//! header  {"t":"header","version":1,"source":...,"source_hash":...,
//!          "features":[...],"feature_hash":...,"seed":...}
//! io      {"t":"io","f":<frame>,"s":<seq>,"b":<builtin>,"a":<args digest>,
//!          "r":<tagged result>}            (or "e":<error> when it failed)
//! frame   {"t":"frame","n":<frame just ended>,"fuel":<remaining, if metered>}
//! end     {"t":"end","world":<content digest>,"outcome":{"ok":true}}
//!         or `"outcome":{"error":<deterministic runtime error>}`
//! ```
//!
//! Traces are self-contained: the header embeds the full merged source, so
//! `rad replay trace.radr` needs nothing else on disk. `source_hash` is an
//! integrity check on the embedded source — a tampered trace is refused.
//!
//! The `a` digest exists for divergence detection: if a replayed run computes
//! different arguments for an io call than the recorded run did, replay halts
//! loudly instead of returning a result from a timeline that never happened.

use crate::gc::GcHeap;
use crate::source_bundle::{SourceLayout, SOURCE_LAYOUT_VERSION};
use crate::value::{Builtin, MapKey, MapStorage, Value};
use std::collections::HashMap;

pub const TRACE_VERSION: u64 = 1;
// Lexical sections preserve one private semantic namespace.
include!("replay/codec_and_identity.rs");
include!("replay/replayer.rs");
include!("replay/tests.rs");
