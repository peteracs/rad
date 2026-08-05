

/// Tag an emit id as foreign (0 = "unknown" stays 0).
pub fn foreign_emit_id(id: u64) -> u64 {
    if id == 0 {
        0
    } else {
        id | FOREIGN_EMIT_BIT
    }
}

#[derive(Clone, Debug)]
pub struct CausalityLedger {
    /// Ring buffers, not Vecs: the retention cap makes eviction the steady
    /// state of any long-running process, so removing the oldest record
    /// must be O(1), not a front-of-Vec memmove of the whole window.
    pub writes: std::collections::VecDeque<WriteRecord>,
    pub emits: std::collections::VecDeque<EmitRecord>,
    /// `(frame, write_watermark)` for each `commit()` that replaced the
    /// world with a fork; the watermark is the *absolute* write count at
    /// commit time, which orders the commit against writes *within* a
    /// frame. Writes performed inside forks are invisible to the ledger, so
    /// any value whose newest record predates a commit may actually
    /// originate from the committed timeline — `why()` discloses that seam
    /// honestly instead of presenting pre-fork provenance as the whole truth.
    pub commits: Vec<(u64, usize)>,
    pub settlements: std::collections::VecDeque<SettlementRecord>,
    pub proposals: std::collections::VecDeque<ProposalRecord>,
    pub resolutions: std::collections::VecDeque<ResolutionRecord>,
    pub relation_assertions: std::collections::VecDeque<RelationAssertionRecord>,
    pub(crate) next_settlement_id: u64,
    pub(crate) next_proposal_id: u64,
    pub(crate) next_resolution_id: u64,
    /// Retention window: the ledger keeps at most this many write and emit
    /// records each, evicting the oldest. Long-running processes must not
    /// OOM by bookkeeping; recent provenance stays in RAM, full history
    /// lives in the trace (`rad run --record` + `rad replay` rebuild it).
    cap: usize,
    /// Number of evicted write records (absolute index of `writes[0]`).
    write_base: usize,
    /// Number of evicted emit records (ids `<= emit_base` are gone).
    emit_base: usize,
    /// Whether any record has ever been evicted — `why()` says so.
    truncated: bool,
}

pub const DEFAULT_RETENTION_CAP: usize = 100_000;

impl Default for CausalityLedger {
    fn default() -> Self {
        CausalityLedger {
            writes: std::collections::VecDeque::new(),
            emits: std::collections::VecDeque::new(),
            commits: Vec::new(),
            settlements: std::collections::VecDeque::new(),
            proposals: std::collections::VecDeque::new(),
            resolutions: std::collections::VecDeque::new(),
            relation_assertions: std::collections::VecDeque::new(),
            next_settlement_id: 1,
            next_proposal_id: 1,
            next_resolution_id: 1,
            cap: DEFAULT_RETENTION_CAP,
            write_base: 0,
            emit_base: 0,
            truncated: false,
        }
    }
}

const SUMMARY_CAP: usize = 96;
const CHAIN_DEPTH_CAP: usize = 16;

/// Truncate a display string to a bounded summary.
pub fn summarize(s: &str) -> String {
    if s.len() <= SUMMARY_CAP {
        s.to_string()
    } else {
        let mut cut = SUMMARY_CAP;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &s[..cut])
    }
}

impl CausalityLedger {
    /// Versioned semantic encoding used by portable attempt checkpoints.
    /// Diagnostic `Debug` output is deliberately not part of replay identity.
    pub(crate) fn encode_checkpoint(&self, out: &mut crate::canonical::CanonicalWriter) {
        fn cause(out: &mut crate::canonical::CanonicalWriter, cause: &Cause) {
            match cause {
                Cause::Main => out.byte(0),
                Cause::System { name } => {
                    out.byte(1);
                    out.text(name);
                }
                Cause::Handler { event, emit_id } => {
                    out.byte(2);
                    out.text(event);
                    out.u64(*emit_id);
                }
            }
        }
        fn write_kind(out: &mut crate::canonical::CanonicalWriter, kind: WriteKind) {
            out.byte(match kind {
                WriteKind::Set => 0,
                WriteKind::Spawn => 1,
                WriteKind::Despawn => 2,
                WriteKind::Remove => 3,
                WriteKind::Resource => 4,
            });
        }

        out.usize(self.writes.len());
        for write in &self.writes {
            out.u64(write.frame);
            out.bool(write.entity.is_some());
            if let Some(entity) = write.entity {
                out.u32(entity);
            }
            out.optional_text(write.entity_name.as_deref());
            out.text(&write.component);
            out.text(&write.value);
            write_kind(out, write.kind);
            cause(out, &write.by);
            out.optional_text(write.origin.as_deref());
            out.optional_u64(write.resolution_id);
        }

        out.usize(self.emits.len());
        for emit in &self.emits {
            out.u64(emit.id);
            out.text(&emit.event);
            out.u64(emit.frame);
            out.text(&emit.payload);
            cause(out, &emit.by);
            out.optional_text(emit.origin.as_deref());
        }

        out.usize(self.commits.len());
        for (frame, watermark) in &self.commits {
            out.u64(*frame);
            out.usize(*watermark);
        }

        out.usize(self.settlements.len());
        for settlement in &self.settlements {
            out.u64(settlement.id);
            out.u64(settlement.frame);
            cause(out, &settlement.by);
        }

        out.usize(self.proposals.len());
        for proposal in &self.proposals {
            out.u64(proposal.id);
            out.u64(proposal.settlement_id);
            out.text(&proposal.intent);
            out.u32(proposal.key);
            out.text(&proposal.payload);
            out.text(&proposal.law);
            out.u32(proposal.source_line);
        }

        out.usize(self.resolutions.len());
        for resolution in &self.resolutions {
            out.u64(resolution.id);
            out.u64(resolution.settlement_id);
            out.text(&resolution.intent);
            out.u32(resolution.key);
            out.text(&resolution.resolver);
            out.usize(resolution.proposal_ids.len());
            for proposal_id in &resolution.proposal_ids {
                out.u64(*proposal_id);
            }
        }

        out.usize(self.relation_assertions.len());
        for assertion in &self.relation_assertions {
            out.u64(assertion.frame);
            out.u64(assertion.assertion_id);
            out.text(&crate::relation_runtime::fact_key_transport_hex(
                &assertion.fact_key,
            ));
            out.usize(assertion.resolution_ids.len());
            for resolution_id in &assertion.resolution_ids {
                out.u64(*resolution_id);
            }
            out.optional_text(assertion.origin.as_deref());
        }

        out.u64(self.next_settlement_id);
        out.u64(self.next_proposal_id);
        out.u64(self.next_resolution_id);
        out.usize(self.cap);
        out.usize(self.write_base);
        out.usize(self.emit_base);
        out.bool(self.truncated);
    }

    pub(crate) fn encode_cause_checkpoint(
        cause: &Cause,
        out: &mut crate::canonical::CanonicalWriter,
    ) {
        match cause {
            Cause::Main => out.byte(0),
            Cause::System { name } => {
                out.byte(1);
                out.text(name);
            }
            Cause::Handler { event, emit_id } => {
                out.byte(2);
                out.text(event);
                out.u64(*emit_id);
            }
        }
    }

    /// Shrink (or grow) the retention window. Mostly for tests and servers
    /// with tight memory budgets.
    pub fn set_retention_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        self.evict_overflow();
    }

    fn evict_overflow(&mut self) {
        if self.writes.len() > self.cap {
            let n = self.writes.len() - self.cap;
            self.writes.drain(..n);
            self.write_base += n;
            self.truncated = true;
        }
        if self.emits.len() > self.cap {
            let n = self.emits.len() - self.cap;
            self.emits.drain(..n);
            self.emit_base += n;
            self.truncated = true;
        }
        while self.settlements.len() > self.cap {
            self.settlements.pop_front();
            self.truncated = true;
        }
        while self.proposals.len() > self.cap {
            self.proposals.pop_front();
            self.truncated = true;
        }
        while self.resolutions.len() > self.cap {
            self.resolutions.pop_front();
            self.truncated = true;
        }
        while self.relation_assertions.len() > self.cap {
            self.relation_assertions.pop_front();
            self.truncated = true;
        }
    }

    /// Returns the emit id used by `Cause::Handler` links (1-based; 0 is
    /// reserved for "unknown" and never matches a record). Ids are stable
    /// across retention eviction.
    pub fn record_emit(&mut self, frame: u64, event: &str, payload: String, by: Cause) -> u64 {
        let id = (self.emit_base + self.emits.len()) as u64 + 1;
        self.emits.push_back(EmitRecord {
            id,
            event: event.to_string(),
            frame,
            payload,
            by,
            origin: None,
        });
        self.evict_overflow();
        id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_write(
        &mut self,
        frame: u64,
        entity: Option<u32>,
        entity_name: Option<String>,
        component: &str,
        value: String,
        kind: WriteKind,
        by: Cause,
    ) {
        self.writes.push_back(WriteRecord {
            frame,
            entity,
            entity_name,
            component: component.to_string(),
            value,
            kind,
            by,
            origin: None,
            resolution_id: None,
        });
        self.evict_overflow();
    }

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
            if let Cause::Handler { emit_id, .. } = w.by {
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
            if let Cause::Handler { emit_id, .. } = settlement.by {
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
                if let Cause::Handler { emit_id, .. } = e.by {
                    wanted.push(emit_id);
                }
            }
        }
        emits.sort_by_key(|e| e.id);

        let tag_cause = |c: &Cause| match c {
            Cause::Handler { event, emit_id } => Cause::Handler {
                event: event.clone(),
                emit_id: foreign_emit_id(*emit_id),
            },
            other => other.clone(),
        };
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
        let mut id_map: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        let origin = if prov.origin.is_empty() {
            "wire".to_string()
        } else {
            prov.origin.clone()
        };
        // Sender order = ascending ids; handler links always point backwards,
        // so the map is complete by the time a reference is rewritten.
        for e in &prov.emits {
            let by = match &e.by {
                Cause::Handler { event, emit_id } => Cause::Handler {
                    event: event.clone(),
                    emit_id: id_map.get(emit_id).copied().unwrap_or(*emit_id),
                },
                other => other.clone(),
            };
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
            let by = match &settlement.by {
                Cause::Handler { event, emit_id } => Cause::Handler {
                    event: event.clone(),
                    emit_id: id_map.get(emit_id).copied().unwrap_or(*emit_id),
                },
                other => other.clone(),
            };
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
                if let crate::relation_runtime::FactValue::Entity(entity) = value {
                    entity.slot = entity_remap
                        .get(&entity.slot)
                        .copied()
                        .unwrap_or(entity.slot);
                }
            }
            self.relation_assertions
                .push_back(RelationAssertionRecord {
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
            let by = match &w.by {
                Cause::Handler { event, emit_id } => Cause::Handler {
                    event: event.clone(),
                    emit_id: id_map.get(emit_id).copied().unwrap_or(*emit_id),
                },
                other => other.clone(),
            };
            let entity = w.entity.map(|e| entity_remap.get(&e).copied().unwrap_or(e));
            self.writes.push_back(WriteRecord {
                frame: w.frame,
                entity,
                entity_name: w.entity_name.clone(),
                component: w.component.clone(),
                value: w.value.clone(),
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

    /// Record that `commit()` adopted a fork's world in `frame`.
    pub fn record_commit(&mut self, frame: u64) {
        let watermark = self.write_base + self.writes.len();
        self.commits.push((frame, watermark));
    }

    fn emit_by_id(&self, id: u64) -> Option<&EmitRecord> {
        if id == 0 || (id as usize) <= self.emit_base {
            return None;
        }
        self.emits.get(id as usize - 1 - self.emit_base)
    }

    /// Explain the last write to `component` on entity `eid`, considering
    /// only writes with `frame < up_to_exclusive` (pass `u64::MAX` for the
    /// live world).
    pub fn explain_entity(&self, eid: u32, component: &str, up_to_exclusive: u64) -> String {
        self.explain(
            |w| w.entity == Some(eid) && (w.component == component || w.component == "*"),
            &format!("{} of entity {}", component, eid),
            up_to_exclusive,
        )
    }

    /// Explain the last write to a resource.
    pub fn explain_resource(&self, resource: &str, up_to_exclusive: u64) -> String {
        self.explain(
            |w| w.entity.is_none() && w.component == resource,
            &format!("resource {}", resource),
            up_to_exclusive,
        )
    }

    /// Explain by entity *name* — used by the time-travel server, where the
    /// caller addresses entities the same way `peek` does.
    pub fn explain_named(&self, name: &str, component: &str, up_to_exclusive: u64) -> String {
        self.explain(
            |w| {
                w.entity_name.as_deref() == Some(name)
                    && (w.component == component || w.component == "*")
            },
            &format!("{} of {}", component, name),
            up_to_exclusive,
        )
    }

    pub fn explain_relation_assertion(
        &self,
        fact_key: &crate::relation_runtime::FactKey,
        assertion_id: u64,
    ) -> String {
        let Some(record) = self
            .relation_assertions
            .iter()
            .rev()
            .find(|record| {
                record.assertion_id == assertion_id && record.fact_key == *fact_key
            })
        else {
            return format!(
                "assertion #{}: exact causal record unavailable",
                assertion_id
            );
        };
        let mut out = format!(
            "assertion #{} of {} {:?}   (created in frame {})",
            assertion_id, fact_key.relation, fact_key.tuple, record.frame
        );
        if let Some(origin) = &record.origin {
            out.push_str(&format!("   [via {origin}, remote frame]"));
        }
        if record.resolution_ids.is_empty() {
            out.push_str("\n  <- no retained resolver fan-in");
        } else {
            for resolution_id in &record.resolution_ids {
                if let Some(tree) = self.render_resolution(*resolution_id) {
                    out.push_str(&tree);
                } else {
                    out.push_str(
                        "\n  note: settlement fan-in provenance was evicted by the retention window",
                    );
                }
            }
        }
        out
    }

    pub(super) fn render_cause_chain(&self, initial: &Cause) -> String {
        let mut out = String::new();
        let mut cause = initial;
        let mut terminated = false;
        for _ in 0..CHAIN_DEPTH_CAP {
            match cause {
                Cause::Main => {
                    out.push_str("\n  <- by top-level code");
                    terminated = true;
                    break;
                }
                Cause::System { name } => {
                    out.push_str(&format!("\n  <- by system {name}"));
                    terminated = true;
                    break;
                }
                Cause::Handler { event, emit_id } => {
                    out.push_str(&format!("\n  <- by `on {event}` handler"));
                    match self.emit_by_id(*emit_id) {
                        Some(emit) => {
                            if emit.payload.starts_with(&emit.event) {
                                out.push_str(&format!(
                                    "\n  <- {} emitted in frame {}",
                                    emit.payload, emit.frame
                                ));
                            } else {
                                out.push_str(&format!(
                                    "\n  <- {} {} emitted in frame {}",
                                    emit.event, emit.payload, emit.frame
                                ));
                            }
                            if let Some(origin) = &emit.origin {
                                out.push_str(&format!(" [via {origin}]"));
                            }
                            cause = &emit.by;
                        }
                        None => {
                            out.push_str("\n  <- (emit record unavailable)");
                            terminated = true;
                            break;
                        }
                    }
                }
            }
        }
        if !terminated {
            out.push_str("\n  <- … (causal chain truncated)");
        }
        out
    }

    fn explain(
        &self,
        matches: impl Fn(&WriteRecord) -> bool,
        what: &str,
        up_to_exclusive: u64,
    ) -> String {
        let Some((w_idx, w)) = self
            .writes
            .iter()
            .enumerate()
            .rev()
            .find(|(_, w)| w.frame < up_to_exclusive && matches(w))
        else {
            let mut msg = format!(
                "{}: no recorded write — the value (if it exists) predates tracking \
                 or was never written on the main timeline",
                what
            );
            if self.truncated {
                msg.push_str(
                    "\n  note: older provenance was evicted by the retention window — \
                     replay the recorded trace for full history",
                );
            }
            if let Some(&(cf, _)) = self
                .commits
                .iter()
                .rev()
                .find(|&&(cf, _)| cf < up_to_exclusive)
            {
                msg.push_str(&format!(
                    "\n  note: commit() adopted a fork in frame {} — the value may have \
                     been written inside that fork (fork writes are not in this ledger)",
                    cf
                ));
            }
            return msg;
        };

        let mut out = String::new();
        let target = w
            .entity_name
            .as_deref()
            .map(|n| n.to_string())
            .or_else(|| w.entity.map(|id| format!("entity {}", id)))
            .unwrap_or_else(|| format!("resource {}", w.component));
        match w.kind {
            WriteKind::Set => out.push_str(&format!(
                "{} of {} = {}   (set in frame {})",
                w.component, target, w.value, w.frame
            )),
            WriteKind::Spawn => out.push_str(&format!(
                "{} of {} = {}   (spawned in frame {})",
                w.component, target, w.value, w.frame
            )),
            WriteKind::Despawn => {
                out.push_str(&format!("{} was despawned in frame {}", target, w.frame))
            }
            WriteKind::Remove => out.push_str(&format!(
                "{} was removed from {} in frame {}",
                w.component, target, w.frame
            )),
            WriteKind::Resource => out.push_str(&format!(
                "resource {} = {}   (set in frame {})",
                w.component, w.value, w.frame
            )),
        }
        if let Some(origin) = &w.origin {
            // Remote provenance: the frame number is the sender's clock.
            out.push_str(&format!("   [via {}, remote frame]", origin));
        }

        let mut resolution_rendered = false;
        if let Some(resolution_id) = w.resolution_id {
            if let Some(tree) = self.render_resolution(resolution_id) {
                out.push_str(&tree);
                resolution_rendered = true;
            } else {
                out.push_str(
                    "\n  note: settlement fan-in provenance was evicted by the retention window",
                );
            }
        }

        // Walk the causal chain: write -> cause -> (emit -> cause)*.
        if !resolution_rendered {
            out.push_str(&self.render_cause_chain(&w.by));
        }
        // The commit seam, disclosed: if a fork was committed after this
        // write (by ledger order, which resolves within-frame ties), the
        // value on screen may have been produced inside that fork —
        // provenance the ledger cannot see. Watermarks are absolute, so
        // retention eviction does not shift them.
        let w_abs = self.write_base + w_idx;
        if let Some(&(cf, _)) = self
            .commits
            .iter()
            .rev()
            .find(|&&(cf, wm)| wm > w_abs && cf < up_to_exclusive)
        {
            out.push_str(&format!(
                "\n  note: commit() adopted a fork in frame {} (after this write) — \
                 the current value may originate inside that fork; fork writes are \
                 not in this ledger",
                cf
            ));
        }
        out
    }
}
