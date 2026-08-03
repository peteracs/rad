//! Typed, pointer-free contract for RFC-0002 candidate validation.
//!
//! Fixtures, hosts, WASM, attempt replay, and the VM share these bounded
//! semantic results instead of parsing compatibility error strings.

use crate::host_value::FrozenValue;
use crate::{CausalValueError, CausalValueLimits};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Write;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;

/// Version 3 makes `fuel_per_invocation` an every-opcode contract and
/// `max_heap_bytes_per_invocation` a disposable-heap contract.
pub const CONSTRAINT_LIMIT_PROFILE_VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstraintLimitProfile {
    value_limits: CausalValueLimits,
    fuel_per_invocation: u64,
    max_heap_bytes_per_invocation: usize,
    max_violations_per_invocation: usize,
    max_violations_per_settlement: usize,
    max_serialized_outcome_bytes: usize,
    max_aggregate_fuel: u64,
    max_aggregate_heap_bytes: usize,
}

impl ConstraintLimitProfile {
    pub const HARD_MAX_FUEL_PER_INVOCATION: u64 = 10_000_000;
    pub const HARD_MAX_HEAP_BYTES_PER_INVOCATION: usize = 64 * 1024 * 1024;
    pub const HARD_MAX_VIOLATIONS_PER_INVOCATION: usize = 4_096;
    pub const HARD_MAX_VIOLATIONS_PER_SETTLEMENT: usize = 65_536;
    pub const MIN_SERIALIZED_OUTCOME_BYTES: usize = 1_024;
    pub const HARD_MAX_SERIALIZED_OUTCOME_BYTES: usize = 16 * 1024 * 1024;
    pub const HARD_MAX_AGGREGATE_FUEL: u64 = 1_000_000_000;
    pub const HARD_MAX_AGGREGATE_HEAP_BYTES: usize = 1024 * 1024 * 1024;

    pub fn try_new(
        value_limits: CausalValueLimits,
        fuel_per_invocation: u64,
        max_heap_bytes_per_invocation: usize,
        max_violations_per_invocation: usize,
        max_violations_per_settlement: usize,
        max_serialized_outcome_bytes: usize,
    ) -> Result<Self, ConstraintProfileError> {
        value_limits
            .validate_profile()
            .map_err(ConstraintProfileError::ValueLimits)?;
        let profile = Self {
            value_limits,
            fuel_per_invocation,
            max_heap_bytes_per_invocation,
            max_violations_per_invocation,
            max_violations_per_settlement,
            max_serialized_outcome_bytes,
            max_aggregate_fuel: Self::HARD_MAX_AGGREGATE_FUEL,
            max_aggregate_heap_bytes: Self::HARD_MAX_AGGREGATE_HEAP_BYTES,
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn value_limits(&self) -> CausalValueLimits {
        self.value_limits
    }

    pub fn fuel_per_invocation(&self) -> u64 {
        self.fuel_per_invocation
    }

    pub fn max_heap_bytes_per_invocation(&self) -> usize {
        self.max_heap_bytes_per_invocation
    }

    pub fn max_violations_per_invocation(&self) -> usize {
        self.max_violations_per_invocation
    }

    pub fn max_violations_per_settlement(&self) -> usize {
        self.max_violations_per_settlement
    }

    pub fn max_serialized_outcome_bytes(&self) -> usize {
        self.max_serialized_outcome_bytes
    }

    pub fn max_aggregate_fuel(&self) -> u64 {
        self.max_aggregate_fuel
    }

    pub fn max_aggregate_heap_bytes(&self) -> usize {
        self.max_aggregate_heap_bytes
    }

    /// Keep proposal, candidate, and rejection values in one limit domain.
    pub fn with_value_limits(
        mut self,
        value_limits: CausalValueLimits,
    ) -> Result<Self, ConstraintProfileError> {
        value_limits
            .validate_profile()
            .map_err(ConstraintProfileError::ValueLimits)?;
        self.value_limits = value_limits;
        self.validate()?;
        Ok(self)
    }

    pub(crate) fn synchronize_value_limits(&mut self, value_limits: CausalValueLimits) {
        // `CausalValueLimits` can only be constructed through validated
        // builders. The remaining profile fields are unchanged, so replacing
        // this one value domain cannot invalidate the profile.
        self.value_limits = value_limits;
        debug_assert!(self.validate().is_ok());
    }

    /// Configure the process-safety envelope reserved before any selected
    /// constraint runs. Invocation meters remain independent inside it.
    pub fn with_aggregate_limits(
        mut self,
        max_aggregate_fuel: u64,
        max_aggregate_heap_bytes: usize,
    ) -> Result<Self, ConstraintProfileError> {
        self.max_aggregate_fuel = max_aggregate_fuel;
        self.max_aggregate_heap_bytes = max_aggregate_heap_bytes;
        self.validate()?;
        Ok(self)
    }

    pub fn version(&self) -> u32 {
        CONSTRAINT_LIMIT_PROFILE_VERSION
    }

    pub fn fingerprint(&self) -> String {
        let canonical = format!(
            "constraint-limits/v{};depth={};nodes={};value_bytes={};items={};fuel={};heap_bytes={};per_invocation={};per_settlement={};outcome_bytes={};aggregate_fuel={};aggregate_heap_bytes={}",
            self.version(),
            self.value_limits.max_depth(),
            self.value_limits.max_nodes(),
            self.value_limits.max_encoded_bytes(),
            self.value_limits.max_collection_items(),
            self.fuel_per_invocation,
            self.max_heap_bytes_per_invocation,
            self.max_violations_per_invocation,
            self.max_violations_per_settlement,
            self.max_serialized_outcome_bytes,
            self.max_aggregate_fuel,
            self.max_aggregate_heap_bytes,
        );
        hex::encode(Sha256::digest(canonical.as_bytes()))
    }

    fn validate(&self) -> Result<(), ConstraintProfileError> {
        validate_limit(
            "fuel_per_invocation",
            self.fuel_per_invocation,
            Self::HARD_MAX_FUEL_PER_INVOCATION,
        )?;
        validate_limit(
            "max_heap_bytes_per_invocation",
            self.max_heap_bytes_per_invocation as u64,
            Self::HARD_MAX_HEAP_BYTES_PER_INVOCATION as u64,
        )?;
        validate_limit(
            "max_violations_per_invocation",
            self.max_violations_per_invocation as u64,
            Self::HARD_MAX_VIOLATIONS_PER_INVOCATION as u64,
        )?;
        validate_limit(
            "max_violations_per_settlement",
            self.max_violations_per_settlement as u64,
            Self::HARD_MAX_VIOLATIONS_PER_SETTLEMENT as u64,
        )?;
        validate_limit(
            "max_serialized_outcome_bytes",
            self.max_serialized_outcome_bytes as u64,
            Self::HARD_MAX_SERIALIZED_OUTCOME_BYTES as u64,
        )?;
        validate_limit(
            "max_aggregate_fuel",
            self.max_aggregate_fuel,
            Self::HARD_MAX_AGGREGATE_FUEL,
        )?;
        validate_limit(
            "max_aggregate_heap_bytes",
            self.max_aggregate_heap_bytes as u64,
            Self::HARD_MAX_AGGREGATE_HEAP_BYTES as u64,
        )?;
        if self.max_aggregate_fuel < self.fuel_per_invocation {
            return Err(ConstraintProfileError::Inconsistent {
                message: "aggregate fuel is smaller than one invocation contract".into(),
            });
        }
        if self.max_aggregate_heap_bytes < self.max_heap_bytes_per_invocation {
            return Err(ConstraintProfileError::Inconsistent {
                message: "aggregate heap is smaller than one invocation contract".into(),
            });
        }
        if self.max_serialized_outcome_bytes < Self::MIN_SERIALIZED_OUTCOME_BYTES {
            return Err(ConstraintProfileError::Inconsistent {
                message: format!(
                    "max_serialized_outcome_bytes must be at least {} so the bounded failure envelope fits",
                    Self::MIN_SERIALIZED_OUTCOME_BYTES
                ),
            });
        }
        if self.max_violations_per_invocation > self.max_violations_per_settlement {
            return Err(ConstraintProfileError::Inconsistent {
                message: "per-invocation violation limit exceeds settlement limit".into(),
            });
        }
        Ok(())
    }
}

impl Default for ConstraintLimitProfile {
    fn default() -> Self {
        Self::try_new(
            CausalValueLimits::default(),
            100_000,
            1024 * 1024,
            256,
            4_096,
            1_048_576,
        )
        .expect("built-in constraint limit profile must be valid")
    }
}

fn validate_limit(
    field: &'static str,
    value: u64,
    hard_max: u64,
) -> Result<(), ConstraintProfileError> {
    if value == 0 || value > hard_max {
        Err(ConstraintProfileError::InvalidLimit {
            field,
            value,
            hard_max,
        })
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstraintProfileError {
    InvalidLimit {
        field: &'static str,
        value: u64,
        hard_max: u64,
    },
    Inconsistent {
        message: String,
    },
    ValueLimits(CausalValueError),
}

impl fmt::Display for ConstraintProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit {
                field,
                value,
                hard_max,
            } => write!(
                formatter,
                "constraint limit `{field}` is {value}; expected 1..={hard_max}"
            ),
            Self::Inconsistent { message } => {
                write!(formatter, "inconsistent constraint limits: {message}")
            }
            Self::ValueLimits(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ConstraintProfileError {}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CandidateKey {
    pub entity: u32,
    pub component: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CandidateView {
    base: BTreeMap<CandidateKey, FrozenValue>,
    patch: BTreeMap<CandidateKey, FrozenValue>,
}

impl CandidateView {
    pub fn new(
        base: BTreeMap<CandidateKey, FrozenValue>,
        patch: BTreeMap<CandidateKey, FrozenValue>,
    ) -> Self {
        Self { base, patch }
    }

    pub fn base(&self, key: &CandidateKey) -> Option<&FrozenValue> {
        self.base.get(key)
    }

    pub fn candidate(&self, key: &CandidateKey) -> Option<&FrozenValue> {
        self.patch.get(key).or_else(|| self.base.get(key))
    }

    pub fn staged_keys(&self) -> impl Iterator<Item = &CandidateKey> {
        self.patch.keys()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstraintIdentity {
    pub qualified_name: String,
    pub attached_component: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstraintViolation {
    pub constraint: ConstraintIdentity,
    pub subject: u32,
    pub code: String,
    pub occurrence: u32,
    pub source_line: u32,
    /// One shared entry in `SettlementRejection::candidate_details`.
    pub candidate: CandidateKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstraintEvaluationFailure {
    pub constraint: ConstraintIdentity,
    pub subject: u32,
    pub code: String,
    pub message: String,
    pub source_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstraintOutcome {
    Valid,
    Violations(Vec<ConstraintViolation>),
    EvaluationFailure(ConstraintEvaluationFailure),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RejectionValue {
    Visible(FrozenValue),
    Redacted,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RejectionProposalOrigin {
    Visible {
        law: String,
        source_line: u32,
        payload: FrozenValue,
    },
    /// Deliberately carries no hidden identity, size, location, or sort key.
    Redacted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateCausalExplanation {
    Visible {
        resolver: String,
        intent: String,
        intent_key: u32,
        proposal_origins: Vec<RejectionProposalOrigin>,
    },
    /// Structurally opaque: hidden origin identity never remains in a field
    /// that a serializer, debugger, or future renderer could accidentally
    /// reveal. Only bounded multiplicity is retained.
    Redacted { proposal_count: usize },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EphemeralCausalExplanation {
    pub candidates: BTreeMap<CandidateKey, CandidateCausalExplanation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectionCapabilityMetadata {
    pub profile_id: String,
    pub readable_components: BTreeSet<String>,
    pub origins_visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementRejection {
    pub settlement_id: u64,
    pub base_world_digest: String,
    pub applicable_constraints: Vec<ConstraintIdentity>,
    pub violations: Vec<ConstraintViolation>,
    pub evaluation_failures: Vec<ConstraintEvaluationFailure>,
    /// Each rejected candidate is frozen at most once regardless of how many
    /// requirements refer to it.
    pub candidate_details: BTreeMap<CandidateKey, RejectionValue>,
    pub explanation: EphemeralCausalExplanation,
    pub limit_profile_fingerprint: String,
    pub capabilities: RejectionCapabilityMetadata,
}

/// Version 6 binds the canonical compiled-program manifest and v3 checkpoint
/// encoding, including the exact global symbol-to-slot mapping.
pub const SETTLEMENT_ATTEMPT_RECORD_VERSION: u32 = 6;

/// Pointer-free recipe and expected result for replaying one rejected host
/// call. Ledger replay remains commit-only; this record belongs to a debugger
/// or test harness and never enters authoritative world provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedSettlementAttempt {
    pub version: u32,
    pub function: String,
    pub arguments: Vec<FrozenValue>,
    pub base_world_digest: String,
    pub program_digest: String,
    pub runtime_feature_fingerprint: String,
    pub constraint_registry_digest: String,
    pub limit_profile_fingerprint: String,
    pub capabilities: RejectionCapabilityMetadata,
    /// Canonical identity of the complete pre-attempt VM checkpoint. A
    /// portable recipe can only be replayed when the host supplies a state
    /// checkpoint with this identity.
    pub checkpoint_digest: String,
    pub rejection: Arc<SettlementRejection>,
}

/// In-process failed-attempt record. The public recipe remains pointer-free;
/// the private replay checkpoint owns a detached VM graph captured before the
/// attempted call began.
#[derive(Clone)]
pub struct RecordedFailedAttempt {
    recipe: Arc<FailedSettlementAttempt>,
    pub(crate) replay_checkpoint: Rc<crate::vm::attempt_replay::AttemptReplayCheckpoint>,
}

impl RecordedFailedAttempt {
    pub(crate) fn new(
        recipe: Arc<FailedSettlementAttempt>,
        replay_checkpoint: crate::vm::attempt_replay::AttemptReplayCheckpoint,
    ) -> Self {
        Self {
            recipe,
            replay_checkpoint: Rc::new(replay_checkpoint),
        }
    }

    /// Return the pointer-free recipe suitable for serialization or for
    /// replay against a separately supplied matching checkpoint.
    pub fn portable_recipe(&self) -> Arc<FailedSettlementAttempt> {
        Arc::clone(&self.recipe)
    }
}

impl Deref for RecordedFailedAttempt {
    type Target = FailedSettlementAttempt;

    fn deref(&self) -> &Self::Target {
        &self.recipe
    }
}

impl fmt::Debug for RecordedFailedAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecordedFailedAttempt")
            .field("recipe", &self.recipe)
            .field("checkpoint_digest", &self.recipe.checkpoint_digest)
            .finish_non_exhaustive()
    }
}

impl PartialEq for RecordedFailedAttempt {
    fn eq(&self, other: &Self) -> bool {
        self.recipe == other.recipe
    }
}

impl Eq for RecordedFailedAttempt {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettlementAttemptOutcome {
    Committed(FrozenValue),
    Rejected(RecordedFailedAttempt),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationResult {
    Accepted,
    Rejected(Arc<SettlementRejection>),
    HostAborted(HostFault),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectionEncodingError {
    Value(CausalValueError),
    OutcomeByteLimit { limit: usize },
    Serialization(String),
}

impl fmt::Display for RejectionEncodingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(error) => error.fmt(formatter),
            Self::OutcomeByteLimit { limit } => {
                write!(
                    formatter,
                    "canonical rejection exceeds the {limit}-byte limit"
                )
            }
            Self::Serialization(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RejectionEncodingError {}

struct BoundedCanonicalWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl BoundedCanonicalWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit.min(16 * 1024)),
            limit,
            exceeded: false,
        }
    }

    fn raw(&mut self, value: &[u8]) -> Result<(), RejectionEncodingError> {
        self.write_all(value).map_err(|error| {
            if self.exceeded {
                RejectionEncodingError::OutcomeByteLimit { limit: self.limit }
            } else {
                RejectionEncodingError::Serialization(error.to_string())
            }
        })
    }

    fn json<T: serde::Serialize>(&mut self, value: &T) -> Result<(), RejectionEncodingError> {
        serde_json::to_writer(&mut *self, value).map_err(|error| {
            if self.exceeded {
                RejectionEncodingError::OutcomeByteLimit { limit: self.limit }
            } else {
                RejectionEncodingError::Serialization(error.to_string())
            }
        })
    }

    fn finish(self) -> Result<Vec<u8>, RejectionEncodingError> {
        if self.exceeded {
            Err(RejectionEncodingError::OutcomeByteLimit { limit: self.limit })
        } else {
            Ok(self.bytes)
        }
    }
}

impl Write for BoundedCanonicalWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(std::io::Error::other("canonical rejection byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SettlementRejection {
    pub fn canonicalize(&mut self) {
        self.applicable_constraints.sort();
        self.applicable_constraints.dedup();
        self.violations.sort_by(|left, right| {
            (
                &left.constraint,
                left.subject,
                &left.code,
                left.occurrence,
                left.source_line,
            )
                .cmp(&(
                    &right.constraint,
                    right.subject,
                    &right.code,
                    right.occurrence,
                    right.source_line,
                ))
        });
        self.evaluation_failures.sort_by(|left, right| {
            (&left.constraint, left.subject, &left.code, left.source_line).cmp(&(
                &right.constraint,
                right.subject,
                &right.code,
                right.source_line,
            ))
        });
        for explanation in self.explanation.candidates.values_mut() {
            if let CandidateCausalExplanation::Visible {
                proposal_origins, ..
            } = explanation
            {
                proposal_origins.sort();
            }
        }
    }

    pub fn redacted_for(&self, capabilities: RejectionCapabilityMetadata) -> Self {
        let may_read = |component: &str| {
            capabilities.readable_components.contains("*")
                || capabilities.readable_components.contains(component)
        };
        let mut rendered = self.clone();
        for (candidate, value) in &mut rendered.candidate_details {
            if !may_read(&candidate.component) {
                *value = RejectionValue::Redacted;
            }
        }
        if !capabilities.origins_visible {
            for explanation in rendered.explanation.candidates.values_mut() {
                let proposal_count = match explanation {
                    CandidateCausalExplanation::Visible {
                        proposal_origins, ..
                    } => proposal_origins.len(),
                    CandidateCausalExplanation::Redacted { proposal_count } => *proposal_count,
                };
                *explanation = CandidateCausalExplanation::Redacted { proposal_count };
            }
        }
        rendered.capabilities = capabilities;
        rendered.canonicalize();
        rendered
    }

    /// Canonical semantic bytes used for deterministic output limits and
    /// attempt-replay fingerprints. Diagnostic source locations deliberately
    /// remain outside this representation: moving an otherwise identical
    /// declaration between files or lines cannot change the rejection.
    /// Visible RAD values embed their own canonical encoding; redaction uses
    /// one stable tagged placeholder.
    pub fn canonical_bytes(
        &self,
        profile: &ConstraintLimitProfile,
    ) -> Result<Vec<u8>, RejectionEncodingError> {
        fn identity(
            writer: &mut BoundedCanonicalWriter,
            value: &ConstraintIdentity,
        ) -> Result<(), RejectionEncodingError> {
            writer.raw(b"{\"attached_component\":")?;
            writer.json(&value.attached_component)?;
            writer.raw(b",\"qualified_name\":")?;
            writer.json(&value.qualified_name)?;
            writer.raw(b"}")
        }
        fn candidate_key(
            writer: &mut BoundedCanonicalWriter,
            value: &CandidateKey,
        ) -> Result<(), RejectionEncodingError> {
            writer.raw(b"{\"component\":")?;
            writer.json(&value.component)?;
            writer.raw(b",\"entity\":")?;
            writer.json(&value.entity)?;
            writer.raw(b"}")
        }
        fn frozen(
            writer: &mut BoundedCanonicalWriter,
            value: &FrozenValue,
            profile: &ConstraintLimitProfile,
        ) -> Result<(), RejectionEncodingError> {
            let limit = profile
                .value_limits()
                .max_encoded_bytes()
                .min(profile.max_serialized_outcome_bytes());
            let limits = profile
                .value_limits()
                .with_max_encoded_bytes(limit)
                .map_err(RejectionEncodingError::Value)?;
            let bytes = value
                .canonical_bytes(&limits)
                .map_err(RejectionEncodingError::Value)?;
            writer.raw(&bytes)
        }
        fn rejection_value(
            writer: &mut BoundedCanonicalWriter,
            value: &RejectionValue,
            profile: &ConstraintLimitProfile,
        ) -> Result<(), RejectionEncodingError> {
            match value {
                RejectionValue::Visible(value) => frozen(writer, value, profile),
                RejectionValue::Redacted => writer.raw(b"{\"redacted\":true}"),
            }
        }

        let mut writer = BoundedCanonicalWriter::new(profile.max_serialized_outcome_bytes());
        writer.raw(b"{\"applicable_constraints\":[")?;
        for (index, constraint) in self.applicable_constraints.iter().enumerate() {
            if index > 0 {
                writer.raw(b",")?;
            }
            identity(&mut writer, constraint)?;
        }
        writer.raw(b"],\"base_world_digest\":")?;
        writer.json(&self.base_world_digest)?;
        writer.raw(b",\"candidate_details\":[")?;
        for (index, (key, value)) in self.candidate_details.iter().enumerate() {
            if index > 0 {
                writer.raw(b",")?;
            }
            writer.raw(b"{\"key\":")?;
            candidate_key(&mut writer, key)?;
            writer.raw(b",\"value\":")?;
            rejection_value(&mut writer, value, profile)?;
            writer.raw(b"}")?;
        }
        writer.raw(b"],\"capabilities\":{\"origins_visible\":")?;
        writer.json(&self.capabilities.origins_visible)?;
        writer.raw(b",\"profile_id\":")?;
        writer.json(&self.capabilities.profile_id)?;
        writer.raw(b",\"readable_components\":[")?;
        for (index, component) in self.capabilities.readable_components.iter().enumerate() {
            if index > 0 {
                writer.raw(b",")?;
            }
            writer.json(component)?;
        }
        writer.raw(b"]},\"evaluation_failures\":[")?;
        for (index, failure) in self.evaluation_failures.iter().enumerate() {
            if index > 0 {
                writer.raw(b",")?;
            }
            writer.raw(b"{\"code\":")?;
            writer.json(&failure.code)?;
            writer.raw(b",\"constraint\":")?;
            identity(&mut writer, &failure.constraint)?;
            writer.raw(b",\"message\":")?;
            writer.json(&failure.message)?;
            writer.raw(b",\"subject\":")?;
            writer.json(&failure.subject)?;
            writer.raw(b"}")?;
        }
        writer.raw(b"],\"explanation\":{\"candidates\":[")?;
        for (index, (key, explanation)) in self.explanation.candidates.iter().enumerate() {
            if index > 0 {
                writer.raw(b",")?;
            }
            writer.raw(b"{\"key\":")?;
            candidate_key(&mut writer, key)?;
            match explanation {
                CandidateCausalExplanation::Visible {
                    resolver,
                    intent,
                    intent_key,
                    proposal_origins,
                } => {
                    writer.raw(b",\"origin\":{\"intent\":")?;
                    writer.json(intent)?;
                    writer.raw(b",\"intent_key\":")?;
                    writer.json(intent_key)?;
                    writer.raw(b",\"proposal_origins\":[")?;
                    for (origin_index, origin) in proposal_origins.iter().enumerate() {
                        if origin_index > 0 {
                            writer.raw(b",")?;
                        }
                        match origin {
                            RejectionProposalOrigin::Visible { law, payload, .. } => {
                                writer.raw(b"{\"law\":")?;
                                writer.json(law)?;
                                writer.raw(b",\"payload\":")?;
                                frozen(&mut writer, payload, profile)?;
                                writer.raw(b"}")?;
                            }
                            RejectionProposalOrigin::Redacted => {
                                writer.raw(b"{\"redacted_origin\":true}")?;
                            }
                        }
                    }
                    writer.raw(b"],\"resolver\":")?;
                    writer.json(resolver)?;
                    writer.raw(b"}")?;
                }
                CandidateCausalExplanation::Redacted { proposal_count } => {
                    writer.raw(b",\"origin\":{\"proposal_count\":")?;
                    writer.json(proposal_count)?;
                    writer.raw(b",\"redacted\":true}")?;
                }
            }
            writer.raw(b"}")?;
        }
        writer.raw(b"]},\"limit_profile_fingerprint\":")?;
        writer.json(&self.limit_profile_fingerprint)?;
        writer.raw(b",\"violations\":[")?;
        for (index, violation) in self.violations.iter().enumerate() {
            if index > 0 {
                writer.raw(b",")?;
            }
            writer.raw(b"{\"candidate\":")?;
            candidate_key(&mut writer, &violation.candidate)?;
            writer.raw(b",\"code\":")?;
            writer.json(&violation.code)?;
            writer.raw(b",\"constraint\":")?;
            identity(&mut writer, &violation.constraint)?;
            writer.raw(b",\"occurrence\":")?;
            writer.json(&violation.occurrence)?;
            writer.raw(b",\"subject\":")?;
            writer.json(&violation.subject)?;
            writer.raw(b"}")?;
        }
        writer.raw(b"]}")?;
        writer.finish()
    }

    pub fn render(&self) -> String {
        let mut output = format!(
            "Settlement rejected: {} violation(s), {} evaluation failure(s)",
            self.violations.len(),
            self.evaluation_failures.len()
        );
        for violation in &self.violations {
            output.push_str(&format!(
                "\n  - {}({}): {}",
                violation.constraint.qualified_name, violation.subject, violation.code
            ));
        }
        for failure in &self.evaluation_failures {
            output.push_str(&format!(
                "\n  - {}({}): {}: {}",
                failure.constraint.qualified_name, failure.subject, failure.code, failure.message
            ));
        }
        output.push_str("\nNo world state was changed.");
        output
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostFault {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmFailure {
    SettlementRejected(Arc<SettlementRejection>),
    Runtime(RuntimeError),
    Host(HostFault),
}

impl VmFailure {
    /// Preserve the pre-typed-API string contract for compatibility callers.
    /// Detailed callers retain the stable error code separately.
    pub fn render_compat(&self) -> String {
        match self {
            Self::SettlementRejected(rejection) => rejection.render(),
            Self::Runtime(error) => error.message.clone(),
            Self::Host(error) => error.message.clone(),
        }
    }
}

impl fmt::Display for VmFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SettlementRejected(rejection) => formatter.write_str(&rejection.render()),
            Self::Runtime(error) => write!(formatter, "{}: {}", error.code, error.message),
            Self::Host(error) => write!(formatter, "{}: {}", error.code, error.message),
        }
    }
}

impl std::error::Error for VmFailure {}

impl From<String> for VmFailure {
    fn from(message: String) -> Self {
        Self::Runtime(RuntimeError {
            code: "runtime.error".into(),
            message,
        })
    }
}

impl From<&str> for VmFailure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_fingerprint_is_deterministic_and_limits_are_validated() {
        let first = ConstraintLimitProfile::default();
        let second = ConstraintLimitProfile::default();
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(first.fingerprint().len(), 64);
        assert!(matches!(
            ConstraintLimitProfile::try_new(
                CausalValueLimits::default(),
                0,
                1024,
                256,
                4_096,
                1_048_576,
            ),
            Err(ConstraintProfileError::InvalidLimit {
                field: "fuel_per_invocation",
                ..
            })
        ));
    }

    #[test]
    fn candidate_view_reads_base_and_complete_patch_without_mutation() {
        let position = CandidateKey {
            entity: 7,
            component: "Position".into(),
        };
        let velocity = CandidateKey {
            entity: 7,
            component: "Velocity".into(),
        };
        let view = CandidateView::new(
            BTreeMap::from([
                (position.clone(), FrozenValue::Int(1)),
                (velocity.clone(), FrozenValue::Int(2)),
            ]),
            BTreeMap::from([(position.clone(), FrozenValue::Int(3))]),
        );
        assert_eq!(view.base(&position), Some(&FrozenValue::Int(1)));
        assert_eq!(view.candidate(&position), Some(&FrozenValue::Int(3)));
        assert_eq!(view.candidate(&velocity), Some(&FrozenValue::Int(2)));
    }

    #[test]
    fn canonical_rejection_encoding_is_fallible_under_a_narrow_output_profile() {
        let key = CandidateKey {
            entity: 7,
            component: "Data".into(),
        };
        let profile =
            ConstraintLimitProfile::try_new(CausalValueLimits::default(), 100, 1024, 16, 32, 1024)
                .unwrap();
        let rejection = SettlementRejection {
            settlement_id: 99,
            base_world_digest: "base".into(),
            applicable_constraints: vec![ConstraintIdentity {
                qualified_name: "Bounds".into(),
                attached_component: "Data".into(),
            }],
            violations: vec![ConstraintViolation {
                constraint: ConstraintIdentity {
                    qualified_name: "Bounds".into(),
                    attached_component: "Data".into(),
                },
                subject: 7,
                code: "data.too_large".into(),
                occurrence: 1,
                source_line: 1,
                candidate: key.clone(),
            }],
            evaluation_failures: Vec::new(),
            candidate_details: BTreeMap::from([(
                key,
                RejectionValue::Visible(FrozenValue::String("x".repeat(2048))),
            )]),
            explanation: EphemeralCausalExplanation::default(),
            limit_profile_fingerprint: profile.fingerprint(),
            capabilities: RejectionCapabilityMetadata {
                profile_id: "test".into(),
                readable_components: BTreeSet::from(["*".into()]),
                origins_visible: true,
            },
        };
        assert!(matches!(
            rejection.canonical_bytes(&profile),
            Err(RejectionEncodingError::Value(
                CausalValueError::EncodedByteLimit { .. }
            )) | Err(RejectionEncodingError::OutcomeByteLimit { .. })
        ));
    }

    #[test]
    fn opaque_settlement_ids_do_not_change_semantic_rejection_bytes() {
        let profile = ConstraintLimitProfile::default();
        let mut first = SettlementRejection {
            settlement_id: 1,
            base_world_digest: "base".into(),
            applicable_constraints: Vec::new(),
            violations: Vec::new(),
            evaluation_failures: vec![ConstraintEvaluationFailure {
                constraint: ConstraintIdentity {
                    qualified_name: "Bounds".into(),
                    attached_component: "Position".into(),
                },
                subject: 7,
                code: "constraint.evaluation_failed".into(),
                message: "failed".into(),
                source_line: 12,
            }],
            candidate_details: BTreeMap::new(),
            explanation: EphemeralCausalExplanation::default(),
            limit_profile_fingerprint: profile.fingerprint(),
            capabilities: RejectionCapabilityMetadata {
                profile_id: "trusted".into(),
                readable_components: BTreeSet::from(["*".into()]),
                origins_visible: true,
            },
        };
        let mut second = first.clone();
        second.settlement_id = 999;
        first.canonicalize();
        second.canonicalize();
        assert_eq!(
            first.canonical_bytes(&profile).unwrap(),
            second.canonical_bytes(&profile).unwrap()
        );
    }
}
