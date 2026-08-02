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
pub use settlement::{ProposalRecord, ResolutionRecord, SettlementRecord};
pub(crate) use settlement::{SettlementProposalInput, SettlementResolutionInput};

/// Who performed a write or an emit.
#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// Top-level program code (or any plain function called from it).
    Main,
    /// A system body (writebacks included).
    System { name: String },
    /// An event handler; `emit_id` keys the exact [`EmitRecord`] of the
    /// event *instance* being handled — the link that makes chains causal
    /// rather than merely correlated.
    Handler { event: String, emit_id: u64 },
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
    pub entity_name: Option<String>,
    pub component: String,
    /// Display summary of the written value (truncated).
    pub value: String,
    pub kind: WriteKind,
    pub by: Cause,
    /// `Some("wire <digest>")` when this record was ingested from another
    /// machine's ledger (it rode a fork payload). Frames inside such records
    /// follow the *sender's* clock; `why()` discloses the origin.
    pub origin: Option<String>,
    /// Fan-in resolution that produced this write, for RFC-0001 settlements.
    pub resolution_id: Option<u64>,
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
}

/// High-bit namespace tag for emit ids that came over the wire. Local ledger
/// ids are sequential and will never reach this range honestly.
pub const FOREIGN_EMIT_BIT: u64 = 1 << 63;

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
        let wanted_resolution_ids: HashSet<u64> = newest
            .values()
            .filter_map(|write| write.resolution_id)
            .collect();
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

        if let Some(resolution_id) = w.resolution_id {
            if let Some(tree) = self.render_resolution(resolution_id) {
                out.push_str(&tree);
            } else {
                out.push_str(
                    "\n  note: settlement fan-in provenance was evicted by the retention window",
                );
            }
        }

        // Walk the causal chain: write -> cause -> (emit -> cause)*.
        let mut cause = &w.by;
        let mut terminated = false;
        for _ in 0..CHAIN_DEPTH_CAP {
            match cause {
                Cause::Main => {
                    out.push_str("\n  <- by top-level code");
                    terminated = true;
                    break;
                }
                Cause::System { name } => {
                    out.push_str(&format!("\n  <- by system {}", name));
                    terminated = true;
                    break;
                }
                Cause::Handler { event, emit_id } => {
                    out.push_str(&format!("\n  <- by `on {}` handler", event));
                    match self.emit_by_id(*emit_id) {
                        Some(emit) => {
                            // Component payloads display as `Name { … }` —
                            // avoid doubling the event name.
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
                                out.push_str(&format!(" [via {}]", origin));
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

#[cfg(test)]
mod tests {
    use super::{CausalityLedger, Cause, WriteKind};
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::VM;

    fn run(src: &str) -> VM {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        let result = Compiler::new().compile(&program).expect("compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.load_compile_result(result);
        vm.run(0).expect("run");
        vm
    }

    #[test]
    fn top_level_write_explains_itself() {
        let vm = run(r#"
            component Health { hp: 100 }
            let hero = spawn("hero", Health { hp: 100 })
            set(hero, Health { hp: 50 })
            print(why(hero, Health))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("Health of hero = { hp: 50 }"), "got: {}", out);
        assert!(out.contains("(set in frame 0)"), "got: {}", out);
        assert!(out.contains("<- by top-level code"), "got: {}", out);
    }

    #[test]
    fn spawn_provenance_when_never_set() {
        let vm = run(r#"
            component Pos { x: 0 }
            let e = spawn("rock", Pos { x: 7 })
            print(why(e, Pos))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("(spawned in frame 0)"), "got: {}", out);
        assert!(out.contains("<- by top-level code"), "got: {}", out);
    }

    #[test]
    fn handler_chain_walks_back_through_two_events() {
        // Hit -> (handler emits) Robbed -> (handler sets) Gold. The chain
        // must surface both events and end at top-level code.
        let vm = run(r#"
            component Gold { amount: 50 }
            event Hit { amount }
            event Robbed { loss }
            let hero = spawn("hero", Gold { amount: 50 })
            on Hit(e) {
                emit Robbed { loss: e.amount }
            }
            on Robbed(e) {
                set(hero, Gold { amount: 0 })
            }
            emit Hit { amount: 10 }
            flush_events()
            flush_events()
            print(why(hero, Gold))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("Gold of hero = { amount: 0 }"), "got: {}", out);
        assert!(out.contains("(set in frame 2)"), "got: {}", out);
        assert!(out.contains("<- by `on Robbed` handler"), "got: {}", out);
        assert!(
            out.contains("Robbed { loss: 10 } emitted in frame 1"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by `on Hit` handler"), "got: {}", out);
        assert!(
            out.contains("Hit { amount: 10 } emitted in frame 0"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by top-level code"), "got: {}", out);
    }

    #[test]
    fn system_writeback_attributes_to_the_system() {
        let vm = run(r#"
            component Health { hp: 100 }
            system Decay(h: mut Health) {
                h = Health { hp: h.hp - 1 }
            }
            let hero = spawn("hero", Health { hp: 100 })
            Decay()
            print(why(hero, Health))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("Health of hero = { hp: 99 }"), "got: {}", out);
        assert!(out.contains("<- by system Decay"), "got: {}", out);
    }

    #[test]
    fn resource_writes_chain_through_handlers() {
        let vm = run(r#"
            resource Treasury { gold: 0 }
            event Loot { amount }
            on Loot(e) {
                let t = get_resource(Treasury) |> unwrap
                set_resource(Treasury, Treasury { gold: t.gold + e.amount })
            }
            emit Loot { amount: 25 }
            flush_events()
            print(why_resource(Treasury))
        "#);
        let out = &vm.print_buffer[0];
        assert!(
            out.contains("resource Treasury = { gold: 25 }"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by `on Loot` handler"), "got: {}", out);
        assert!(
            out.contains("Loot { amount: 25 } emitted in frame 0"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by top-level code"), "got: {}", out);
    }

    #[test]
    fn unwritten_values_say_so() {
        let vm = run(r#"
            component Pos { x: 0 }
            component Vel { dx: 0 }
            let e = spawn("rock", Pos { x: 1 })
            print(why(e, Vel))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("no recorded write"), "got: {}", out);
    }

    #[test]
    fn simulation_forks_leave_no_provenance() {
        // The harder decay runs only inside simulate(): the main timeline's
        // ledger must still attribute Health to its spawn.
        let vm = run(r#"
            component Health { hp: 100 }
            system Decay(h: mut Health) {
                h = Health { hp: h.hp - 10 }
            }
            let hero = spawn("hero", Health { hp: 100 })
            let before = fork()
            let after = simulate(before, [system::Decay], 5)
            print(why(hero, Health))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("(spawned in frame 0)"), "got: {}", out);
        assert!(!out.contains("by system Decay"), "got: {}", out);
    }

    #[test]
    fn eviction_is_amortized_constant_time() {
        // The retention cap exists so long-running processes don't OOM —
        // which means the *eviction path* is the steady state of a long
        // process, not a rare corner. It must be O(1) per write. (It once
        // was a front-of-Vec drain per write: a full-window memmove each,
        // quadratic overall — the 1M-entity bench hung on world setup.)
        use std::time::Instant;
        let writes_n = 200_000usize;

        let mut under = CausalityLedger::default();
        under.set_retention_cap(1_000_000); // never evicts
        let t = Instant::now();
        for i in 0..writes_n {
            under.record_write(
                0,
                Some(i as u32),
                None,
                "Hp",
                format!("{{ hp: {} }}", i),
                WriteKind::Set,
                Cause::Main,
            );
        }
        let t_under = t.elapsed();

        let mut over = CausalityLedger::default();
        over.set_retention_cap(10_000); // evicts on ~95% of writes
        let t = Instant::now();
        for i in 0..writes_n {
            over.record_write(
                0,
                Some(i as u32),
                None,
                "Hp",
                format!("{{ hp: {} }}", i),
                WriteKind::Set,
                Cause::Main,
            );
        }
        let t_over = t.elapsed();

        assert_eq!(over.writes.len(), 10_000);
        // Generous bound: evicting writes may cost a small constant more
        // than appending ones (deallocation), but never a multiple.
        assert!(
            t_over < t_under * 4 + std::time::Duration::from_millis(50),
            "eviction must be amortized O(1): {:?} under cap vs {:?} evicting",
            t_under,
            t_over
        );
    }

    #[test]
    fn despawn_matches_any_component_query() {
        let vm = run(r#"
            component Pos { x: 0 }
            event Cull { }
            let e = spawn("rock", Pos { x: 1 })
            on Cull(c) {
                despawn(e)
            }
            emit Cull { }
            flush_events()
            print(why(e, Pos))
        "#);
        let out = &vm.print_buffer[0];
        assert!(
            out.contains("rock was despawned in frame 1"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by `on Cull` handler"), "got: {}", out);
    }
}
