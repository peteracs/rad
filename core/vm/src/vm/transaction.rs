use super::*;
use crate::value::ComponentData;
use std::collections::BTreeSet;
use std::sync::Arc;

fn transaction_causes(
    current: &crate::causality::Cause,
    pending_host: Option<crate::causality::Cause>,
) -> (crate::causality::Cause, crate::causality::Cause) {
    match pending_host {
        Some(host_cause) => {
            let restore_cause = match &host_cause {
                crate::causality::Cause::HostCall { parent, .. } => (**parent).clone(),
                _ => current.clone(),
            };
            (host_cause, restore_cause)
        }
        None => (current.clone(), current.clone()),
    }
}

pub(crate) struct PendingTransactionWrite {
    pub(crate) entity: Option<u32>,
    pub(crate) entity_name: Option<String>,
    pub(crate) component: String,
    pub(crate) kind: crate::causality::WriteKind,
    pub(crate) summary: crate::causality::WriteSummary,
    pub(crate) view_refresh: MaterializedViewRefresh,
}

pub(crate) enum MaterializedViewRefresh {
    Component,
    Field(String),
}

pub(crate) struct TransactionContext {
    pub(crate) name: Arc<str>,
    pub(crate) owner_frame_id: u64,
    pub(crate) owner_chunk_id: usize,
    pub(crate) begin_ip: usize,
    /// Cause attached beneath this transaction's committed writes.
    pub(crate) effect_parent: crate::causality::Cause,
    /// Dynamic cause active before the one-shot host result was consumed.
    /// This is what execution must restore after commit or rollback.
    pub(crate) restore_cause: crate::causality::Cause,
    pub(crate) changes_only: Arc<[String]>,
    pub(crate) pending_writes: Vec<PendingTransactionWrite>,
    pub(crate) command_buffer_len: usize,
    undo: Vec<TransactionUndo>,
}

pub(crate) enum TransactionUndo {
    Component {
        entity: u32,
        component: String,
        previous: Option<ComponentData>,
    },
    Resource {
        resource: String,
        previous: Option<ComponentData>,
    },
    Spawn {
        entity: u32,
        checkpoint: crate::world::TransactionSpawnCheckpoint,
    },
    Despawn {
        entity: u32,
        generation: u32,
        name: Option<String>,
        components: Vec<ComponentData>,
        relations: crate::world::TransactionRelationCheckpoint,
    },
}

impl TransactionUndo {
    fn release(self) {
        match self {
            Self::Component { previous, .. } | Self::Resource { previous, .. } => {
                if let Some(component) = previous {
                    Value::release_component_data(&component);
                }
            }
            Self::Despawn { components, .. } => {
                for component in &components {
                    Value::release_component_data(component);
                }
            }
            Self::Spawn { .. } => {}
        }
    }
}

pub(crate) struct PostCommitContext {
    owner_frame_id: u64,
    owner_chunk_id: usize,
    restore_cause: crate::causality::Cause,
}

impl VM {
    pub(crate) fn enforce_post_commit_opcode(&self, op: crate::opcode::Op) -> Result<(), String> {
        let Some(context) = self.post_commit.as_ref() else {
            return Ok(());
        };
        match crate::bytecode_effects::opcode_policy(
            crate::bytecode_effects::EffectBoundary::PostCommit,
            op,
        ) {
            crate::bytecode_effects::OpcodePolicy::Allow => Ok(()),
            crate::bytecode_effects::OpcodePolicy::Reject(_) => Err(
                "post_commit may perform external effects but cannot mutate authoritative state"
                    .to_string(),
            ),
            crate::bytecode_effects::OpcodePolicy::CheckOwnerFrameExit => {
                let frame = self.current_frame();
                if frame.frame_id == context.owner_frame_id
                    && frame.chunk_id == context.owner_chunk_id
                {
                    Err("post_commit cannot exit before EndPostCommit".to_string())
                } else {
                    Ok(())
                }
            }
        }
    }

    pub(crate) fn enforce_transaction_opcode(&self, op: crate::opcode::Op) -> Result<(), String> {
        let Some(context) = self.transaction.as_ref() else {
            return Ok(());
        };
        use crate::bytecode_effects::{BoundaryViolation, EffectBoundary, OpcodePolicy};
        match crate::bytecode_effects::opcode_policy(EffectBoundary::Transaction, op) {
            OpcodePolicy::Allow => Ok(()),
            OpcodePolicy::CheckOwnerFrameExit => {
                let frame = self.current_frame();
                if frame.frame_id == context.owner_frame_id
                    && frame.chunk_id == context.owner_chunk_id
                {
                    Err(format!(
                        "Transaction '{}' cannot return before its commit boundary (opened at byte {})",
                        context.name, context.begin_ip
                    ))
                } else {
                    Ok(())
                }
            }
            OpcodePolicy::Reject(BoundaryViolation::NestedTransaction) => {
                Err("nested transactions are not allowed".to_string())
            }
            OpcodePolicy::Reject(BoundaryViolation::SettlementKernel) => Err(format!(
                "Transaction '{}' cannot enter the causal settlement kernel",
                context.name
            )),
            OpcodePolicy::Reject(BoundaryViolation::GlobalOrCapturedState) => Err(format!(
                "Transaction '{}' cannot mutate global or captured state",
                context.name
            )),
            OpcodePolicy::Reject(BoundaryViolation::AsyncBeforeCommit) => Err(format!(
                "Transaction '{}' cannot start or await async work before commit",
                context.name
            )),
            OpcodePolicy::Reject(BoundaryViolation::EventBeforeCommit) => Err(format!(
                "Transaction '{}' must emit events from post_commit",
                context.name
            )),
            OpcodePolicy::Reject(BoundaryViolation::ScheduleBeforeCommit) => Err(format!(
                "Transaction '{}' cannot schedule observers before commit",
                context.name
            )),
            OpcodePolicy::Reject(BoundaryViolation::OutputBeforeCommit) => Err(format!(
                "Transaction '{}' must perform output from post_commit",
                context.name
            )),
            OpcodePolicy::Reject(BoundaryViolation::TimelineOrDeclarationState) => Err(format!(
                "Transaction '{}' cannot replace timeline or declaration state",
                context.name
            )),
            OpcodePolicy::Reject(BoundaryViolation::InteriorMutation) => Err(format!(
                "Transaction '{}' cannot perform aliasable interior mutation",
                context.name
            )),
            OpcodePolicy::Reject(BoundaryViolation::VmLifecycleState) => Err(format!(
                "Transaction '{}' cannot mutate handler or VM lifecycle state",
                context.name
            )),
            OpcodePolicy::Reject(
                BoundaryViolation::SettlementEffect(_)
                | BoundaryViolation::AuthoritativeStateMutation,
            ) => Err(format!(
                "Internal VM error: transaction opcode policy returned an invalid boundary decision for {:?}",
                op
            )),
        }
    }

    pub(crate) fn enforce_transaction_builtin(&self, builtin: Builtin) -> Result<(), String> {
        let Some(context) = self.transaction.as_ref() else {
            return Ok(());
        };
        let Some(effect) = crate::bytecode_effects::forbidden_builtin_effect(
            crate::bytecode_effects::EffectBoundary::Transaction,
            builtin,
        ) else {
            return Ok(());
        };
        Err(format!(
            "Transaction '{}' cannot call effectful builtin '{}' before commit ({})",
            context.name,
            builtin.name(),
            effect
        ))
    }

    pub(crate) fn enforce_post_commit_builtin(&self, builtin: Builtin) -> Result<(), String> {
        if self.post_commit.is_none() {
            return Ok(());
        }
        let Some(effect) = crate::bytecode_effects::forbidden_builtin_effect(
            crate::bytecode_effects::EffectBoundary::PostCommit,
            builtin,
        ) else {
            return Ok(());
        };
        Err(format!(
            "post_commit builtin '{}' may mutate authoritative state ({effect})",
            builtin.name(),
        ))
    }

    pub(crate) fn begin_transaction(
        &mut self,
        name: Arc<str>,
        changes_only: Arc<[String]>,
    ) -> Result<(), String> {
        if self.transaction.is_some() || self.post_commit.is_some() {
            return Err("nested transactions are not allowed".to_string());
        }
        if self.settlement.is_some() {
            return Err(
                "an ordinary transaction cannot begin inside a causal settlement".to_string(),
            );
        }
        let (owner_frame_id, owner_chunk_id, begin_ip) = {
            let frame = self.current_frame();
            (frame.frame_id, frame.chunk_id, frame.ip.saturating_sub(1))
        };
        let (effect_parent, restore_cause) =
            transaction_causes(&self.current_cause, self.pending_host_cause.take());
        self.current_cause = crate::causality::Cause::Transaction {
            name: name.as_ref().into(),
            parent: Box::new(effect_parent.clone()),
        };
        self.transaction = Some(TransactionContext {
            name,
            owner_frame_id,
            owner_chunk_id,
            begin_ip,
            effect_parent,
            restore_cause,
            changes_only,
            pending_writes: Vec::new(),
            command_buffer_len: self.command_buffer.len(),
            undo: Vec::new(),
        });
        Ok(())
    }

    pub(crate) fn check_transaction_contract(
        &mut self,
        condition: Value,
        is_ensure: bool,
        label: &str,
    ) -> Result<(), String> {
        let context = self.transaction.as_ref().ok_or_else(|| {
            "transaction contract check without an active transaction".to_string()
        })?;
        let passed = condition.as_bool().ok_or_else(|| {
            format!(
                "Transaction '{}' {label} returned {}; contracts must return bool",
                context.name,
                condition.type_name()
            )
        })?;
        if passed {
            return Ok(());
        }
        let phase = if is_ensure {
            "postcondition"
        } else {
            "precondition"
        };
        Err(format!(
            "Transaction '{}' failed {phase} {label}",
            context.name
        ))
    }

    pub(crate) fn transaction_check_write(&mut self, authority: &str) -> Result<(), String> {
        let Some(context) = self.transaction.as_ref() else {
            return Ok(());
        };
        if context
            .changes_only
            .first()
            .is_some_and(|candidate| candidate == "*")
            || context
                .changes_only
                .binary_search_by(|candidate| candidate.as_str().cmp(authority))
                .is_ok()
        {
            return Ok(());
        }
        Err(format!(
            "Transaction '{}' attempted to change '{}' outside changes_only [{}]",
            context.name,
            authority,
            context
                .changes_only
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    pub(crate) fn transaction_journal_component(&mut self, entity: u32, component: &str) {
        if self.transaction.is_none() {
            return;
        }
        let mut previous = self.world.get_component(entity, component);
        if let Some(previous) = &mut previous {
            Value::persist_component_data(previous);
        }
        self.transaction
            .as_mut()
            .expect("transaction presence checked")
            .undo
            .push(TransactionUndo::Component {
                entity,
                component: component.to_string(),
                previous,
            });
    }

    pub(crate) fn transaction_journal_resource(&mut self, resource: &str) {
        if self.transaction.is_none() {
            return;
        }
        let mut previous = self.world.get_resource(resource);
        if let Some(previous) = &mut previous {
            Value::persist_component_data(previous);
        }
        self.transaction
            .as_mut()
            .expect("transaction presence checked")
            .undo
            .push(TransactionUndo::Resource {
                resource: resource.to_string(),
                previous,
            });
    }

    pub(crate) fn transaction_record_spawn(
        &mut self,
        entity: u32,
        checkpoint: crate::world::TransactionSpawnCheckpoint,
    ) {
        if let Some(transaction) = &mut self.transaction {
            transaction
                .undo
                .push(TransactionUndo::Spawn { entity, checkpoint });
        }
    }

    pub(crate) fn transaction_prepare_despawn(
        &mut self,
        entity: u32,
    ) -> Result<Option<TransactionUndo>, String> {
        if self.transaction.is_none() {
            return Ok(None);
        }
        let entity_ref = self
            .world
            .entity_ref(entity)
            .ok_or_else(|| format!("despawn() called on non-existent entity {entity}"))?;
        let mut components = self.world.components_on_entity(entity);
        for component in &mut components {
            Value::persist_component_data(component);
        }
        Ok(Some(TransactionUndo::Despawn {
            entity,
            generation: entity_ref.generation,
            name: self.world.entity_name(entity),
            components,
            relations: self.world.transaction_relation_checkpoint(),
        }))
    }

    pub(crate) fn transaction_record_despawn(&mut self, undo: Option<TransactionUndo>) {
        if let (Some(transaction), Some(undo)) = (&mut self.transaction, undo) {
            transaction.undo.push(undo);
        }
    }

    pub(crate) fn abort_transaction(&mut self) {
        if let Some(context) = self.transaction.take() {
            for undo in context.undo.into_iter().rev() {
                match undo {
                    TransactionUndo::Component {
                        entity,
                        component,
                        previous,
                    } => match previous {
                        Some(previous) => {
                            assert!(self.world.add_component_owned(entity, previous));
                        }
                        None => {
                            let _ = self.world.remove_component(entity, &component);
                        }
                    },
                    TransactionUndo::Resource { resource, previous } => match previous {
                        Some(previous) => self.world.set_resource_owned(&resource, previous),
                        None => {
                            let _ = self.world.remove_resource(&resource);
                        }
                    },
                    TransactionUndo::Spawn { entity, checkpoint } => {
                        self.world.rollback_transaction_spawn(entity, checkpoint);
                    }
                    TransactionUndo::Despawn {
                        entity,
                        generation,
                        name,
                        components,
                        relations,
                    } => {
                        self.world.restore_transaction_relations(relations);
                        self.world
                            .restore_transaction_despawn(
                                entity,
                                generation,
                                name.as_deref(),
                                components,
                            )
                            .expect("transaction despawn undo must restore its captured entity");
                    }
                }
            }
            self.command_buffer.truncate(context.command_buffer_len);
            self.current_cause = context.restore_cause;
        }
    }

    pub(crate) fn abort_post_commit(&mut self) {
        if let Some(context) = self.post_commit.take() {
            self.current_cause = context.restore_cause;
        }
    }

    /// Discard every effect-scoped execution region at a full host boundary.
    ///
    /// Keep this distinct from `run_frames` error unwinding: the inner runner
    /// intentionally preserves settlement state for the settlement kernel,
    /// while a public run boundary owns and clears the complete region stack.
    /// Centralizing the complete cleanup prevents a newly added region from
    /// being wired into only some host-boundary paths.
    pub(crate) fn abort_effect_regions(&mut self) {
        self.abort_settlement();
        self.abort_transaction();
        self.abort_post_commit();
    }

    /// No public execution boundary may expose an unfinished transaction or
    /// post-commit effect region. A failed transaction restores its exact base
    /// snapshot; a failed post-commit effect leaves the already committed
    /// world intact and restores only the enclosing causal context.
    /// Whether any effect-scoped region is open.
    ///
    /// The three firewalls below each begin by testing their own region, so a
    /// caller that ran all three paid three tests per opcode to learn nothing
    /// in the common case: no region is open at all.
    #[inline(always)]
    fn inside_effect_region(&self) -> bool {
        self.settlement.is_some() || self.transaction.is_some() || self.post_commit.is_some()
    }

    /// Apply every active region's opcode firewall.
    ///
    /// The dispatch loop used to call the settlement, transaction, and
    /// post-commit firewalls in sequence. That is the same shape as the balance
    /// guards before they were fused: correct at today's single call site, but a
    /// second dispatch path that wired up only one or two would silently drop a
    /// firewall rather than fail to compile. One entry point makes a
    /// half-enforced boundary unrepresentable, and short-circuits the common
    /// case in one test instead of three.
    #[inline]
    pub(crate) fn enforce_region_opcode(&self, op: crate::opcode::Op) -> Result<(), String> {
        if !self.inside_effect_region() {
            return Ok(());
        }
        self.enforce_settlement_opcode(op)?;
        self.enforce_transaction_opcode(op)?;
        self.enforce_post_commit_opcode(op)
    }

    /// Apply every active region's builtin firewall. See
    /// [`Self::enforce_region_opcode`].
    #[inline]
    pub(crate) fn enforce_region_builtin(&self, builtin: Builtin) -> Result<(), String> {
        if !self.inside_effect_region() {
            return Ok(());
        }
        self.enforce_settlement_builtin(builtin)?;
        self.enforce_transaction_builtin(builtin)?;
        self.enforce_post_commit_builtin(builtin)
    }

    /// Reject any execution region still open when control leaves the VM.
    ///
    /// Settlement and transaction each had their own guard, and every public
    /// boundary had to remember to run both — one call site even repeated its
    /// `owns_execution_boundary` test once per guard. A new boundary that ran
    /// only one would abort that region and let the other leak across, which is
    /// exactly the bug the guards exist to prevent. Enforcing the whole
    /// invariant in one place makes a half-implemented boundary unrepresentable.
    ///
    /// Regions are reported in nesting order — a settlement encloses the
    /// transaction it is resolving — so the outermost unbalanced region names
    /// the failure.
    pub(crate) fn enforce_region_balance<T, E>(&mut self, result: Result<T, E>) -> Result<T, E>
    where
        E: From<String>,
    {
        let unbalanced = if self.settlement.is_some() {
            Some("settlement at public execution boundary; transaction was aborted")
        } else if self.transaction.is_some() {
            Some("transaction at public execution boundary")
        } else if self.post_commit.is_some() {
            Some("post_commit at public execution boundary")
        } else {
            None
        };
        let Some(region) = unbalanced else {
            return result;
        };
        self.abort_effect_regions();
        match result {
            Ok(_) => Err(format!("Internal VM error: unbalanced {region}").into()),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn finish_transaction(&mut self, has_post_commit: bool) -> Result<(), String> {
        let context = self
            .transaction
            .take()
            .ok_or_else(|| "EndTransaction without an active transaction".to_string())?;
        let frame = self.current_frame();
        if frame.frame_id != context.owner_frame_id || frame.chunk_id != context.owner_chunk_id {
            let message = format!(
                "EndTransaction cannot close transaction '{}' owned by frame {} chunk {} (opened at byte {})",
                context.name,
                context.owner_frame_id,
                context.owner_chunk_id,
                context.begin_ip
            );
            self.transaction = Some(context);
            return Err(message);
        }
        let cause = crate::causality::Cause::Transaction {
            name: context.name.as_ref().into(),
            parent: Box::new(context.effect_parent.clone()),
        };
        // Most transactional services do not declare a materialized view.
        // Building two ordered de-duplication sets and a grouping map for
        // every commit was then pure overhead (and several allocations) even
        // though the refresh routines had no possible consumer. Preserve the
        // exact grouped refresh path when at least one view is installed.
        if self.world.has_materialized_views() {
            let touched_components = context
                .pending_writes
                .iter()
                .filter_map(|write| match (write.entity, &write.view_refresh) {
                    (Some(entity), MaterializedViewRefresh::Component) => {
                        Some((entity, write.component.clone()))
                    }
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            let touched_fields = context
                .pending_writes
                .iter()
                .filter_map(|write| match (write.entity, &write.view_refresh) {
                    (Some(entity), MaterializedViewRefresh::Field(field))
                        if !touched_components.contains(&(entity, write.component.clone())) =>
                    {
                        Some((entity, write.component.clone(), field.clone()))
                    }
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            let mut touched_component_groups = BTreeMap::<u32, Vec<String>>::new();
            for (entity, component) in &touched_components {
                touched_component_groups
                    .entry(*entity)
                    .or_default()
                    .push(component.clone());
            }
            for (entity, components) in touched_component_groups {
                self.world.refresh_materialized_views_for_components(
                    components.iter().map(String::as_str),
                    entity,
                );
            }
            for (entity, component, field) in touched_fields {
                self.world
                    .refresh_materialized_views_for_field(&component, &field, entity);
            }
        }
        for write in context.pending_writes {
            self.ledger
                .record_write(crate::causality::WriteRecord::local(
                    self.causality_frame,
                    write.entity,
                    write.entity_name,
                    write.component,
                    write.summary,
                    write.kind,
                    cause.clone(),
                ));
        }
        for undo in context.undo {
            undo.release();
        }
        if has_post_commit {
            self.current_cause = cause;
            self.post_commit = Some(PostCommitContext {
                owner_frame_id: context.owner_frame_id,
                owner_chunk_id: context.owner_chunk_id,
                restore_cause: context.restore_cause,
            });
        } else {
            self.current_cause = context.restore_cause;
        }
        Ok(())
    }

    pub(crate) fn finish_post_commit(&mut self) -> Result<(), String> {
        let context = self
            .post_commit
            .take()
            .ok_or_else(|| "EndPostCommit without an active post_commit boundary".to_string())?;
        let frame = self.current_frame();
        if frame.frame_id != context.owner_frame_id || frame.chunk_id != context.owner_chunk_id {
            self.post_commit = Some(context);
            return Err("EndPostCommit cannot close a boundary owned by another frame".to_string());
        }
        self.current_cause = context.restore_cause;
        Ok(())
    }
}

#[cfg(test)]
mod cause_tests {
    use super::*;

    fn host(parent: crate::causality::Cause, digest: &str) -> crate::causality::Cause {
        crate::causality::Cause::HostCall {
            extension: "risk.model".into(),
            generation: "17".into(),
            plugin_digest: "plugin".into(),
            export: "score".into(),
            input_digest: digest.into(),
            output_digest: "output".into(),
            parent: Box::new(parent),
        }
    }

    #[test]
    fn one_shot_host_cause_does_not_become_the_next_calls_parent() {
        let current = crate::causality::Cause::Main;
        let first = host(current.clone(), "first");
        let (first_effect, restored) = transaction_causes(&current, Some(first));
        assert!(matches!(
            first_effect,
            crate::causality::Cause::HostCall { .. }
        ));
        assert_eq!(restored, crate::causality::Cause::Main);

        let second = host(restored.clone(), "second");
        let (second_effect, restored_again) = transaction_causes(&restored, Some(second));
        let crate::causality::Cause::HostCall { parent, .. } = second_effect else {
            panic!("second effect must remain a host call");
        };
        assert_eq!(*parent, crate::causality::Cause::Main);
        assert_eq!(restored_again, crate::causality::Cause::Main);
    }
}
