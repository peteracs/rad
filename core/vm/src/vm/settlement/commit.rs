impl VM {

    pub(crate) fn finish_settlement(&mut self) -> Result<(), crate::constraint_types::VmFailure> {
        self.ensure_current_frame_owns_settlement("EndSettlement")?;
        let result = self.resolve_and_commit_settlement();
        if result.is_err() {
            self.abort_settlement();
        }
        result
    }

    pub(crate) fn ensure_current_frame_owns_settlement(
        &self,
        operation: &str,
    ) -> Result<(), String> {
        let context = self
            .settlement
            .as_ref()
            .ok_or_else(|| format!("{} without an active settlement", operation))?;
        let frame = self.current_frame();
        if frame.frame_id != context.owner_frame_id || frame.chunk_id != context.owner_chunk_id {
            return Err(format!(
                "Internal VM error: {} in frame {} chunk {} cannot close settlement {} owned by frame {} chunk {} (began at byte {})",
                operation,
                frame.frame_id,
                frame.chunk_id,
                context.settlement_id,
                context.owner_frame_id,
                context.owner_chunk_id,
                context.begin_ip
            ));
        }
        Ok(())
    }

    /// A settlement owner must reach its matching EndSettlement. Callees may
    /// return normally while the caller's transaction remains active.
    pub(crate) fn guard_frame_exit(&mut self, operation: &str) -> Result<(), String> {
        let Some(context) = self.settlement.as_ref() else {
            return Ok(());
        };
        let frame = self.current_frame();
        if frame.frame_id != context.owner_frame_id {
            return Ok(());
        }
        let message = format!(
            "Internal VM error: {} would leave settlement {} opened by frame {} chunk {} at byte {}; transaction was aborted",
            operation,
            context.settlement_id,
            context.owner_frame_id,
            context.owner_chunk_id,
            context.begin_ip
        );
        self.abort_settlement();
        Err(message)
    }

    fn resolve_and_commit_settlement(&mut self) -> Result<(), crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
        let proposals = self
            .settlement
            .as_ref()
            .ok_or_else(|| "No active settlement".to_string())?
            .proposals
            .clone();

        let mut groups: BTreeMap<(String, u32), Vec<Proposal>> = BTreeMap::new();
        for proposal in proposals {
            groups
                .entry((proposal.intent.clone(), proposal.key))
                .or_default()
                .push(proposal);
        }
        for proposals in groups.values_mut() {
            proposals.sort_by(|a, b| {
                (&a.canonical, &a.law, a.source_line).cmp(&(&b.canonical, &b.law, b.source_line))
            });
        }

        for ((intent, key), proposals) in groups {
            let resolver = self
                .resolver_registry
                .get(&intent)
                .cloned()
                .ok_or_else(|| format!("Proposed intent '{}' has no owning resolver", intent))?;
            let callee = self
                .globals
                .get(resolver.global_slot as usize)
                .cloned()
                .ok_or_else(|| format!("Resolver '{}' was not initialized", resolver.name))?;
            if callee.is_nil() {
                return Err(format!("Resolver '{}' was not initialized", resolver.name));
            }
            let proposal_ids = proposals
                .iter()
                .map(|proposal| proposal.id)
                .collect::<Vec<_>>();
            let list = Value::list(
                &mut self.gc,
                proposals.iter().map(|proposal| proposal.payload).collect(),
            );
            let key_value = Value::from_entity_id(&mut self.gc, key);
            self.settlement.as_mut().unwrap().active = Some(ActiveResolution {
                resolver: resolver.name.clone(),
                intent: resolver.intent.clone(),
                key,
                proposal_ids,
                writes: Vec::new(),
                relation_operations: Vec::new(),
            });
            if let Result::Err(error) = self.call_value(&callee, vec![key_value, list]) {
                self.settlement.as_mut().unwrap().active = None;
                return Err(format!(
                    "Settlement aborted while resolver `{}` handled {}({}): {}\nNo world state was changed.",
                    resolver.name, intent, key, error
                ));
            }
            let active = self
                .settlement
                .as_mut()
                .and_then(|context| context.active.take())
                .ok_or_else(|| "Resolver finished without an active resolution".to_string())?;
            self.settlement
                .as_mut()
                .unwrap()
                .patches
                .push(ResolutionPatch {
                    resolver: active.resolver,
                    intent: active.intent,
                    key: active.key,
                    proposal_ids: active.proposal_ids,
                    writes: active.writes,
                    relation_operations: active.relation_operations,
                });
        }

        let context = self.settlement.as_ref().unwrap();
        let mut owners: HashMap<(u32, String), (&str, &str, u32)> = HashMap::new();
        for patch in &context.patches {
            for write in &patch.writes {
                let subject = (write.entity, write.component.type_name.clone());
                if let Some((other_resolver, other_intent, _other_key)) = owners.get(&subject) {
                    let entity_label = self
                        .world
                        .entity_name(write.entity)
                        .unwrap_or_else(|| write.entity.to_string());
                    return Err(format!(
                        "Settlement aborted: conflicting candidate writes\n\n`{}` of entity `{}` was written by:\n  - resolver `{}` for {}({})\n  - resolver `{}` for {}({})\n\nNo world state was changed.\n\nhelp: combine these causes under one owning intent/resolver,\n      or split them into two explicit settle boundaries.",
                        write.component.type_name,
                        entity_label,
                        other_resolver,
                        other_intent,
                        entity_label,
                        patch.resolver,
                        patch.intent,
                        entity_label
                    ));
                }
                owners.insert(subject, (&patch.resolver, &patch.intent, patch.key));
            }
        }

        // Build the complete authoritative candidate before any constraint
        // executes. Constraint reads are served from this snapshot, so
        // components, relations, and lifecycle never acquire separate
        // validation/commit phases.
        let base = context.base.clone();
        let mut candidate = World::new();
        candidate.restore(base);
        for patch in &context.patches {
            for write in &patch.writes {
                if !candidate.set_component(write.entity, write.component.clone()) {
                    return Err(format!(
                        "Settlement aborted: failed to replace `{}` on entity {}\nNo world state was changed.",
                        write.component.type_name, write.entity
                    ));
                }
            }
        }

        let relation_operations = context
            .patches
            .iter()
            .flat_map(|patch| patch.relation_operations.iter().cloned())
            .collect::<Vec<_>>();
        let mut relation_changes = Vec::new();
        if !relation_operations.is_empty() {
            let transaction = crate::relation_runtime::RelationTransaction {
                spawns: Vec::new(),
                component_writes: Vec::new(),
                operations: relation_operations,
                despawns: Vec::new(),
            };
            let relation_candidate = candidate
                .prepare_relation_candidate(
                    &transaction,
                    candidate.live_relation_entities(),
                    BTreeMap::new(),
                )
                .map_err(|error| {
                    format!("Settlement aborted: {}\nNo world state was changed.", error)
                })?;
            relation_changes = candidate
                .adopt_relation_candidate(relation_candidate)
                .map_err(|error| {
                    format!("Settlement aborted: {}\nNo world state was changed.", error)
                })?;
        }

        let committed = candidate.snapshot();
        {
            let context = self.settlement.as_mut().unwrap();
            context.candidate = Some(committed.clone());
            context.relation_changes = relation_changes;
        }
        match self.evaluate_candidate_constraints() {
            crate::constraint_types::ValidationResult::Accepted => {}
            crate::constraint_types::ValidationResult::Rejected(rejection) => {
                return Result::Err(crate::constraint_types::VmFailure::SettlementRejected(
                    rejection,
                ));
            }
            crate::constraint_types::ValidationResult::HostAborted(fault) => {
                return Result::Err(crate::constraint_types::VmFailure::Host(fault));
            }
        }

        let context = self.settlement.take().unwrap();
        self.world.restore(committed);
        self.record_settlement_provenance(&context);
        Ok(())
    }

    fn record_settlement_provenance(&mut self, context: &SettlementContext) {
        if self.in_simulation_fork > 0 || self.is_worker {
            return;
        }
        let proposal_inputs = context
            .proposals
            .iter()
            .map(|proposal| crate::causality::SettlementProposalInput {
                runtime_id: proposal.id,
                intent: proposal.intent.clone(),
                key: proposal.key,
                payload: self.ledger_payload(&proposal.payload),
                law: proposal.law.clone(),
                source_line: proposal.source_line,
            })
            .collect::<Vec<_>>();
        let resolution_inputs = context
            .patches
            .iter()
            .map(|patch| crate::causality::SettlementResolutionInput {
                intent: patch.intent.clone(),
                key: patch.key,
                resolver: patch.resolver.clone(),
                proposal_runtime_ids: patch.proposal_ids.clone(),
            })
            .collect::<Vec<_>>();
        let resolution_ids = self.ledger.record_settlement(
            self.causality_frame,
            context.origin.clone(),
            &proposal_inputs,
            &resolution_inputs,
        );
        for (patch_index, patch) in context.patches.iter().enumerate() {
            let resolution_id = resolution_ids.get(patch_index).copied();
            for write in &patch.writes {
                let summary = Self::component_summary(&write.component);
                let entity_name = self.world.entity_name(write.entity);
                self.ledger.record_write_with_resolution(
                    self.causality_frame,
                    write.entity,
                    entity_name,
                    &write.component.type_name,
                    summary,
                    context.origin.clone(),
                    resolution_id,
                );
            }
        }
        for change in &context.relation_changes {
            let component = format!("relation::{}", change.fact_key.relation);
            let value = format!("{:?} {:?}", change.kind, change.fact_key.tuple);
            let mut recorded = false;
            for (patch_index, patch) in context.patches.iter().enumerate() {
                let cause = relation_resolution_cause(
                    &patch.resolver,
                    &patch.intent,
                    patch.key,
                    &patch.proposal_ids,
                );
                if !change.causes.contains(&cause) {
                    continue;
                }
                recorded = true;
                self.ledger.record_write_with_resolution(
                    self.causality_frame,
                    patch.key,
                    self.world.entity_name(patch.key),
                    &component,
                    value.clone(),
                    context.origin.clone(),
                    resolution_ids.get(patch_index).copied(),
                );
            }
            if !recorded {
                self.ledger.record_write(
                    self.causality_frame,
                    None,
                    None,
                    &component,
                    value,
                    crate::causality::WriteKind::Set,
                    context.origin.clone(),
                );
            }
        }
    }
}
