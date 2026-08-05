use super::encoding;
use super::model::{
    DerivationError, DerivationLimits, DerivationResult, DerivationStats, ProofAlternative,
};
use crate::relation_runtime::FactKey;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct DerivationMeter {
    limits: DerivationLimits,
    facts: BTreeSet<FactKey>,
    proofs: BTreeSet<(FactKey, ProofAlternative)>,
    proofs_per_fact: BTreeMap<FactKey, usize>,
    capability_sets: BTreeMap<FactKey, BTreeSet<BTreeSet<String>>>,
    stats: DerivationStats,
}

impl DerivationMeter {
    pub(crate) fn new(limits: DerivationLimits) -> Self {
        Self {
            limits,
            facts: BTreeSet::new(),
            proofs: BTreeSet::new(),
            proofs_per_fact: BTreeMap::new(),
            capability_sets: BTreeMap::new(),
            stats: DerivationStats {
                canonical_bytes: b"rfc0003.derivation.v1".len() + 8,
                ..DerivationStats::default()
            },
        }
    }

    pub(crate) fn limits(&self) -> DerivationLimits {
        self.limits
    }

    pub(crate) fn stats(&self) -> DerivationStats {
        self.stats
    }

    fn increment(
        value: &mut usize,
        limit: usize,
        code: &'static str,
        amount: usize,
    ) -> DerivationResult<()> {
        *value = value
            .checked_add(amount)
            .ok_or_else(|| DerivationError::new(code, "counter overflow"))?;
        if *value > limit {
            return Err(DerivationError::new(code, format!("limit {limit}")));
        }
        Ok(())
    }

    pub(crate) fn row(&mut self) -> DerivationResult<()> {
        Self::increment(
            &mut self.stats.rows_scanned,
            self.limits.max_rows_scanned,
            "derivation.rows_scanned_limit",
            1,
        )
    }

    pub(crate) fn join(&mut self) -> DerivationResult<()> {
        Self::increment(
            &mut self.stats.join_attempts,
            self.limits.max_join_attempts,
            "derivation.join_attempt_limit",
            1,
        )
    }

    pub(crate) fn intermediate_bytes(&mut self, bytes: usize) -> DerivationResult<()> {
        Self::increment(
            &mut self.stats.intermediate_bytes,
            self.limits.max_intermediate_bytes,
            "derivation.intermediate_byte_limit",
            bytes,
        )
    }

    pub(crate) fn intermediate_state(&mut self, bytes: usize) -> DerivationResult<()> {
        Self::increment(
            &mut self.stats.intermediate_states,
            self.limits.max_intermediate_states,
            "derivation.intermediate_state_limit",
            1,
        )?;
        self.intermediate_bytes(bytes)
    }

    pub(crate) fn proof_combination(&mut self, bytes: usize) -> DerivationResult<()> {
        Self::increment(
            &mut self.stats.proof_combination_attempts,
            self.limits.max_proof_combination_attempts,
            "derivation.proof_combination_limit",
            1,
        )?;
        self.intermediate_bytes(bytes)
    }

    pub(crate) fn aggregate_entry(&mut self, bytes: usize) -> DerivationResult<()> {
        Self::increment(
            &mut self.stats.aggregate_group_entries,
            self.limits.max_aggregate_group_entries,
            "derivation.aggregate_group_limit",
            1,
        )?;
        self.intermediate_bytes(bytes)
    }

    pub(crate) fn retain(
        &mut self,
        result: &mut BTreeMap<FactKey, BTreeSet<ProofAlternative>>,
        fact: FactKey,
        proof: ProofAlternative,
    ) -> DerivationResult<()> {
        if self.proofs.contains(&(fact.clone(), proof.clone())) {
            return Ok(());
        }
        if !self.facts.contains(&fact) && self.facts.len() >= self.limits.max_facts {
            return Err(DerivationError::new(
                "derivation.fact_limit",
                format!("limit {}", self.limits.max_facts),
            ));
        }
        let fact_proofs = self.proofs_per_fact.get(&fact).copied().unwrap_or(0);
        if fact_proofs >= self.limits.max_proofs_per_fact {
            return Err(DerivationError::new(
                "derivation.proofs_per_fact_limit",
                format!("limit {}", self.limits.max_proofs_per_fact),
            ));
        }
        if self.proofs.len() >= self.limits.max_total_proofs {
            return Err(DerivationError::new(
                "derivation.total_proof_limit",
                format!("limit {}", self.limits.max_total_proofs),
            ));
        }
        let support_nodes = self
            .stats
            .support_nodes
            .checked_add(proof.supports.len())
            .ok_or_else(|| DerivationError::new("derivation.support_limit", "counter overflow"))?;
        if support_nodes > self.limits.max_support_nodes {
            return Err(DerivationError::new(
                "derivation.support_limit",
                format!("limit {}", self.limits.max_support_nodes),
            ));
        }
        if proof.depth > self.limits.max_proof_depth {
            return Err(DerivationError::new(
                "derivation.depth_limit",
                format!("limit {}", self.limits.max_proof_depth),
            ));
        }
        let mut capability_sets = self.capability_sets.get(&fact).cloned().unwrap_or_default();
        capability_sets.insert(proof.required_capabilities.clone());
        if capability_sets.len() > self.limits.max_capability_alternatives {
            return Err(DerivationError::new(
                "derivation.capability_alternative_limit",
                format!("limit {}", self.limits.max_capability_alternatives),
            ));
        }
        let mut additional = encoding::checked_add(8, proof.canonical_len()?)?;
        if !self.facts.contains(&fact) {
            additional = encoding::checked_add(additional, encoding::fact_len(&fact)?)?;
            additional = encoding::checked_add(additional, 8)?;
        }
        let canonical_bytes = encoding::checked_add(self.stats.canonical_bytes, additional)?;
        if canonical_bytes > self.limits.max_canonical_bytes {
            return Err(DerivationError::new(
                "derivation.canonical_byte_limit",
                format!("limit {}", self.limits.max_canonical_bytes),
            ));
        }

        self.facts.insert(fact.clone());
        self.proofs.insert((fact.clone(), proof.clone()));
        self.proofs_per_fact.insert(fact.clone(), fact_proofs + 1);
        self.capability_sets.insert(fact.clone(), capability_sets);
        self.stats.facts = self.facts.len();
        self.stats.proofs = self.proofs.len();
        self.stats.support_nodes = support_nodes;
        self.stats.canonical_bytes = canonical_bytes;
        result.entry(fact).or_default().insert(proof);
        Ok(())
    }
}
