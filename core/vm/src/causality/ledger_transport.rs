// Moving provenance between machines: the closure a fork payload carries,
// and ingesting a sender's ledger without losing its origin.

impl CausalityLedger {

    /// Compute the [`WireProvenance`] closure for a fork being encoded:
    /// the newest write per `(entity-or-resource, component)` among records
    /// accepted by `keep` (callers pass liveness in the fork), plus the
    /// transitive emit chain, plus emit records for the fork's in-flight
    /// queue (`queue_ids`, native ids). All emit ids in the result are
    /// rewritten into the foreign namespace, ready for the wire.
    pub fn provenance_closure(
        &self,
        keep: impl Fn(&WriteRecord) -> bool,
        keep_relation: impl Fn(&RelationAssertionRecord) -> bool,
        queue_ids: &[u64],
    ) -> WireProvenance {
        use std::collections::{HashMap, HashSet};
        fn cause_emit_id(cause: &Cause) -> Option<u64> {
            match cause {
                Cause::Handler { emit_id, .. } => Some(*emit_id),
                Cause::Transaction { parent, .. } | Cause::HostCall { parent, .. } => {
                    cause_emit_id(parent)
                }
                Cause::Main | Cause::System { .. } => None,
            }
        }
        let mut newest: HashMap<(Option<u32>, &str), &WriteRecord> = HashMap::new();
        for w in self.writes.iter().rev() {
            if !keep(w) {
                continue;
            }
            newest.entry((w.entity, w.component.as_str())).or_insert(w);
        }

        // Transitive emit closure: handler-caused writes pull in the emit
        // they were handling; emits caused by other handlers chain further.
        let mut wanted: Vec<u64> = queue_ids.iter().copied().filter(|&id| id != 0).collect();
        for w in newest.values() {
            if let Some(emit_id) = cause_emit_id(&w.by) {
                wanted.push(emit_id);
            }
        }

        // Settlement writes carry a fan-in tree. Keep only the resolution
        // records reachable from the live writes in this fork, along with
        // their proposals and owning settlement records.
        let relation_assertions: Vec<RelationAssertionRecord> = self
            .relation_assertions
            .iter()
            .filter(|record| keep_relation(record))
            .cloned()
            .collect();
        let mut wanted_resolution_ids: HashSet<u64> = newest
            .values()
            .filter_map(|write| write.resolution_id)
            .collect();
        wanted_resolution_ids.extend(
            relation_assertions
                .iter()
                .flat_map(|record| record.resolution_ids.iter().copied()),
        );
        let resolutions: Vec<ResolutionRecord> = self
            .resolutions
            .iter()
            .filter(|resolution| wanted_resolution_ids.contains(&resolution.id))
            .cloned()
            .collect();
        let wanted_proposal_ids: HashSet<u64> = resolutions
            .iter()
            .flat_map(|resolution| resolution.proposal_ids.iter().copied())
            .collect();
        let proposals: Vec<ProposalRecord> = self
            .proposals
            .iter()
            .filter(|proposal| wanted_proposal_ids.contains(&proposal.id))
            .cloned()
            .collect();
        let wanted_settlement_ids: HashSet<u64> = resolutions
            .iter()
            .map(|resolution| resolution.settlement_id)
            .collect();
        let settlements: Vec<SettlementRecord> = self
            .settlements
            .iter()
            .filter(|settlement| wanted_settlement_ids.contains(&settlement.id))
            .cloned()
            .collect();
        for settlement in &settlements {
            if let Some(emit_id) = cause_emit_id(&settlement.by) {
                wanted.push(emit_id);
            }
        }
        let mut seen: HashSet<u64> = HashSet::new();
        let mut emits: Vec<&EmitRecord> = Vec::new();
        while let Some(id) = wanted.pop() {
            if !seen.insert(id) {
                continue;
            }
            // Already-foreign ids (multi-hop forks) have no local record.
            if id & FOREIGN_EMIT_BIT != 0 {
                continue;
            }
            if let Some(e) = self.emit_by_id(id) {
                emits.push(e);
                if let Some(emit_id) = cause_emit_id(&e.by) {
                    wanted.push(emit_id);
                }
            }
        }
        emits.sort_by_key(|e| e.id);

        fn tag_cause(cause: &Cause) -> Cause {
            match cause {
                Cause::Handler { event, emit_id } => Cause::Handler {
                    event: event.clone(),
                    emit_id: foreign_emit_id(*emit_id),
                },
                Cause::Transaction { name, parent } => Cause::Transaction {
                    name: name.clone(),
                    parent: Box::new(tag_cause(parent)),
                },
                Cause::HostCall {
                    extension,
                    generation,
                    plugin_digest,
                    export,
                    input_digest,
                    output_digest,
                    parent,
                } => Cause::HostCall {
                    extension: extension.clone(),
                    generation: generation.clone(),
                    plugin_digest: plugin_digest.clone(),
                    export: export.clone(),
                    input_digest: input_digest.clone(),
                    output_digest: output_digest.clone(),
                    parent: Box::new(tag_cause(parent)),
                },
                other => other.clone(),
            }
        }
        let mut writes: Vec<WriteRecord> = newest
            .into_values()
            .map(|w| {
                let mut w = w.clone();
                w.by = tag_cause(&w.by);
                w
            })
            .collect();
        // Deterministic wire order: resources first (entity None), then by
        // entity id, then component name.
        writes.sort_by(|a, b| (a.entity, &a.component).cmp(&(b.entity, &b.component)));
        let emits = emits
            .into_iter()
            .map(|e| {
                let mut e = e.clone();
                e.id = foreign_emit_id(e.id);
                e.by = tag_cause(&e.by);
                e
            })
            .collect();
        let settlements = settlements
            .into_iter()
            .map(|mut settlement| {
                settlement.by = tag_cause(&settlement.by);
                settlement
            })
            .collect();
        WireProvenance {
            origin: String::new(),
            writes,
            emits,
            settlements,
            proposals,
            resolutions,
            relation_assertions,
            truncation: self.truncation.clone(),
        }
    }

    /// Ingest a foreign provenance closure (the other half of
    /// [`Self::provenance_closure`]): every record lands in this ledger
    /// marked with `prov.origin`, foreign emit ids are remapped to fresh
    /// local ids, and entity ids are rewritten through `entity_remap`
    /// (merge may have remapped colliding spawns). Returns the emit id map
    /// so the caller can rewrite the in-flight queue's ids too.
    pub fn ingest(
        &mut self,
        prov: &WireProvenance,
        entity_remap: &std::collections::HashMap<u32, u32>,
    ) -> std::collections::HashMap<u64, u64> {
        fn remap_cause(cause: &Cause, id_map: &std::collections::HashMap<u64, u64>) -> Cause {
            match cause {
                Cause::Handler { event, emit_id } => Cause::Handler {
                    event: event.clone(),
                    emit_id: id_map.get(emit_id).copied().unwrap_or(*emit_id),
                },
                Cause::Transaction { name, parent } => Cause::Transaction {
                    name: name.clone(),
                    parent: Box::new(remap_cause(parent, id_map)),
                },
                Cause::HostCall {
                    extension,
                    generation,
                    plugin_digest,
                    export,
                    input_digest,
                    output_digest,
                    parent,
                } => Cause::HostCall {
                    extension: extension.clone(),
                    generation: generation.clone(),
                    plugin_digest: plugin_digest.clone(),
                    export: export.clone(),
                    input_digest: input_digest.clone(),
                    output_digest: output_digest.clone(),
                    parent: Box::new(remap_cause(parent, id_map)),
                },
                other => other.clone(),
            }
        }
        let mut id_map: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        let origin = if prov.origin.is_empty() {
            "wire".to_string()
        } else {
            prov.origin.clone()
        };
        self.absorb_remote_truncation(&prov.truncation, &origin);
        // Sender order = ascending ids; handler links always point backwards,
        // so the map is complete by the time a reference is rewritten.
        for e in &prov.emits {
            let by = remap_cause(&e.by, &id_map);
            let new_id = (self.emit_base + self.emits.len()) as u64 + 1;
            self.emits.push_back(EmitRecord {
                id: new_id,
                event: e.event.clone(),
                frame: e.frame,
                payload: e.payload.clone(),
                by,
                origin: Some(e.origin.clone().unwrap_or_else(|| origin.clone())),
            });
            id_map.insert(e.id, new_id);
            self.evict_overflow();
        }
        let mut settlement_id_map = std::collections::HashMap::new();
        for settlement in &prov.settlements {
            let id = self.next_settlement_id;
            self.next_settlement_id += 1;
            let by = remap_cause(&settlement.by, &id_map);
            self.settlements.push_back(SettlementRecord {
                id,
                frame: settlement.frame,
                by,
            });
            settlement_id_map.insert(settlement.id, id);
        }
        let mut proposal_id_map = std::collections::HashMap::new();
        for proposal in &prov.proposals {
            let Some(settlement_id) = settlement_id_map.get(&proposal.settlement_id).copied()
            else {
                continue;
            };
            let id = self.next_proposal_id;
            self.next_proposal_id += 1;
            self.proposals.push_back(ProposalRecord {
                id,
                settlement_id,
                intent: proposal.intent.clone(),
                key: entity_remap
                    .get(&proposal.key)
                    .copied()
                    .unwrap_or(proposal.key),
                payload: proposal.payload.clone(),
                law: proposal.law.clone(),
                source_line: proposal.source_line,
            });
            proposal_id_map.insert(proposal.id, id);
        }
        let mut resolution_id_map = std::collections::HashMap::new();
        for resolution in &prov.resolutions {
            let Some(settlement_id) = settlement_id_map.get(&resolution.settlement_id).copied()
            else {
                continue;
            };
            let id = self.next_resolution_id;
            self.next_resolution_id += 1;
            self.resolutions.push_back(ResolutionRecord {
                id,
                settlement_id,
                intent: resolution.intent.clone(),
                key: entity_remap
                    .get(&resolution.key)
                    .copied()
                    .unwrap_or(resolution.key),
                resolver: resolution.resolver.clone(),
                proposal_ids: resolution
                    .proposal_ids
                    .iter()
                    .filter_map(|proposal_id| proposal_id_map.get(proposal_id).copied())
                    .collect(),
            });
            resolution_id_map.insert(resolution.id, id);
        }
        for relation_assertion in &prov.relation_assertions {
            let mut fact_key = relation_assertion.fact_key.clone();
            for value in &mut fact_key.tuple {
                if let crate::relation::runtime::FactValue::Entity(entity) = value {
                    entity.slot = entity_remap
                        .get(&entity.slot)
                        .copied()
                        .unwrap_or(entity.slot);
                }
            }
            self.relation_assertions.push_back(RelationAssertionRecord {
                frame: relation_assertion.frame,
                assertion_id: relation_assertion.assertion_id,
                fact_key,
                resolution_ids: relation_assertion
                    .resolution_ids
                    .iter()
                    .filter_map(|id| resolution_id_map.get(id).copied())
                    .collect(),
                origin: Some(
                    relation_assertion
                        .origin
                        .clone()
                        .unwrap_or_else(|| origin.clone()),
                ),
            });
        }
        for w in &prov.writes {
            let by = remap_cause(&w.by, &id_map);
            let entity = w.entity.map(|e| entity_remap.get(&e).copied().unwrap_or(e));
            self.writes.push_back(WriteRecord {
                frame: w.frame,
                entity,
                entity_name: w.entity_name.clone(),
                component: w.component.clone(),
                summary: w.summary.clone(),
                kind: w.kind,
                by,
                origin: Some(w.origin.clone().unwrap_or_else(|| origin.clone())),
                resolution_id: w
                    .resolution_id
                    .and_then(|id| resolution_id_map.get(&id).copied()),
            });
        }
        self.evict_overflow();
        id_map
    }
}
