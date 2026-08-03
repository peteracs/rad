//! RFC-0001 runtime kernel.
//!
//! This module owns the whole causal transaction: proposal capture,
//! canonical grouping, isolated resolver patches, conflict validation, and
//! copy-on-write atomic adoption. The bytecode dispatcher only delegates to
//! these operations.

use super::*;
use crate::value::{ComponentData, Value};
use crate::world::{World, WorldSnapshot};
use sha2::Digest;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

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
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveResolution {
    pub(crate) resolver: String,
    pub(crate) intent: String,
    pub(crate) key: u32,
    pub(crate) proposal_ids: Vec<u64>,
    pub(crate) writes: Vec<CandidateWrite>,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveConstraint {
    pub(crate) identity: crate::constraint_types::ConstraintIdentity,
    pub(crate) subject: u32,
    pub(crate) violations: Vec<crate::constraint_types::ConstraintViolation>,
    pub(crate) overflowed: bool,
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
    pub(crate) active: Option<ActiveResolution>,
    pub(crate) active_constraint: Option<ActiveConstraint>,
    pub(crate) next_proposal_id: u64,
}

impl VM {
    pub(crate) fn enforce_settlement_opcode(&self, op: crate::opcode::Op) -> Result<(), String> {
        if self.settlement.is_none() {
            return Ok(());
        }
        if let crate::bytecode_effects::OpcodeEffect::Forbidden(effect) =
            crate::bytecode_effects::opcode_effect(op)
        {
            return Err(format!(
                "Settlement effect firewall: opcode {:?} cannot perform {} while a settlement is active",
                op, effect
            ));
        }
        Ok(())
    }

    pub(crate) fn enforce_settlement_builtin(&self, builtin: Builtin) -> Result<(), String> {
        if self.settlement.is_none() {
            return Ok(());
        }
        if let Some(effect) = crate::bytecode_effects::forbidden_builtin_effect(builtin) {
            return Err(format!(
                "Settlement effect firewall: builtin `{}` cannot access {} while a settlement is active",
                builtin.name(), effect
            ));
        }
        Ok(())
    }

    pub(crate) fn reject_captured_local_mutation(
        &self,
        slot: usize,
        operation: &str,
    ) -> Result<(), String> {
        if self.settlement.is_none() {
            return Ok(());
        }
        let index = self.current_frame().stack_base.saturating_add(slot);
        if self
            .stack
            .get(index)
            .is_some_and(|value| value.as_cell().is_some())
        {
            return Err(format!(
                "Settlement effect firewall: {} cannot mutate captured local slot {}",
                operation, slot
            ));
        }
        Ok(())
    }

    /// Discard an in-flight causal transaction without touching the live
    /// world or committed provenance ledger.
    ///
    /// Public execution boundaries call this while unwinding an error that
    /// escaped past `BeginSettlement`. Resolver calls deliberately do not own
    /// that boundary: `finish_settlement` remains responsible for aborting
    /// errors raised while the candidate patch is being built.
    pub(crate) fn abort_settlement(&mut self) {
        if let Some(context) = self.settlement.take() {
            // Only one settlement can be active. Its runtime ownership token
            // was never externally committed, so an abort can safely return
            // the allocator to the pre-attempt state as part of atomic unwind.
            self.next_settlement_id = context.settlement_id;
        }
    }

    /// Enforce the public VM invariant that no execution result can expose an
    /// unfinished settlement. Errors keep their original diagnostic; a
    /// successful escape is a compiler/bytecode fault and becomes an error.
    pub(crate) fn enforce_settlement_balance<T, E>(&mut self, result: Result<T, E>) -> Result<T, E>
    where
        E: From<String>,
    {
        if self.settlement.is_none() {
            return result;
        }
        self.abort_settlement();
        match result {
            Ok(_) => Err(
                "Internal VM error: unbalanced settlement at public execution boundary; transaction was aborted"
                    .to_string()
                    .into(),
            ),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn begin_settlement(&mut self) -> Result<(), String> {
        if self.settlement.is_some() {
            return Err("nested settlements are not allowed".to_string());
        }
        let (owner_frame_id, owner_chunk_id, begin_ip) = {
            let frame = self.current_frame();
            (frame.frame_id, frame.chunk_id, frame.ip.saturating_sub(1))
        };
        let settlement_id = self.allocate_settlement_id();
        self.settlement = Some(SettlementContext {
            settlement_id,
            owner_frame_id,
            owner_chunk_id,
            begin_ip,
            base: self.world.snapshot(),
            origin: self.current_cause.clone(),
            proposals: Vec::new(),
            patches: Vec::new(),
            active: None,
            active_constraint: None,
            next_proposal_id: 1,
        });
        Ok(())
    }

    pub(crate) fn propose_intent(
        &mut self,
        intent_name: &str,
        payload: Value,
        source_line: u32,
    ) -> Result<(), String> {
        // A proposal is a transaction-owned immutable value.  The source may
        // alias a constant, global, capture, ECS field, or another local; copy
        // its complete value graph before canonicalizing or retaining it.
        let limits = self.causal_value_limits;
        let payload = payload
            .freeze_causal(&mut self.gc, &limits)
            .map_err(|error| error.to_string())?;
        let info = self
            .intent_registry
            .get(intent_name)
            .cloned()
            .ok_or_else(|| format!("Cannot propose unknown intent '{}'", intent_name))?;
        let data = payload
            .as_component()
            .ok_or_else(|| "propose expected a typed intent payload".to_string())?;
        if data.type_name != info.name || data.layout.as_ref() != info.fields.as_ref() {
            return Err(format!(
                "Intent '{}' payload does not match its declared typed layout",
                intent_name
            ));
        }
        let key_index = data
            .layout
            .iter()
            .position(|field| field == &info.key_field)
            .ok_or_else(|| {
                format!(
                    "Intent '{}' payload is missing key field '{}'",
                    intent_name, info.key_field
                )
            })?;
        let key = data
            .values
            .get(key_index)
            .and_then(Value::as_entity_id)
            .ok_or_else(|| {
                format!(
                    "Intent '{}.{}' must contain an entity key",
                    intent_name, info.key_field
                )
            })?;
        let canonical = crate::canonical_value::internal_bytes(&payload, &limits)
            .map_err(|error| error.to_string())?;
        let law = self.current_chunk().name.clone();
        let context = self.settlement.as_mut().ok_or_else(|| {
            format!(
                "`propose {}` is only valid inside a settlement law",
                intent_name
            )
        })?;
        if context.active.is_some() {
            return Err("Resolvers cannot propose another intent in v0".to_string());
        }
        if context.active_constraint.is_some() {
            return Err("Constraints cannot propose intents".to_string());
        }
        let id = context.next_proposal_id;
        context.next_proposal_id += 1;
        context.proposals.push(Proposal {
            id,
            intent: intent_name.to_string(),
            key,
            payload,
            canonical,
            law,
            source_line,
        });
        Ok(())
    }

    pub(crate) fn stage_candidate(
        &mut self,
        entity: Value,
        component: Value,
    ) -> Result<(), String> {
        let entity = entity
            .as_entity_id()
            .ok_or_else(|| "`next` target must be an entity".to_string())?;
        // Candidate values cross the same capture boundary as proposals.
        // Keep staged state detached from any mutable source alias.
        let limits = self.causal_value_limits;
        let component = component
            .freeze_causal(&mut self.gc, &limits)
            .map_err(|error| error.to_string())?;
        let data = component
            .into_component()
            .ok_or_else(|| "`next` value must be a component".to_string())?;
        self.sandbox_check_write(&data.type_name)?;
        self.sandbox_check_write_shape(&data)?;
        if self.world.get_component(entity, &data.type_name).is_none() {
            return Err(format!(
                "`next` may only replace an existing component: entity {} has no `{}`",
                entity, data.type_name
            ));
        }
        let context = self
            .settlement
            .as_mut()
            .ok_or_else(|| "`next` is only valid while resolving a settlement".to_string())?;
        if context.active_constraint.is_some() {
            return Err("Constraints cannot stage candidate writes".to_string());
        }
        let active = context
            .active
            .as_mut()
            .ok_or_else(|| "`next` is only valid inside a resolver".to_string())?;
        if entity != active.key {
            return Err(format!(
                "resolver `{}` attempted to write entity {}, but its current key is {}",
                active.resolver, entity, active.key
            ));
        }
        if active
            .writes
            .iter()
            .any(|write| write.entity == entity && write.component.type_name == data.type_name)
        {
            return Err(format!(
                "resolver `{}` attempted to stage `{}` more than once for entity {}",
                active.resolver, data.type_name, entity
            ));
        }
        active.writes.push(CandidateWrite {
            entity,
            component: data,
        });
        Ok(())
    }

    pub(crate) fn read_constraint_component(
        &mut self,
        entity: Value,
        component: &str,
        candidate: bool,
    ) -> Result<Value, String> {
        let entity = entity
            .as_entity_id()
            .ok_or_else(|| "constraint read expected an entity subject".to_string())?;
        self.sandbox_check_read(component)?;
        let context = self
            .settlement
            .as_ref()
            .ok_or_else(|| "constraint read requires an active settlement".to_string())?;
        let active = context
            .active_constraint
            .as_ref()
            .ok_or_else(|| "constraint read is only valid while a constraint runs".to_string())?;
        if active.subject != entity {
            return Err(format!(
                "constraint `{}` may only read subject {}",
                active.identity.qualified_name, active.subject
            ));
        }
        let data = if candidate {
            context
                .patches
                .iter()
                .flat_map(|patch| patch.writes.iter())
                .find(|write| write.entity == entity && write.component.type_name == component)
                .map(|write| write.component.clone())
                .or_else(|| context.base.get_component(entity, component))
        } else {
            context.base.get_component(entity, component)
        };
        Ok(match data {
            Some(data) => Value::component(&mut self.gc, data.type_name, data.layout, data.values),
            None => Value::NIL,
        })
    }

    pub(crate) fn require_constraint(
        &mut self,
        condition: Value,
        code: String,
        source_line: u32,
    ) -> Result<(), String> {
        let valid = condition
            .as_bool()
            .ok_or_else(|| "constraint requirement must evaluate to bool".to_string())?;
        if valid {
            return Ok(());
        }
        let (subject, attached_component) = self
            .settlement
            .as_ref()
            .and_then(|context| context.active_constraint.as_ref())
            .map(|active| (active.subject, active.identity.attached_component.clone()))
            .ok_or_else(|| "`require` is only valid while a constraint runs".to_string())?;
        let per_invocation_limit = self
            .constraint_limit_profile
            .max_violations_per_invocation();
        let active = self
            .settlement
            .as_mut()
            .and_then(|context| context.active_constraint.as_mut())
            .ok_or_else(|| "`require` is only valid while a constraint runs".to_string())?;
        if active.violations.len() >= per_invocation_limit {
            active.overflowed = true;
            return Ok(());
        }
        let occurrence = active
            .violations
            .iter()
            .filter(|violation| violation.code == code)
            .count() as u32
            + 1;
        active
            .violations
            .push(crate::constraint_types::ConstraintViolation {
                constraint: active.identity.clone(),
                subject: active.subject,
                code,
                occurrence,
                source_line,
                candidate: crate::constraint_types::CandidateKey {
                    entity: subject,
                    component: attached_component,
                },
            });
        Ok(())
    }

    fn candidate_component(
        context: &SettlementContext,
        entity: u32,
        component: &str,
    ) -> Option<ComponentData> {
        context
            .patches
            .iter()
            .flat_map(|patch| patch.writes.iter())
            .find(|write| write.entity == entity && write.component.type_name == component)
            .map(|write| write.component.clone())
            .or_else(|| context.base.get_component(entity, component))
    }

    pub(crate) fn constraint_capabilities(
        &self,
    ) -> crate::constraint_types::RejectionCapabilityMetadata {
        match &self.sandbox_caps {
            Some(capabilities) => {
                let readable_components = capabilities
                    .readable_components
                    .iter()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                let profile_id = format!(
                    "sandbox:{}",
                    hex::encode(sha2::Sha256::digest(
                        readable_components
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\0")
                            .as_bytes()
                    ))
                );
                crate::constraint_types::RejectionCapabilityMetadata {
                    profile_id,
                    origins_visible: capabilities.may_read_all(),
                    readable_components,
                }
            }
            None => crate::constraint_types::RejectionCapabilityMetadata {
                profile_id: "trusted".into(),
                readable_components: std::collections::BTreeSet::from(["*".into()]),
                origins_visible: true,
            },
        }
    }

    fn evaluate_candidate_constraints(&mut self) -> crate::constraint_types::ValidationResult {
        use crate::constraint_types::{
            CandidateCausalExplanation, ConstraintEvaluationFailure, ConstraintIdentity,
            ConstraintViolation, EphemeralCausalExplanation, HostFault, RejectionEncodingError,
            RejectionProposalOrigin, RejectionValue, SettlementRejection, ValidationResult,
        };

        let staged = self
            .settlement
            .as_ref()
            .expect("constraint phase requires settlement")
            .patches
            .iter()
            .flat_map(|patch| patch.writes.iter())
            .map(|write| (write.entity, write.component.type_name.clone()))
            .collect::<std::collections::BTreeSet<_>>();
        let registry = self.constraint_registry.as_ref().clone();
        let mut selected = std::collections::BTreeSet::new();
        for constraint in &registry {
            for (entity, component) in &staged {
                if component != &constraint.attached_component
                    && !constraint.watches.contains(component)
                {
                    continue;
                }
                let exists = Self::candidate_component(
                    self.settlement.as_ref().unwrap(),
                    *entity,
                    &constraint.attached_component,
                )
                .is_some();
                if exists {
                    selected.insert((constraint.name.clone(), *entity));
                }
            }
        }

        let by_name = registry
            .into_iter()
            .map(|constraint| (constraint.name.clone(), constraint))
            .collect::<BTreeMap<_, _>>();
        let invocation_count = selected.len();
        let required_fuel = (invocation_count as u64)
            .checked_mul(self.constraint_limit_profile.fuel_per_invocation());
        let required_heap = invocation_count.checked_mul(
            self.constraint_limit_profile
                .max_heap_bytes_per_invocation(),
        );
        if required_fuel
            .is_none_or(|required| required > self.constraint_limit_profile.max_aggregate_fuel())
            || required_heap.is_none_or(|required| {
                required > self.constraint_limit_profile.max_aggregate_heap_bytes()
            })
        {
            return ValidationResult::HostAborted(HostFault {
                code: "constraint.aggregate_budget_unavailable".into(),
                message: format!(
                    "{} selected constraint invocation(s) exceed the configured aggregate host envelope",
                    invocation_count
                ),
            });
        }
        let mut violations: Vec<ConstraintViolation> = Vec::new();
        let mut evaluation_failures = Vec::new();
        for (name, subject) in &selected {
            let constraint = by_name
                .get(name)
                .expect("selected constraint must be registered");
            let identity = ConstraintIdentity {
                qualified_name: constraint.name.clone(),
                attached_component: constraint.attached_component.clone(),
            };
            let Some(proposed) = Self::candidate_component(
                self.settlement.as_ref().unwrap(),
                *subject,
                &constraint.attached_component,
            ) else {
                continue;
            };
            let Some(callee) = self.globals.get(constraint.global_slot as usize).copied() else {
                evaluation_failures.push(ConstraintEvaluationFailure {
                    constraint: identity,
                    subject: *subject,
                    code: "constraint.not_initialized".into(),
                    message: "compiled constraint global is missing".into(),
                    source_line: 0,
                });
                continue;
            };
            self.settlement.as_mut().unwrap().active_constraint = Some(ActiveConstraint {
                identity: identity.clone(),
                subject: *subject,
                violations: Vec::new(),
                overflowed: false,
            });
            let subject_value = Value::from_entity_id(&mut self.gc, *subject);
            let proposed_value = Value::component(
                &mut self.gc,
                proposed.type_name,
                proposed.layout,
                proposed.values,
            );
            let frame_depth = self.frames.len();
            let stack_depth = self.stack.len();
            let next_frame_id = self.next_frame_id;
            let saved_fuel = self.fuel;
            let saved_mem_limit = self.mem_limit;
            let invocation_fuel = self.constraint_limit_profile.fuel_per_invocation();
            self.fuel = invocation_fuel;
            self.mem_limit = self.gc.bytes_allocated().saturating_add(
                self.constraint_limit_profile
                    .max_heap_bytes_per_invocation(),
            );
            let result = self.call_value(&callee, vec![subject_value, proposed_value]);
            self.fuel = saved_fuel;
            self.mem_limit = saved_mem_limit;
            if result.is_err() {
                self.frames.truncate(frame_depth);
                self.stack.truncate(stack_depth);
                self.next_frame_id = next_frame_id;
            }
            let active = self
                .settlement
                .as_mut()
                .and_then(|context| context.active_constraint.take())
                .expect("constraint invocation must retain its active context");
            match result {
                Ok(_) if active.overflowed => {
                    evaluation_failures.push(ConstraintEvaluationFailure {
                        constraint: identity,
                        subject: *subject,
                        code: "constraint.invocation_violation_limit".into(),
                        message: format!(
                            "constraint exceeded {} violations",
                            self.constraint_limit_profile
                                .max_violations_per_invocation()
                        ),
                        source_line: 0,
                    });
                }
                Ok(_) => violations.extend(active.violations),
                Err(error) => {
                    evaluation_failures.push(ConstraintEvaluationFailure {
                        constraint: identity,
                        subject: *subject,
                        code: if error.contains("memory limit") {
                            "constraint.memory_exhausted".into()
                        } else if error.contains("fuel") || error.contains("Budget exhausted") {
                            "constraint.fuel_exhausted".into()
                        } else {
                            "constraint.evaluation_failed".into()
                        },
                        message: error,
                        source_line: 0,
                    });
                }
            }
        }

        if violations.len()
            > self
                .constraint_limit_profile
                .max_violations_per_settlement()
        {
            violations.clear();
            evaluation_failures.push(ConstraintEvaluationFailure {
                constraint: ConstraintIdentity {
                    qualified_name: "<settlement>".into(),
                    attached_component: "<all>".into(),
                },
                subject: 0,
                code: "constraint.settlement_violation_limit".into(),
                message: format!(
                    "settlement exceeded {} violations",
                    self.constraint_limit_profile
                        .max_violations_per_settlement()
                ),
                source_line: 0,
            });
        }
        if violations.is_empty() && evaluation_failures.is_empty() {
            return ValidationResult::Accepted;
        }

        let context = self.settlement.as_ref().unwrap();
        let mut base_world = World::new();
        base_world.restore(context.base.clone());
        let base_world_digest = base_world.content_digest();
        let capabilities = self.constraint_capabilities();
        let applicable_constraints = selected
            .iter()
            .filter_map(|(name, _)| by_name.get(name))
            .map(|constraint| ConstraintIdentity {
                qualified_name: constraint.name.clone(),
                attached_component: constraint.attached_component.clone(),
            })
            .collect::<Vec<_>>();
        let bounded_rejection = |code: &str, message: String| {
            let mut bounded = SettlementRejection {
                settlement_id: context.settlement_id,
                base_world_digest: base_world_digest.clone(),
                // The fallback is a fixed-size envelope. Keeping the full
                // registry or capability set here could make the value that
                // reports an output-limit failure exceed that same limit.
                applicable_constraints: Vec::new(),
                violations: Vec::new(),
                evaluation_failures: vec![ConstraintEvaluationFailure {
                    constraint: ConstraintIdentity {
                        qualified_name: "<settlement>".into(),
                        attached_component: "<all>".into(),
                    },
                    subject: 0,
                    code: code.into(),
                    message,
                    source_line: 0,
                }],
                candidate_details: BTreeMap::new(),
                explanation: EphemeralCausalExplanation::default(),
                limit_profile_fingerprint: self.constraint_limit_profile.fingerprint(),
                capabilities: crate::constraint_types::RejectionCapabilityMetadata {
                    profile_id: capabilities.profile_id.clone(),
                    readable_components: std::collections::BTreeSet::new(),
                    origins_visible: false,
                },
            };
            bounded.canonicalize();
            std::sync::Arc::new(bounded)
        };

        // Candidate values are exported at most once per key. Preflight the
        // raw component encoding against a bounded share of the final output
        // before allocating a pointer-free FrozenValue tree.
        let candidate_keys = violations
            .iter()
            .map(|violation| violation.candidate.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let detail_budget = self.constraint_limit_profile.max_serialized_outcome_bytes() / 2;
        let mut detail_bytes = 0usize;
        let mut candidate_details = BTreeMap::new();
        for key in &candidate_keys {
            let readable = capabilities.readable_components.contains("*")
                || capabilities.readable_components.contains(&key.component);
            if !readable {
                candidate_details.insert(key.clone(), RejectionValue::Redacted);
                continue;
            }
            let remaining = detail_budget.saturating_sub(detail_bytes);
            if remaining == 0 {
                return ValidationResult::Rejected(bounded_rejection(
                    "constraint.outcome_byte_limit",
                    "candidate detail table exceeds its bounded output arena".into(),
                ));
            }
            let limits = match self
                .constraint_limit_profile
                .value_limits()
                .with_max_encoded_bytes(
                    remaining.min(
                        self.constraint_limit_profile
                            .value_limits()
                            .max_encoded_bytes(),
                    ),
                ) {
                Ok(limits) => limits,
                Err(error) => {
                    return ValidationResult::HostAborted(HostFault {
                        code: "constraint.invalid_value_profile".into(),
                        message: error.to_string(),
                    })
                }
            };
            let Some(component) = Self::candidate_component(context, key.entity, &key.component)
            else {
                continue;
            };
            let encoded = match crate::canonical_value::component_bytes(&component, &limits) {
                Ok(encoded) => encoded,
                Err(crate::CausalValueError::EncodedByteLimit { .. }) => {
                    return ValidationResult::Rejected(bounded_rejection(
                        "constraint.outcome_byte_limit",
                        "candidate detail table exceeds its bounded output arena".into(),
                    ))
                }
                Err(error) => {
                    return ValidationResult::HostAborted(HostFault {
                        code: "constraint.rejection_value_invalid".into(),
                        message: error.to_string(),
                    })
                }
            };
            detail_bytes = detail_bytes.saturating_add(encoded.len());
            let value = match crate::host_value::export_component_data(component, &limits) {
                Ok(value) => value,
                Err(error) => {
                    return ValidationResult::HostAborted(HostFault {
                        code: "constraint.rejection_value_invalid".into(),
                        message: error.to_string(),
                    })
                }
            };
            candidate_details.insert(key.clone(), RejectionValue::Visible(value));
        }

        // Follow each rejected candidate to the one resolution patch that
        // staged it, then include only that patch's proposal fan-in.
        let proposal_by_id = context
            .proposals
            .iter()
            .map(|proposal| (proposal.id, proposal))
            .collect::<BTreeMap<_, _>>();
        let mut origin_bytes = 0usize;
        let origin_budget = self.constraint_limit_profile.max_serialized_outcome_bytes() / 2;
        let mut explanation = EphemeralCausalExplanation::default();
        for key in &candidate_keys {
            let Some(patch) = context.patches.iter().find(|patch| {
                patch.writes.iter().any(|write| {
                    write.entity == key.entity && write.component.type_name == key.component
                })
            }) else {
                continue;
            };
            if patch.proposal_ids.len()
                > self
                    .constraint_limit_profile
                    .max_violations_per_invocation()
            {
                return ValidationResult::Rejected(bounded_rejection(
                    "constraint.outcome_origin_limit",
                    format!(
                        "candidate origin fan-in exceeds {} records",
                        self.constraint_limit_profile
                            .max_violations_per_invocation()
                    ),
                ));
            }
            let mut origins = Vec::with_capacity(patch.proposal_ids.len());
            for proposal_id in &patch.proposal_ids {
                let Some(proposal) = proposal_by_id.get(proposal_id) else {
                    continue;
                };
                if !capabilities.origins_visible {
                    origins.push(RejectionProposalOrigin::Redacted);
                    continue;
                }
                if proposal.canonical.len() > origin_budget.saturating_sub(origin_bytes) {
                    return ValidationResult::Rejected(bounded_rejection(
                        "constraint.outcome_byte_limit",
                        "candidate origin table exceeds its bounded output arena".into(),
                    ));
                }
                origin_bytes = origin_bytes.saturating_add(proposal.canonical.len());
                let payload = match crate::host_value::export_value(&proposal.payload) {
                    Ok(payload) => payload,
                    Err(error) => {
                        return ValidationResult::HostAborted(HostFault {
                            code: "constraint.rejection_origin_invalid".into(),
                            message: error.to_string(),
                        })
                    }
                };
                origins.push(RejectionProposalOrigin::Visible {
                    law: proposal.law.clone(),
                    source_line: proposal.source_line,
                    payload,
                });
            }
            explanation.candidates.insert(
                key.clone(),
                CandidateCausalExplanation {
                    resolver: if capabilities.origins_visible {
                        patch.resolver.clone()
                    } else {
                        "<redacted-origin>".into()
                    },
                    intent: if capabilities.origins_visible {
                        patch.intent.clone()
                    } else {
                        "<redacted-origin>".into()
                    },
                    intent_key: patch.key,
                    proposal_origins: origins,
                },
            );
        }

        let mut rejection = SettlementRejection {
            settlement_id: context.settlement_id,
            base_world_digest: base_world_digest.clone(),
            applicable_constraints: applicable_constraints.clone(),
            violations,
            evaluation_failures,
            candidate_details,
            explanation,
            limit_profile_fingerprint: self.constraint_limit_profile.fingerprint(),
            capabilities: capabilities.clone(),
        };
        rejection.canonicalize();
        match rejection.canonical_bytes(&self.constraint_limit_profile) {
            Ok(_) => ValidationResult::Rejected(std::sync::Arc::new(rejection)),
            Err(RejectionEncodingError::OutcomeByteLimit { .. }) => {
                ValidationResult::Rejected(bounded_rejection(
                    "constraint.outcome_byte_limit",
                    format!(
                        "canonical rejection exceeds the {}-byte limit",
                        self.constraint_limit_profile.max_serialized_outcome_bytes()
                    ),
                ))
            }
            Err(error) => ValidationResult::HostAborted(HostFault {
                code: "constraint.rejection_encoding_failed".into(),
                message: error.to_string(),
            }),
        }
    }

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

        let _ = context;
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

        // Apply into a CoW candidate world. The live world and ledger remain
        // untouched until every write succeeds.
        let context = self.settlement.as_ref().unwrap();
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

        let committed = candidate.snapshot();
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
    }
}
