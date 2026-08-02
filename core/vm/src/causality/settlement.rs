//! Fan-in provenance records and rendering support for RFC-0001.

use super::*;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct SettlementRecord {
    pub id: u64,
    pub frame: u64,
    pub by: Cause,
}

#[derive(Clone, Debug)]
pub struct ProposalRecord {
    pub id: u64,
    pub settlement_id: u64,
    pub intent: String,
    pub key: u32,
    pub payload: String,
    pub law: String,
    pub source_line: u32,
}

#[derive(Clone, Debug)]
pub struct ResolutionRecord {
    pub id: u64,
    pub settlement_id: u64,
    pub intent: String,
    pub key: u32,
    pub resolver: String,
    pub proposal_ids: Vec<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct SettlementProposalInput {
    pub runtime_id: u64,
    pub intent: String,
    pub key: u32,
    pub payload: String,
    pub law: String,
    pub source_line: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct SettlementResolutionInput {
    pub intent: String,
    pub key: u32,
    pub resolver: String,
    pub proposal_runtime_ids: Vec<u64>,
}

impl CausalityLedger {
    pub(crate) fn record_settlement(
        &mut self,
        frame: u64,
        by: Cause,
        proposals: &[SettlementProposalInput],
        resolutions: &[SettlementResolutionInput],
    ) -> Vec<u64> {
        let settlement_id = self.next_settlement_id;
        self.next_settlement_id += 1;
        self.settlements.push_back(SettlementRecord {
            id: settlement_id,
            frame,
            by,
        });

        let mut proposal_ids = HashMap::new();
        for proposal in proposals {
            let id = self.next_proposal_id;
            self.next_proposal_id += 1;
            proposal_ids.insert(proposal.runtime_id, id);
            self.proposals.push_back(ProposalRecord {
                id,
                settlement_id,
                intent: proposal.intent.clone(),
                key: proposal.key,
                payload: proposal.payload.clone(),
                law: proposal.law.clone(),
                source_line: proposal.source_line,
            });
        }

        let mut result = Vec::with_capacity(resolutions.len());
        for resolution in resolutions {
            let id = self.next_resolution_id;
            self.next_resolution_id += 1;
            result.push(id);
            self.resolutions.push_back(ResolutionRecord {
                id,
                settlement_id,
                intent: resolution.intent.clone(),
                key: resolution.key,
                resolver: resolution.resolver.clone(),
                proposal_ids: resolution
                    .proposal_runtime_ids
                    .iter()
                    .filter_map(|runtime_id| proposal_ids.get(runtime_id).copied())
                    .collect(),
            });
        }
        self.evict_overflow();
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_write_with_resolution(
        &mut self,
        frame: u64,
        entity: u32,
        entity_name: Option<String>,
        component: &str,
        value: String,
        by: Cause,
        resolution_id: Option<u64>,
    ) {
        self.writes.push_back(WriteRecord {
            frame,
            entity: Some(entity),
            entity_name,
            component: component.to_string(),
            value,
            kind: WriteKind::Set,
            by,
            origin: None,
            resolution_id,
        });
        self.evict_overflow();
    }

    pub(super) fn render_resolution(&self, resolution_id: u64) -> Option<String> {
        let resolution = self
            .resolutions
            .iter()
            .find(|resolution| resolution.id == resolution_id)?;
        self.settlements
            .iter()
            .find(|settlement| settlement.id == resolution.settlement_id)?;
        let key_label = self
            .writes
            .iter()
            .rev()
            .find(|write| {
                write.resolution_id == Some(resolution_id) && write.entity == Some(resolution.key)
            })
            .and_then(|write| write.entity_name.clone())
            .unwrap_or_else(|| format!("entity {}", resolution.key));
        let mut out = format!(
            "\n\n  <- resolver `{}`\n     intent: {}\n     key: {}",
            resolution.resolver, resolution.intent, key_label
        );
        const SHOWN_PROPOSALS: usize = 8;
        let mut shown = 0usize;
        for proposal_id in &resolution.proposal_ids {
            if shown == SHOWN_PROPOSALS {
                break;
            }
            let Some(proposal) = self
                .proposals
                .iter()
                .find(|proposal| proposal.id == *proposal_id)
            else {
                continue;
            };
            out.push_str(&format!(
                "\n\n     <- proposal {}\n        proposed by law `{}` at line {}",
                summarize(&proposal.payload),
                proposal.law,
                proposal.source_line
            ));
            shown += 1;
        }
        if resolution.proposal_ids.len() > shown {
            out.push_str(&format!(
                "\n\n     <- {} proposals shown, {} additional proposals omitted",
                shown,
                resolution.proposal_ids.len() - shown
            ));
        }
        Some(out)
    }
}
