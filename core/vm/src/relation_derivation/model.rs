use crate::relation_runtime::{FactKey, FactValue};
use sha2::Digest as _;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivationError {
    pub code: &'static str,
    pub detail: String,
}

impl DerivationError {
    pub(crate) fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for DerivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for DerivationError {}

pub type DerivationResult<T> = Result<T, DerivationError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DerivationLimits {
    pub max_bindings: usize,
    pub max_facts: usize,
    pub max_proofs_per_fact: usize,
    pub max_total_proofs: usize,
    pub max_support_nodes: usize,
    pub max_proof_depth: usize,
    pub max_capability_alternatives: usize,
    pub max_canonical_bytes: usize,
    pub max_rows_scanned: usize,
    pub max_join_attempts: usize,
    pub max_intermediate_states: usize,
    pub max_intermediate_bytes: usize,
    pub max_proof_combination_attempts: usize,
    pub max_aggregate_group_entries: usize,
}

impl Default for DerivationLimits {
    fn default() -> Self {
        Self {
            max_bindings: 4_096,
            max_facts: 4_096,
            max_proofs_per_fact: 256,
            max_total_proofs: 16_384,
            max_support_nodes: 131_072,
            max_proof_depth: 64,
            max_capability_alternatives: 256,
            max_canonical_bytes: 16 * 1024 * 1024,
            max_rows_scanned: 1_000_000,
            max_join_attempts: 1_000_000,
            max_intermediate_states: 131_072,
            max_intermediate_bytes: 64 * 1024 * 1024,
            max_proof_combination_attempts: 131_072,
            max_aggregate_group_entries: 131_072,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DerivationStats {
    pub facts: usize,
    pub proofs: usize,
    pub support_nodes: usize,
    pub canonical_bytes: usize,
    pub rows_scanned: usize,
    pub join_attempts: usize,
    /// Actual candidate rows visited after indexing. Semantic join charging
    /// remains `join_attempts`, so optimized and reference paths adjudicate
    /// the same profile while this counter exposes physical speedup.
    pub physical_join_attempts: usize,
    pub intermediate_states: usize,
    pub intermediate_bytes: usize,
    pub proof_combination_attempts: usize,
    pub aggregate_group_entries: usize,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SupportRef {
    Authoritative {
        key: FactKey,
        assertion_id: u64,
        required_capabilities: BTreeSet<String>,
    },
    Derived {
        key: FactKey,
        proof_id: String,
        required_capabilities: BTreeSet<String>,
        proof_depth: usize,
    },
}

impl SupportRef {
    pub fn key(&self) -> &FactKey {
        match self {
            Self::Authoritative { key, .. } | Self::Derived { key, .. } => key,
        }
    }

    pub fn required_capabilities(&self) -> &BTreeSet<String> {
        match self {
            Self::Authoritative {
                required_capabilities,
                ..
            }
            | Self::Derived {
                required_capabilities,
                ..
            } => required_capabilities,
        }
    }

    pub(crate) fn depth(&self) -> usize {
        match self {
            Self::Authoritative { .. } => 1,
            Self::Derived { proof_depth, .. } => *proof_depth,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProofAlternative {
    pub rule: String,
    pub bindings: BTreeMap<String, FactValue>,
    pub supports: BTreeSet<SupportRef>,
    pub aggregate_group: Option<Vec<FactValue>>,
    pub required_capabilities: BTreeSet<String>,
    pub depth: usize,
}

impl ProofAlternative {
    pub fn identity(&self) -> String {
        hex::encode(sha2::Sha256::digest(self.canonical_bytes()))
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        super::encoding::proof_bytes(self)
    }

    pub(crate) fn canonical_len(&self) -> DerivationResult<usize> {
        super::encoding::proof_len(self)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DerivedRelationState {
    facts: Arc<BTreeMap<FactKey, BTreeSet<ProofAlternative>>>,
    stats: DerivationStats,
}

impl DerivedRelationState {
    pub fn facts(&self) -> &BTreeMap<FactKey, BTreeSet<ProofAlternative>> {
        &self.facts
    }

    pub fn proofs(&self, fact: &FactKey) -> Option<&BTreeSet<ProofAlternative>> {
        self.facts.get(fact)
    }

    pub fn stats(&self) -> DerivationStats {
        self.stats
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        super::encoding::derivation_bytes(&self.facts)
    }

    pub fn visible_facts(&self, capabilities: &BTreeSet<String>) -> Vec<FactKey> {
        self.facts
            .iter()
            .filter(|(_, proofs)| {
                proofs.iter().any(|proof| {
                    proof
                        .required_capabilities
                        .iter()
                        .all(|required| capabilities.contains(required))
                })
            })
            .map(|(fact, _)| fact.clone())
            .collect()
    }

    pub(crate) fn new(
        facts: BTreeMap<FactKey, BTreeSet<ProofAlternative>>,
        stats: DerivationStats,
    ) -> Self {
        Self {
            facts: Arc::new(facts),
            stats,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct LogicalRow {
    pub tuple: Vec<FactValue>,
    pub support: SupportRef,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BindingState {
    pub bindings: BTreeMap<String, FactValue>,
    pub supports: BTreeSet<SupportRef>,
}
