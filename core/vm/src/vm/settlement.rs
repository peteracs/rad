//! RFC-0001 runtime kernel.
//!
//! This module owns the whole causal transaction: proposal capture,
//! canonical grouping, isolated resolver patches, conflict validation, and
//! copy-on-write atomic adoption. The bytecode dispatcher only delegates to
//! these operations.

use super::*;
use crate::value::{ComponentData, Value};
use crate::world::{World, WorldSnapshot};
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
pub(crate) struct Proposal {
    pub(crate) id: u64,
    pub(crate) intent: String,
    pub(crate) key: u32,
    pub(crate) payload: Value,
    pub(crate) canonical: String,
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

#[derive(Clone)]
pub(crate) struct SettlementContext {
    pub(crate) base: WorldSnapshot,
    pub(crate) origin: crate::causality::Cause,
    pub(crate) proposals: Vec<Proposal>,
    pub(crate) patches: Vec<ResolutionPatch>,
    pub(crate) active: Option<ActiveResolution>,
    pub(crate) next_proposal_id: u64,
}

impl VM {
    /// Discard an in-flight causal transaction without touching the live
    /// world or committed provenance ledger.
    ///
    /// Public execution boundaries call this while unwinding an error that
    /// escaped past `BeginSettlement`. Resolver calls deliberately do not own
    /// that boundary: `finish_settlement` remains responsible for aborting
    /// errors raised while the candidate patch is being built.
    pub(crate) fn abort_settlement(&mut self) {
        self.settlement = None;
    }

    /// Enforce the public VM invariant that no execution result can expose an
    /// unfinished settlement. Errors keep their original diagnostic; a
    /// successful escape is a compiler/bytecode fault and becomes an error.
    pub(crate) fn enforce_settlement_balance<T>(
        &mut self,
        result: Result<T, String>,
    ) -> Result<T, String> {
        if self.settlement.is_none() {
            return result;
        }
        self.abort_settlement();
        match result {
            Ok(_) => Err(
                "Internal VM error: unbalanced settlement at public execution boundary; transaction was aborted"
                    .to_string(),
            ),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn begin_settlement(&mut self) -> Result<(), String> {
        if self.settlement.is_some() {
            return Err("nested settlements are not allowed".to_string());
        }
        self.settlement = Some(SettlementContext {
            base: self.world.snapshot(),
            origin: self.current_cause.clone(),
            proposals: Vec::new(),
            patches: Vec::new(),
            active: None,
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
        let canonical = {
            let mut encoded = String::new();
            crate::wire::encode_value_into(&payload, &mut encoded)?;
            encoded
        };
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

    pub(crate) fn finish_settlement(&mut self) -> Result<(), String> {
        let result = self.resolve_and_commit_settlement();
        if result.is_err() {
            self.abort_settlement();
        }
        result
    }

    fn resolve_and_commit_settlement(&mut self) -> Result<(), String> {
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
            if let Err(error) = self.call_value(&callee, vec![key_value, list]) {
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

        // Apply into a CoW candidate world. The live world and ledger remain
        // untouched until every write succeeds.
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
