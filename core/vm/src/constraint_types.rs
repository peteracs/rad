//! Typed, pointer-free contract for RFC-0002 candidate validation.
//!
//! Fixtures, hosts, WASM, attempt replay, and the VM share these bounded
//! semantic results instead of parsing compatibility error strings.

use crate::host_value::FrozenValue;
use crate::{CausalValueError, CausalValueLimits};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

pub const CONSTRAINT_LIMIT_PROFILE_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstraintLimitProfile {
    value_limits: CausalValueLimits,
    fuel_per_invocation: u64,
    max_heap_bytes_per_invocation: usize,
    max_violations_per_invocation: usize,
    max_violations_per_settlement: usize,
    max_serialized_outcome_bytes: usize,
}

impl ConstraintLimitProfile {
    pub const HARD_MAX_FUEL_PER_INVOCATION: u64 = 10_000_000;
    pub const HARD_MAX_HEAP_BYTES_PER_INVOCATION: usize = 64 * 1024 * 1024;
    pub const HARD_MAX_VIOLATIONS_PER_INVOCATION: usize = 4_096;
    pub const HARD_MAX_VIOLATIONS_PER_SETTLEMENT: usize = 65_536;
    pub const MIN_SERIALIZED_OUTCOME_BYTES: usize = 1_024;
    pub const HARD_MAX_SERIALIZED_OUTCOME_BYTES: usize = 16 * 1024 * 1024;

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

    pub fn version(&self) -> u32 {
        CONSTRAINT_LIMIT_PROFILE_VERSION
    }

    pub fn fingerprint(&self) -> String {
        let canonical = format!(
            "constraint-limits/v{};depth={};nodes={};value_bytes={};items={};fuel={};heap_bytes={};per_invocation={};per_settlement={};outcome_bytes={}",
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
    pub details: BTreeMap<String, RejectionValue>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectionProposalOrigin {
    pub law: String,
    pub source_line: u32,
    pub payload: RejectionValue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EphemeralCausalExplanation {
    pub proposal_origins: Vec<RejectionProposalOrigin>,
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
    pub explanation: EphemeralCausalExplanation,
    pub limit_profile_fingerprint: String,
    pub capabilities: RejectionCapabilityMetadata,
}

pub const SETTLEMENT_ATTEMPT_RECORD_VERSION: u32 = 1;

/// Pointer-free recipe and expected result for replaying one rejected host
/// call. Ledger replay remains commit-only; this record belongs to a debugger
/// or test harness and never enters authoritative world provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedSettlementAttempt {
    pub version: u32,
    pub function: String,
    pub arguments: Vec<FrozenValue>,
    pub base_world_digest: String,
    pub limit_profile_fingerprint: String,
    pub capabilities: RejectionCapabilityMetadata,
    pub rejection: Arc<SettlementRejection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettlementAttemptOutcome {
    Committed(FrozenValue),
    Rejected(Arc<FailedSettlementAttempt>),
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
        self.explanation.proposal_origins.sort_by(|left, right| {
            (&left.law, &left.payload, left.source_line).cmp(&(
                &right.law,
                &right.payload,
                right.source_line,
            ))
        });
    }

    pub fn redacted_for(&self, capabilities: RejectionCapabilityMetadata) -> Self {
        let may_read = |component: &str| {
            capabilities.readable_components.contains("*")
                || capabilities.readable_components.contains(component)
        };
        let mut rendered = self.clone();
        for violation in &mut rendered.violations {
            if !may_read(&violation.constraint.attached_component) {
                for value in violation.details.values_mut() {
                    *value = RejectionValue::Redacted;
                }
            }
        }
        if !capabilities.origins_visible {
            for origin in &mut rendered.explanation.proposal_origins {
                origin.payload = RejectionValue::Redacted;
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
    pub fn canonical_bytes(&self, profile: &ConstraintLimitProfile) -> Vec<u8> {
        fn rejection_value(
            value: &RejectionValue,
            limits: &CausalValueLimits,
        ) -> serde_json::Value {
            match value {
                RejectionValue::Visible(value) => {
                    let bytes = value
                        .canonical_bytes(limits)
                        .expect("accepted rejection values are already causally bounded");
                    serde_json::from_slice(&bytes)
                        .expect("FrozenValue canonical encoding is valid JSON")
                }
                RejectionValue::Redacted => serde_json::json!({"redacted": true}),
            }
        }
        let constraints = self
            .applicable_constraints
            .iter()
            .map(|identity| {
                serde_json::json!({
                    "attached_component": identity.attached_component,
                    "qualified_name": identity.qualified_name,
                })
            })
            .collect::<Vec<_>>();
        let violations = self
            .violations
            .iter()
            .map(|violation| {
                let details = violation
                    .details
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            rejection_value(value, &profile.value_limits()),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>();
                serde_json::json!({
                    "code": violation.code,
                    "constraint": {
                        "attached_component": violation.constraint.attached_component,
                        "qualified_name": violation.constraint.qualified_name,
                    },
                    "details": details,
                    "occurrence": violation.occurrence,
                    "subject": violation.subject,
                })
            })
            .collect::<Vec<_>>();
        let failures = self
            .evaluation_failures
            .iter()
            .map(|failure| {
                serde_json::json!({
                    "code": failure.code,
                    "constraint": {
                        "attached_component": failure.constraint.attached_component,
                        "qualified_name": failure.constraint.qualified_name,
                    },
                    "message": failure.message,
                    "subject": failure.subject,
                })
            })
            .collect::<Vec<_>>();
        let origins = self
            .explanation
            .proposal_origins
            .iter()
            .map(|origin| {
                serde_json::json!({
                    "law": origin.law,
                    "payload": rejection_value(&origin.payload, &profile.value_limits()),
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_vec(&serde_json::json!({
            "applicable_constraints": constraints,
            "base_world_digest": self.base_world_digest,
            "capabilities": {
                "origins_visible": self.capabilities.origins_visible,
                "profile_id": self.capabilities.profile_id,
                "readable_components": self.capabilities.readable_components,
            },
            "evaluation_failures": failures,
            "explanation": { "proposal_origins": origins },
            "limit_profile_fingerprint": self.limit_profile_fingerprint,
            "settlement_id": self.settlement_id,
            "violations": violations,
        }))
        .expect("pointer-free rejection contract must serialize")
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
}
