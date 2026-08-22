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
pub use settlement::{ProposalRecord, RelationAssertionRecord, ResolutionRecord, SettlementRecord};
pub(crate) use settlement::{SettlementProposalInput, SettlementResolutionInput};

const INLINE_CAUSAL_TEXT_BYTES: usize = 23;
const CAUSAL_SUMMARY_BYTES: usize = 96;

/// Short provenance text stored directly inside its record. Component names,
/// field names, scalar summaries, and system names stay allocation-free on
/// frame paths; longer diagnostic text spills to one boxed string.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CausalText {
    Inline {
        len: u8,
        bytes: [u8; INLINE_CAUSAL_TEXT_BYTES],
    },
    Heap(Box<str>),
}

impl CausalText {
    pub fn new(value: &str) -> Self {
        if value.len() <= INLINE_CAUSAL_TEXT_BYTES {
            let mut bytes = [0; INLINE_CAUSAL_TEXT_BYTES];
            bytes[..value.len()].copy_from_slice(value.as_bytes());
            Self::Inline {
                len: value.len() as u8,
                bytes,
            }
        } else {
            Self::Heap(value.into())
        }
    }

    pub fn summary(value: impl std::fmt::Display) -> Self {
        struct BoundedWriter {
            bytes: [u8; CAUSAL_SUMMARY_BYTES],
            len: usize,
            truncated: bool,
        }

        impl std::fmt::Write for BoundedWriter {
            fn write_str(&mut self, value: &str) -> std::fmt::Result {
                let remaining = self.bytes.len().saturating_sub(self.len);
                if value.len() <= remaining {
                    self.bytes[self.len..self.len + value.len()].copy_from_slice(value.as_bytes());
                    self.len += value.len();
                    return Ok(());
                }
                let mut accepted = remaining;
                while !value.is_char_boundary(accepted) {
                    accepted -= 1;
                }
                self.bytes[self.len..self.len + accepted]
                    .copy_from_slice(&value.as_bytes()[..accepted]);
                self.len += accepted;
                self.truncated = true;
                Ok(())
            }
        }

        let mut writer = BoundedWriter {
            bytes: [0; CAUSAL_SUMMARY_BYTES],
            len: 0,
            truncated: false,
        };
        let _ = std::fmt::write(&mut writer, format_args!("{value}"));
        if writer.truncated {
            while writer.len > CAUSAL_SUMMARY_BYTES - '…'.len_utf8()
                || std::str::from_utf8(&writer.bytes[..writer.len]).is_err()
            {
                writer.len -= 1;
            }
            let ellipsis =
                '…'.encode_utf8(&mut writer.bytes[writer.len..writer.len + '…'.len_utf8()]);
            writer.len += ellipsis.len();
        }
        Self::new(
            std::str::from_utf8(&writer.bytes[..writer.len])
                .expect("causal summary writer preserves UTF-8"),
        )
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Inline { len, bytes } => std::str::from_utf8(&bytes[..usize::from(*len)])
                .expect("inline causal text is constructed from UTF-8"),
            Self::Heap(value) => value,
        }
    }
}

impl std::fmt::Debug for CausalText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(formatter)
    }
}

impl std::fmt::Display for CausalText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::ops::Deref for CausalText {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for CausalText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for CausalText {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for CausalText {
    fn from(value: String) -> Self {
        if value.len() <= INLINE_CAUSAL_TEXT_BYTES {
            Self::new(&value)
        } else {
            Self::Heap(value.into_boxed_str())
        }
    }
}

impl PartialEq<str> for CausalText {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for CausalText {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CausalScalar {
    Nil,
    Bool(bool),
    Int(i64),
    Float(u64),
    Entity(u32),
    Text(CausalText),
}

impl CausalScalar {
    pub fn from_value(value: &crate::value::Value) -> Self {
        if *value == crate::value::Value::NIL {
            Self::Nil
        } else if let Some(value) = value.as_bool() {
            Self::Bool(value)
        } else if let Some(value) = value.as_int() {
            Self::Int(value)
        } else if let Some(value) = value.as_float() {
            Self::Float(value.to_bits())
        } else if let Some(value) = value.as_entity_id() {
            Self::Entity(value)
        } else if let Some(value) = value.as_str() {
            Self::Text(value.into())
        } else {
            Self::Text(CausalText::summary(value))
        }
    }
}

impl std::fmt::Display for CausalScalar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nil => formatter.write_str("nil"),
            Self::Bool(value) => value.fmt(formatter),
            Self::Int(value) => value.fmt(formatter),
            Self::Float(bits) => f64::from_bits(*bits).fmt(formatter),
            Self::Entity(value) => write!(formatter, "entity({value})"),
            Self::Text(value) => value.fmt(formatter),
        }
    }
}

pub type FieldSummaries = smallvec::SmallVec<[(CausalText, CausalScalar); 1]>;

#[derive(Clone, Debug, PartialEq)]
pub enum WriteSummary {
    Full {
        value: CausalText,
        fields: FieldSummaries,
    },
    Field((CausalText, CausalScalar)),
}

impl WriteSummary {
    pub fn full(value: impl Into<CausalText>, fields: FieldSummaries) -> Self {
        Self::Full {
            value: value.into(),
            fields,
        }
    }

    pub fn text_fields(value: impl Into<CausalText>, fields: Vec<(String, String)>) -> Self {
        Self::full(
            value,
            fields
                .into_iter()
                .map(|(field, value)| (field.into(), CausalScalar::Text(value.into())))
                .collect(),
        )
    }

    pub fn fields(&self) -> &[(CausalText, CausalScalar)] {
        match self {
            Self::Full { fields, .. } => fields.as_slice(),
            Self::Field(field) => std::slice::from_ref(field),
        }
    }
}

impl std::fmt::Display for WriteSummary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full { value, .. } => value.fmt(formatter),
            Self::Field((field, value)) => write!(formatter, "{{ {field}: {value} }}"),
        }
    }
}

/// Who performed a write or an emit.
#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// Top-level program code (or any plain function called from it).
    Main,
    /// A system body (writebacks included).
    System { name: CausalText },
    /// An event handler; `emit_id` keys the exact [`EmitRecord`] of the
    /// event *instance* being handled — the link that makes chains causal
    /// rather than merely correlated.
    Handler { event: CausalText, emit_id: u64 },
    /// Every state write and explicit post-commit effect from one stable
    /// transaction shares this cause while retaining the caller chain.
    Transaction {
        name: CausalText,
        parent: Box<Cause>,
    },
    /// A successful call through the isolated native boundary. Digests bind
    /// the exact input/output bytes without retaining sensitive payloads.
    HostCall {
        extension: CausalText,
        generation: CausalText,
        plugin_digest: CausalText,
        export: CausalText,
        input_digest: CausalText,
        output_digest: CausalText,
        parent: Box<Cause>,
    },
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
    pub entity_name: Option<CausalText>,
    pub component: CausalText,
    /// Display summary of the written value (truncated).
    /// Compact display and field-level value. Scalar field writes retain
    /// exact bits and are formatted only when inspected or serialized.
    pub summary: WriteSummary,
    pub kind: WriteKind,
    pub by: Cause,
    /// `Some("wire <digest>")` when this record was ingested from another
    /// machine's ledger (it rode a fork payload). Frames inside such records
    /// follow the *sender's* clock; `why()` discloses the origin.
    pub origin: Option<String>,
    /// Fan-in resolution that produced this write, for RFC-0001 settlements.
    pub resolution_id: Option<u64>,
}

impl WriteRecord {
    pub fn local(
        frame: u64,
        entity: Option<u32>,
        entity_name: Option<String>,
        component: impl Into<CausalText>,
        summary: WriteSummary,
        kind: WriteKind,
        by: Cause,
    ) -> Self {
        Self {
            frame,
            entity,
            entity_name: entity_name.map(Into::into),
            component: component.into(),
            summary,
            kind,
            by,
            origin: None,
            resolution_id: None,
        }
    }

    pub fn with_resolution_id(mut self, resolution_id: Option<u64>) -> Self {
        self.resolution_id = resolution_id;
        self
    }

    pub fn fields(&self) -> &[(CausalText, CausalScalar)] {
        self.summary.fields()
    }

    pub fn value_string(&self) -> String {
        self.summary.to_string()
    }
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

/// Cryptographic commitment to provenance records omitted by a bounded
/// retention window. The count is exact; the digest is a domain-separated,
/// order-sensitive BLAKE3 chain over every evicted record.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProvenanceTruncation {
    pub evicted_records: u64,
    pub digest: [u8; 32],
}

impl ProvenanceTruncation {
    pub fn is_truncated(&self) -> bool {
        self.evicted_records != 0
    }

    pub fn digest_hex(&self) -> String {
        blake3::Hash::from_bytes(self.digest).to_hex().to_string()
    }
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
    pub relation_assertions: Vec<RelationAssertionRecord>,
    /// Sender-side records omitted before this closure was built. Receivers
    /// retain and re-commit this marker instead of presenting an incomplete
    /// chain as complete history.
    pub truncation: ProvenanceTruncation,
}

/// High-bit namespace tag for emit ids that came over the wire. Local ledger
/// ids are sequential and will never reach this range honestly.
pub const FOREIGN_EMIT_BIT: u64 = 1 << 63;
// Lexical sections preserve one private semantic namespace.
include!("causality/ledger.rs");
include!("causality/ledger_truncation.rs");
include!("causality/ledger_checkpoint.rs");
include!("causality/ledger_transport.rs");
include!("causality/ledger_explain.rs");
include!("causality/tests.rs");
