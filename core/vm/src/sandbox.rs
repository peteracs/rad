//! Capability model for speculative sandboxed execution (Feature #1).
//!
//! A `SandboxCaps` bounds what untrusted code (AI-generated plans, mods,
//! plugins) may do while running inside a forked world. It is the runtime
//! half of the three-layer enforcement model:
//!
//! 1. **static** — the checker already bans IO/event systems in `simulate`
//!    schedules; sandbox callers additionally run under a denied-builtin mask.
//! 2. **runtime ACL** — `bi_set` / `bi_spawn` / `bi_despawn` consult the
//!    component allowlist on every write when `sandbox_caps` is set.
//! 3. **physics** — `fuel` and `mem_limit` (see `VM::charge_fuel`) ensure that
//!    anything surviving layers 1 and 2 still dies of resource starvation.
//!
//! `commit` is deliberately not grantable: the host commits winners, untrusted
//! code never touches the live world directly.

use std::collections::HashSet;

use crate::types::Effect;
use crate::value::Builtin;

/// Default fuel grant: 10M charge points (loop iterations + calls).
pub const DEFAULT_FUEL: u64 = 10_000_000;
/// Default allocation ceiling: 64 MiB.
pub const DEFAULT_MEM_BYTES: usize = 64 * 1024 * 1024;
/// Default RNG seed for sandboxed runs (deterministic by default).
pub const DEFAULT_SEED: u64 = 1;
// Lexical sections preserve one private semantic namespace.
include!("sandbox/capabilities.rs");
include!("sandbox/isolation_tests.rs");
include!("sandbox/simulation_tests.rs");
