#[derive(Clone, Debug)]
pub struct IntentRuntimeInfo {
    pub(crate) name: String,
    pub(crate) key_field: String,
    pub(crate) fields: Arc<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct ResolverRuntimeInfo {
    pub(crate) name: String,
    pub(crate) intent: String,
    pub(crate) global_slot: u16,
}

#[derive(Clone, Debug)]
pub struct ConstraintRuntimeInfo {
    pub(crate) name: String,
    pub(crate) attached_component: String,
    pub(crate) watches: Arc<Vec<String>>,
    pub(crate) global_slot: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct Proposal {
    pub(crate) id: u64,
    pub(crate) intent: String,
    pub(crate) key: u32,
    pub(crate) payload: Value,
    pub(crate) canonical: Vec<u8>,
    pub(crate) law: String,
    pub(crate) source_line: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct CandidateWrite {
    pub(crate) entity: u32,
    pub(crate) component: ComponentData,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolutionPatch {
    pub(crate) resolver: String,
    pub(crate) intent: String,
    pub(crate) key: u32,
    pub(crate) proposal_ids: Vec<u64>,
    pub(crate) writes: Vec<CandidateWrite>,
    pub(crate) relation_operations: Vec<crate::relation_runtime::PendingRelationOperation>,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveResolution {
    pub(crate) resolver: String,
    pub(crate) intent: String,
    pub(crate) key: u32,
    pub(crate) proposal_ids: Vec<u64>,
    pub(crate) writes: Vec<CandidateWrite>,
    pub(crate) relation_operations: Vec<crate::relation_runtime::PendingRelationOperation>,
}

fn relation_resolution_cause(
    resolver: &str,
    intent: &str,
    key: u32,
    proposal_ids: &[u64],
) -> String {
    let proposals = proposal_ids
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("resolution:{resolver}:{intent}:{key}:proposals={proposals}")
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveConstraint {
    pub(crate) identity: crate::constraint_types::ConstraintIdentity,
    pub(crate) subject: u32,
    violations: Vec<ActiveConstraintViolation>,
    occurrences: BTreeMap<String, u32>,
    retained_bytes: usize,
    max_retained_bytes: usize,
    pub(crate) overflowed: bool,
}

#[derive(Clone, Debug)]
struct ActiveConstraintViolation {
    code: String,
    occurrence: u32,
    source_line: u32,
}

impl ActiveConstraint {
    fn record_violation(&mut self, code: String, source_line: u32) -> Result<(), String> {
        const MAX_CODE_BYTES: usize = 128;
        if code.is_empty() || code.len() > MAX_CODE_BYTES {
            return Err(format!(
                "constraint violation code must contain 1..={MAX_CODE_BYTES} bytes"
            ));
        }
        let first_occurrence = !self.occurrences.contains_key(&code);
        let bytes = std::mem::size_of::<ActiveConstraintViolation>()
            .saturating_add(code.len())
            .saturating_add(if first_occurrence { code.len() } else { 0 });
        if bytes > self.max_retained_bytes.saturating_sub(self.retained_bytes) {
            self.overflowed = true;
            return Ok(());
        }
        let occurrence = self.occurrences.entry(code.clone()).or_insert(0);
        *occurrence = occurrence.saturating_add(1);
        self.retained_bytes = self.retained_bytes.saturating_add(bytes);
        self.violations.push(ActiveConstraintViolation {
            code,
            occurrence: *occurrence,
            source_line,
        });
        Ok(())
    }

    fn into_violations(self) -> Vec<crate::constraint_types::ConstraintViolation> {
        let candidate = crate::constraint_types::CandidateKey {
            entity: self.subject,
            component: self.identity.attached_component.clone(),
        };
        self.violations
            .into_iter()
            .map(|violation| crate::constraint_types::ConstraintViolation {
                constraint: self.identity.clone(),
                subject: self.subject,
                code: violation.code,
                occurrence: violation.occurrence,
                source_line: violation.source_line,
                candidate: candidate.clone(),
            })
            .collect()
    }
}

#[derive(Clone)]
pub(crate) struct SettlementContext {
    pub(crate) settlement_id: u64,
    pub(crate) owner_frame_id: u64,
    pub(crate) owner_chunk_id: usize,
    pub(crate) begin_ip: usize,
    pub(crate) base: WorldSnapshot,
    pub(crate) origin: crate::causality::Cause,
    pub(crate) proposals: Vec<Proposal>,
    pub(crate) patches: Vec<ResolutionPatch>,
    pub(crate) candidate: Option<WorldSnapshot>,
    pub(crate) relation_changes: Vec<crate::relation_runtime::FactChange>,
    pub(crate) active: Option<ActiveResolution>,
    pub(crate) active_constraint: Option<ActiveConstraint>,
    pub(crate) next_proposal_id: u64,
}
