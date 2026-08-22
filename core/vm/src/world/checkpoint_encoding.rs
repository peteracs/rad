// The operational checkpoint: one canonical, sorted, self-describing byte
// encoding of authoritative world state.

impl WorldSnapshot {

    /// Encode the complete operational world state in one deterministic
    /// inventory. Unlike [`WorldSnapshot::snapshot_json_like`], this is not a
    /// presentation format: it includes allocator/type state, exact storage
    /// topology, derived indexes, queued work, provenance, and observable
    /// rollout metadata because each can change future execution.
    pub(crate) fn encode_operational_checkpoint(&self, out: &mut impl OperationalWorldEncoder) {
        use crate::causality::{Cause, WriteKind};

        fn cause(out: &mut impl OperationalWorldEncoder, input: &Cause) {
            match input {
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
                Cause::Transaction { name, parent } => {
                    out.byte(3);
                    out.text(name);
                    cause(out, parent);
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
                    out.byte(4);
                    out.text(extension);
                    out.text(generation);
                    out.text(plugin_digest);
                    out.text(export);
                    out.text(input_digest);
                    out.text(output_digest);
                    cause(out, parent);
                }
            }
        }

        fn write_kind(out: &mut impl OperationalWorldEncoder, kind: WriteKind) {
            out.byte(match kind {
                WriteKind::Set => 0,
                WriteKind::Spawn => 1,
                WriteKind::Despawn => 2,
                WriteKind::Remove => 3,
                WriteKind::Resource => 4,
            });
        }

        fn component(out: &mut impl OperationalWorldEncoder, data: &ComponentData) {
            out.text(&data.type_name);
            out.usize(data.layout.len());
            for field in data.layout.iter() {
                out.text(field);
            }
            out.usize(data.values.len());
            for value in &data.values {
                out.value(*value);
            }
        }

        fn encode_index_value(out: &mut impl OperationalWorldEncoder, value: &IndexValue) {
            match value {
                IndexValue::Min | IndexValue::Max => {
                    unreachable!("ordered-index sentinels are never stored")
                }
                IndexValue::Int(value) => {
                    out.byte(0);
                    out.i64(*value);
                }
                IndexValue::Str(value) => {
                    out.byte(1);
                    out.text(value);
                }
                IndexValue::Bool(value) => {
                    out.byte(2);
                    out.bool(*value);
                }
                IndexValue::Entity(value) => {
                    out.byte(3);
                    out.u32(*value);
                }
                IndexValue::Float(bits) => {
                    out.byte(4);
                    out.u64(*bits);
                }
                IndexValue::Native(value) => {
                    out.byte(5);
                    out.text(&value.type_name);
                    out.text(&value.repr.to_string());
                    out.text(value.flavor.as_str());
                    out.u64(value.bits);
                }
                IndexValue::Tuple(items) => {
                    out.byte(6);
                    out.usize(items.len());
                    for item in items {
                        encode_index_value(out, item);
                    }
                }
            }
        }

        out.text("rad-operational-world/v5");
        out.u32(self.next_id);
        out.bool(self.fresh_ids_exhausted);
        out.usize(self.free_ids.len());
        for id in self.free_ids.iter() {
            out.u32(*id);
        }
        let mut generations = self.generations.iter().collect::<Vec<_>>();
        generations.sort_unstable_by_key(|(slot, _)| **slot);
        out.usize(generations.len());
        for (slot, generation) in generations {
            out.u32(*slot);
            out.u32(*generation);
        }
        let relation_bytes = self.authoritative_relations.operational_checkpoint_bytes();
        out.usize(relation_bytes.len());
        for byte in relation_bytes {
            out.byte(byte);
        }
        let derived_bytes = self.derived_relations.canonical_bytes();
        out.usize(derived_bytes.len());
        for byte in derived_bytes {
            out.byte(byte);
        }

        let mut names = self.name_to_id.iter().collect::<Vec<_>>();
        names.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(names.len());
        for (name, id) in names {
            out.text(name);
            out.u32(*id);
        }
        let mut ids = self.id_to_name.iter().collect::<Vec<_>>();
        ids.sort_unstable_by_key(|(id, _)| **id);
        out.usize(ids.len());
        for (id, name) in ids {
            out.u32(*id);
            out.text(name);
        }

        let mut types = self.type_registry.iter().collect::<Vec<_>>();
        types.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(types.len());
        for (name, id) in types {
            out.text(name);
            out.u32(*id);
        }
        out.u32(self.next_type_id);

        out.usize(self.archetypes.len());
        for archetype in &self.archetypes {
            out.usize(archetype.type_set.len());
            for type_id in &archetype.type_set {
                out.u32(*type_id);
            }
            out.usize(archetype.entities.len());
            for entity in archetype.entities.iter() {
                out.u32(*entity);
            }
            let mut columns = archetype.columns.iter().collect::<Vec<_>>();
            columns.sort_unstable_by_key(|(type_id, _)| **type_id);
            out.usize(columns.len());
            for (type_id, column) in columns {
                out.u32(*type_id);
                out.text(&column.type_name);
                out.usize(column.layout.len());
                for field in column.layout.iter() {
                    out.text(field);
                }
                out.usize(column.fields.len());
                for values in &column.fields {
                    out.usize(values.len());
                    for value in values.as_slice() {
                        out.value(*value);
                    }
                }
            }
            let mut rows = archetype.entity_row.iter().collect::<Vec<_>>();
            rows.sort_unstable_by_key(|(entity, _)| **entity);
            out.usize(rows.len());
            for (entity, row) in rows {
                out.u32(*entity);
                out.usize(*row);
            }
        }

        let mut archetype_map = self.archetype_map.iter().collect::<Vec<_>>();
        archetype_map.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(archetype_map.len());
        for (types, archetype) in archetype_map {
            out.usize(types.len());
            for type_id in types {
                out.u32(*type_id);
            }
            out.u32(*archetype);
        }
        let mut entity_archetypes = self.entity_archetype.iter().collect::<Vec<_>>();
        entity_archetypes.sort_unstable_by_key(|(entity, _)| **entity);
        out.usize(entity_archetypes.len());
        for (entity, archetype) in entity_archetypes {
            out.u32(*entity);
            out.u32(*archetype);
        }

        let mut indexed_fields = self.indexed_fields.iter().collect::<Vec<_>>();
        indexed_fields.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(indexed_fields.len());
        for (component_name, fields) in indexed_fields {
            out.text(component_name);
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort_unstable();
            out.usize(fields.len());
            for field in fields {
                out.text(field);
            }
        }
        let mut indices = self.indices.iter().collect::<Vec<_>>();
        indices.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(indices.len());
        for (key, entities) in indices {
            out.text(&key.type_name);
            out.text(&key.field_name);
            encode_index_value(out, &key.value);
            out.usize(entities.len());
            for entity in entities {
                out.u32(*entity);
            }
        }

        let mut ordered_fields = self.ordered_fields.iter().collect::<Vec<_>>();
        ordered_fields.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(ordered_fields.len());
        for (component, fields) in ordered_fields {
            out.text(component);
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort_unstable();
            out.usize(fields.len());
            for field in fields {
                out.text(field);
            }
        }
        out.usize(self.ordered_indices.len());
        for (key, entities) in self.ordered_indices.iter() {
            out.text(&key.type_name);
            out.text(&key.field_name);
            encode_index_value(out, &key.value);
            let mut entities = entities.clone();
            entities.sort_unstable();
            out.usize(entities.len());
            for entity in entities {
                out.u32(entity);
            }
        }

        let mut views = self.materialized_views.iter().collect::<Vec<_>>();
        views.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(views.len());
        for (name, view) in views {
            out.text(name);
            out.usize(view.dependencies.len());
            for dependency in &view.dependencies {
                out.text(dependency);
            }
            out.bool(view.key.is_some());
            if let Some((component, field)) = &view.key {
                out.text(component);
                out.text(field);
            }
            out.usize(view.predicate.clauses.len());
            for clause in &view.predicate.clauses {
                use crate::materialized_view::{ViewComparison, ViewPredicateValue};
                out.text(&clause.component);
                out.text(&clause.field);
                out.byte(match clause.comparison {
                    ViewComparison::Eq => 0,
                    ViewComparison::Ne => 1,
                    ViewComparison::Lt => 2,
                    ViewComparison::Le => 3,
                    ViewComparison::Gt => 4,
                    ViewComparison::Ge => 5,
                });
                match &clause.expected {
                    ViewPredicateValue::Int(value) => {
                        out.byte(0);
                        out.i64(*value);
                    }
                    ViewPredicateValue::Float(bits) => {
                        out.byte(1);
                        out.u64(*bits);
                    }
                    ViewPredicateValue::Str(value) => {
                        out.byte(2);
                        out.text(value);
                    }
                    ViewPredicateValue::Bool(value) => {
                        out.byte(3);
                        out.bool(*value);
                    }
                }
            }
            out.u64(view.revision);
            out.usize(view.members.len());
            for entity in &view.members {
                out.u32(*entity);
            }
            out.usize(view.entity_keys.len());
            let mut entity_keys = view.entity_keys.iter().collect::<Vec<_>>();
            entity_keys.sort_unstable_by_key(|entry| *entry.0);
            for (entity, key) in entity_keys {
                out.u32(*entity);
                encode_index_value(out, key);
            }
            out.usize(view.changes.len());
            for change in &view.changes {
                out.u64(change.revision);
                out.u32(change.entity);
                out.bool(change.entered);
                out.text(&change.reason);
            }
            let mut reasons = view.reasons.iter().collect::<Vec<_>>();
            reasons.sort_unstable_by_key(|entry| *entry.0);
            out.usize(reasons.len());
            for (entity, reason) in reasons {
                out.u32(*entity);
                out.text(reason);
            }
        }

        let mut dependents = self.view_dependents.iter().collect::<Vec<_>>();
        dependents.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(dependents.len());
        for (dependency, names) in dependents {
            out.text(dependency);
            out.usize(names.len());
            for name in names {
                out.text(name);
            }
        }

        let mut field_dependents = self.view_field_dependents.iter().collect::<Vec<_>>();
        field_dependents.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(field_dependents.len());
        for (component, fields) in field_dependents {
            out.text(component);
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort_unstable_by(|left, right| left.0.cmp(right.0));
            out.usize(fields.len());
            for (field, names) in fields {
                out.text(field);
                out.usize(names.len());
                for name in names {
                    out.text(name);
                }
            }
        }

        out.usize(self.entered_phases.len());
        for phase in self.entered_phases.iter() {
            out.text(phase);
        }
        let mut lifecycle_traces = self.lifecycle_traces.iter().collect::<Vec<_>>();
        lifecycle_traces.sort_unstable_by_key(|entry| *entry.0);
        out.usize(lifecycle_traces.len());
        for (entity, trace) in lifecycle_traces {
            out.u32(*entity);
            out.usize(trace.len());
            for phase in trace {
                out.text(phase);
            }
        }

        let mut resources = self.resources.iter().collect::<Vec<_>>();
        resources.sort_unstable_by(|left, right| left.0.cmp(right.0));
        out.usize(resources.len());
        for (name, data) in resources {
            out.text(name);
            component(out, data);
        }

        out.usize(self.events.len());
        for (name, payload, trace_id) in self.events.iter() {
            out.text(name);
            out.value(*payload);
            out.u64(*trace_id);
        }
        out.usize(self.emit_ids.len());
        for emit_id in self.emit_ids.iter() {
            out.u64(*emit_id);
        }
        out.usize(self.delayed.len());
        for (delay, name, payload, emit_id) in self.delayed.iter() {
            out.i64(*delay);
            out.text(name);
            out.value(*payload);
            out.u64(*emit_id);
        }

        out.bool(self.provenance.is_some());
        if let Some(provenance) = &self.provenance {
            out.text(&provenance.origin);
            out.usize(provenance.writes.len());
            for write in &provenance.writes {
                out.u64(write.frame);
                out.optional_u32(write.entity);
                out.optional_text(write.entity_name.as_deref());
                out.text(&write.component);
                out.text(&write.value_string());
                out.usize(write.fields().len());
                for (field, value) in write.fields() {
                    out.text(field);
                    out.text(&value.to_string());
                }
                write_kind(out, write.kind);
                cause(out, &write.by);
                out.optional_text(write.origin.as_deref());
                out.optional_u64(write.resolution_id);
            }
            out.usize(provenance.emits.len());
            for emit in &provenance.emits {
                out.u64(emit.id);
                out.text(&emit.event);
                out.u64(emit.frame);
                out.text(&emit.payload);
                cause(out, &emit.by);
                out.optional_text(emit.origin.as_deref());
            }
            out.usize(provenance.settlements.len());
            for settlement in &provenance.settlements {
                out.u64(settlement.id);
                out.u64(settlement.frame);
                cause(out, &settlement.by);
            }
            out.usize(provenance.proposals.len());
            for proposal in &provenance.proposals {
                out.u64(proposal.id);
                out.u64(proposal.settlement_id);
                out.text(&proposal.intent);
                out.u32(proposal.key);
                out.text(&proposal.payload);
                out.text(&proposal.law);
                out.u32(proposal.source_line);
            }
            out.usize(provenance.resolutions.len());
            for resolution in &provenance.resolutions {
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
            out.usize(provenance.relation_assertions.len());
            for assertion in &provenance.relation_assertions {
                out.u64(assertion.frame);
                out.u64(assertion.assertion_id);
                out.text(&crate::relation::runtime::fact_key_transport_hex(
                    &assertion.fact_key,
                ));
                out.usize(assertion.resolution_ids.len());
                for resolution_id in &assertion.resolution_ids {
                    out.u64(*resolution_id);
                }
                out.optional_text(assertion.origin.as_deref());
            }
        }
        // rollout_seed is excluded from content digests and wire snapshots,
        // but included here because fork_seed() makes it observable.
        out.optional_u64(self.rollout_seed);
    }
}
