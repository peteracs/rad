//! Causality queries (list item #4): "why does this value exist?"
//!
//! The VM keeps a provenance ledger of every main-timeline world write and
//! every event emission. Writes record *who* performed them (top-level code,
//! a system, or an event handler), and handler causes link to the exact emit
//! record of the event instance they were handling — which itself records
//! who emitted it. `why(entity, Component)` walks that chain:
//!
//! ```text
//! Gold of hero = { amount: 0 }   (set in frame 3)
//!   <- by `on Hit` handler (frame 3)
//!   <- Hit { amount: 10 } emitted in frame 2
//!   <- by top-level code
//! ```
//!
//! Scope: the ledger tracks the main timeline only. Writes inside
//! `simulate()` forks and sandbox guests are speculative — they never need
//! explaining because they never become "this value".
//!
//! Frames follow the record/replay convention: writes before the first
//! `flush_events` are frame 0, handlers dispatched by the k-th flush write
//! in frame k. This makes the ledger composable with the time-travel
//! server: "why, as of timeline index k" = writes with `frame < k`.

mod settlement;
pub use settlement::{ProposalRecord, ResolutionRecord, SettlementRecord};
pub(crate) use settlement::{SettlementProposalInput, SettlementResolutionInput};

/// Who performed a write or an emit.
#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// Top-level program code (or any plain function called from it).
    Main,
    /// A system body (writebacks included).
    System { name: String },
    /// An event handler; `emit_id` keys the exact [`EmitRecord`] of the
    /// event *instance* being handled — the link that makes chains causal
    /// rather than merely correlated.
    Handler { event: String, emit_id: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WriteKind {
    Set,
    Spawn,
    /// Despawn is recorded once per entity with component `"*"`; queries on
    /// any component of that entity match it.
    Despawn,
    Remove,
    Resource,
}

#[derive(Clone, Debug)]
pub struct WriteRecord {
    pub frame: u64,
    /// `None` for resource writes.
    pub entity: Option<u32>,
    /// Entity name at write time, when it had one.
    pub entity_name: Option<String>,
    pub component: String,
    /// Display summary of the written value (truncated).
    pub value: String,
    pub kind: WriteKind,
    pub by: Cause,
    /// `Some("wire <digest>")` when this record was ingested from another
    /// machine's ledger (it rode a fork payload). Frames inside such records
    /// follow the *sender's* clock; `why()` discloses the origin.
    pub origin: Option<String>,
    /// Fan-in resolution that produced this write, for RFC-0001 settlements.
    pub resolution_id: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct EmitRecord {
    pub id: u64,
    pub event: String,
    pub frame: u64,
    /// Display summary of the payload (truncated).
    pub payload: String,
    pub by: Cause,
    /// See [`WriteRecord::origin`].
    pub origin: Option<String>,
}

/// The provenance closure that rides a fork payload: for every value alive
/// in the fork, the last write that produced it, plus the transitive emit
/// chain those writes hang off (`write -> emit -> emitter's write -> …`),
/// plus the emit records of in-flight events. This is what lets the
/// *receiving* machine answer `why()` for state it never computed.
///
/// Emit ids inside are namespaced with [`FOREIGN_EMIT_BIT`] so they can
/// never collide with the receiver's own ledger ids; `commit()` remaps them
/// into fresh local ids at ingest time.
#[derive(Clone, Debug, Default)]
pub struct WireProvenance {
    /// Short origin label, set at decode time from the payload digest.
    pub origin: String,
    pub writes: Vec<WriteRecord>,
    pub emits: Vec<EmitRecord>,
    pub settlements: Vec<SettlementRecord>,
    pub proposals: Vec<ProposalRecord>,
    pub resolutions: Vec<ResolutionRecord>,
}

/// High-bit namespace tag for emit ids that came over the wire. Local ledger
/// ids are sequential and will never reach this range honestly.
pub const FOREIGN_EMIT_BIT: u64 = 1 << 63;
// Lexical sections preserve one private semantic namespace.
include!("causality_sections/ledger.rs");
include!("causality_sections/tests.rs");
