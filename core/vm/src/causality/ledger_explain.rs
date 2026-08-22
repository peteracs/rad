// Answering `why`: entity, field, removal, missing-value, resource, and
// relation explanations, and the cause chain rendered beneath them.

impl CausalityLedger {

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

    pub fn explain_field(
        &self,
        eid: u32,
        component: &str,
        field: &str,
        up_to_exclusive: u64,
    ) -> String {
        let field_value = self
            .writes
            .iter()
            .rev()
            .filter(|write| write.frame < up_to_exclusive)
            .find(|write| {
                write.entity == Some(eid)
                    && write.component == component
                    && write.fields().iter().any(|(name, _)| name == field)
            })
            .and_then(|write| {
                write
                    .fields()
                    .iter()
                    .find(|(name, _)| name == field)
                    .map(|(_, value)| value.clone())
            });
        let chain = self.explain(
            |write| {
                write.entity == Some(eid)
                    && (write.component == component || write.component == "*")
                    && (matches!(write.kind, WriteKind::Remove | WriteKind::Despawn)
                        || write.fields().iter().any(|(name, _)| name == field))
            },
            &format!("{component}.{field} of entity {eid}"),
            up_to_exclusive,
        );
        field_value.map_or(chain.clone(), |value| {
            format!("{component}.{field} of entity {eid} = {value}\n{chain}")
        })
    }

    pub fn explain_removed(&self, eid: u32, component: &str) -> String {
        self.explain_removed_at(eid, component, u64::MAX)
    }

    pub fn explain_removed_at(&self, eid: u32, component: &str, up_to_exclusive: u64) -> String {
        self.explain(
            |write| {
                write.entity == Some(eid)
                    && (write.component == component || write.component == "*")
                    && matches!(write.kind, WriteKind::Remove | WriteKind::Despawn)
            },
            &format!("removal of {component} from entity {eid}"),
            up_to_exclusive,
        )
    }

    pub fn explain_missing_value(&self, component: &str, value: &str) -> String {
        self.explain_missing_value_at(component, value, u64::MAX)
    }

    pub fn explain_missing_value_at(
        &self,
        component: &str,
        value: &str,
        up_to_exclusive: u64,
    ) -> String {
        let candidate = self.writes.iter().rev().find(|write| {
            write.frame < up_to_exclusive
                && write.component == component
                && write
                    .fields()
                    .iter()
                    .any(|(_, field_value)| field_value.to_string() == value)
        });
        match candidate.and_then(|write| write.entity) {
            Some(entity) => self.explain_entity(entity, component, up_to_exclusive),
            None => format!(
                "{component} index value {value}: no retained publication or removal record"
            ),
        }
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
        fact_key: &crate::relation::runtime::FactKey,
        assertion_id: u64,
    ) -> String {
        let Some(record) = self
            .relation_assertions
            .iter()
            .rev()
            .find(|record| record.assertion_id == assertion_id && record.fact_key == *fact_key)
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
                Cause::Transaction { name, parent } => {
                    out.push_str(&format!("\n  <- by transaction {name}"));
                    cause = parent;
                }
                Cause::HostCall {
                    extension,
                    generation,
                    plugin_digest,
                    export,
                    input_digest,
                    output_digest,
                    parent,
                } => {
                    out.push_str(&format!(
                        "\n  <- by host {extension}@{generation}.{export} [plugin {plugin_digest}, input {input_digest}, output {output_digest}]"
                    ));
                    cause = parent;
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
            if self.truncation.is_truncated() {
                msg.push_str(&format!(
                    "\n  note: {} older provenance records were evicted by the retention window \
                     (commitment {}) — replay the recorded trace for full history",
                    self.truncation.evicted_records,
                    self.truncation.digest_hex()
                ));
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
                w.component, target, w.summary, w.frame
            )),
            WriteKind::Spawn => out.push_str(&format!(
                "{} of {} = {}   (spawned in frame {})",
                w.component, target, w.summary, w.frame
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
                w.component, w.summary, w.frame
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
