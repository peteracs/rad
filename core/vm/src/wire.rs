//! Compact value codec for the fork wire format (v2).
//!
//! The v1 codec built a `serde_json::Value` tree with a `{"t": ..., "v": ...}`
//! envelope per scalar — measured at 246 ms / 1.45 MB for a 10k-entity world.
//! v2 writes canonical JSON directly into a `String` (no intermediate tree)
//! and spends bytes only where type fidelity demands them:
//!
//! - `nil` → `null`, `bool` → `true`/`false`, `int` → bare integer
//! - `float` → bare number that always carries `.` or `e` (so the decoder
//!   can tell it from an int; integral floats print as `1.0`)
//! - `str` → JSON string, `list` → JSON array
//! - `entity` → `{"e":id}`, `tuple` → `{"t":[...]}`
//! - `map` → `{"m":[[[tag,key],value],...]}` (keys sorted — canonical)
//! - sum type → `{"s":[type,variant,{fields sorted}]}`
//! - component → `{"c":[type,[layout],[values]]}`
//!
//! Encoding is deterministic: the same value produces the same bytes on
//! every machine, which is what makes the wire digest and the
//! re-encode-is-byte-identical guarantee possible.

use crate::causality::{
    Cause, EmitRecord, ProposalRecord, ProvenanceTruncation, RelationAssertionRecord,
    ResolutionRecord, SettlementRecord, WireProvenance, WriteKind, WriteRecord,
};
use crate::value::{Allocator, MapKey, MapStorage, Value};
use std::fmt::Write;

// ---------------------------------------------------------------------------
// Provenance section: the sender's ledger closure rides the fork payload so
// the receiver can answer why() for state it never computed.
//
//   "prov":[writes,emits,settlements,proposals,resolutions,relation_assertions,truncation]
//   write: [frame, entity|null, name|null, component, value, kind, cause, origin|null,
//           resolution_id|null, fields]
//   emit:  [id, event, frame, payload, cause, origin|null]
//   cause: [0] main | [1, system] | [2, event, emit_id] |
//          [3, transaction, parent] ; kind: 0..=4
// ---------------------------------------------------------------------------

// Lexical sections preserve one private semantic namespace.
include!("wire/encode.rs");
include!("wire/decode.rs");
include!("wire/tests.rs");
