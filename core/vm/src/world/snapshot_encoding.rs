pub(crate) enum EntitySelectionError {
    LimitExceeded { actual: usize },
    AllocationFailed,
}

impl WorldSnapshot {
    pub(crate) fn relation_state(
        &self,
    ) -> &crate::relation_runtime::AuthoritativeRelationState {
        &self.authoritative_relations
    }

    pub(crate) fn derived_relation_state(
        &self,
    ) -> &crate::relation_derivation::DerivedRelationState {
        &self.derived_relations
    }

    pub(crate) fn entity_ref(
        &self,
        entity: u32,
    ) -> Option<crate::relation_runtime::EntityRef> {
        self.entity_archetype
            .contains_key(&entity)
            .then(|| crate::relation_runtime::EntityRef {
                slot: entity,
                generation: self.generations.get(&entity).copied().unwrap_or(0),
            })
    }

    /// Encode the complete operational world state in one deterministic
    /// inventory. Unlike [`WorldSnapshot::snapshot_json_like`], this is not a
    /// presentation format: it includes allocator/type state, exact storage
    /// topology, derived indexes, queued work, provenance, and observable
    /// rollout metadata because each can change future execution.
    pub(crate) fn encode_operational_checkpoint(&self, out: &mut impl OperationalWorldEncoder) {
        use crate::causality::{Cause, WriteKind};

        fn cause(out: &mut impl OperationalWorldEncoder, cause: &Cause) {
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

        out.text("rad-operational-world/v3");
        out.u32(self.next_id);
        out.bool(self.fresh_ids_exhausted);
        out.usize(self.free_ids.len());
        for id in self.free_ids.iter() {
            out.u32(*id);
        }
        let mut generations = self.generations.iter().collect::<Vec<_>>();
        generations.sort_by_key(|(slot, _)| **slot);
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
        names.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(names.len());
        for (name, id) in names {
            out.text(name);
            out.u32(*id);
        }
        let mut ids = self.id_to_name.iter().collect::<Vec<_>>();
        ids.sort_by_key(|(id, _)| **id);
        out.usize(ids.len());
        for (id, name) in ids {
            out.u32(*id);
            out.text(name);
        }

        let mut types = self.type_registry.iter().collect::<Vec<_>>();
        types.sort_by(|left, right| left.0.cmp(right.0));
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
            columns.sort_by_key(|(type_id, _)| **type_id);
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
            rows.sort_by_key(|(entity, _)| **entity);
            out.usize(rows.len());
            for (entity, row) in rows {
                out.u32(*entity);
                out.usize(*row);
            }
        }

        let mut archetype_map = self.archetype_map.iter().collect::<Vec<_>>();
        archetype_map.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(archetype_map.len());
        for (types, archetype) in archetype_map {
            out.usize(types.len());
            for type_id in types {
                out.u32(*type_id);
            }
            out.u32(*archetype);
        }
        let mut entity_archetypes = self.entity_archetype.iter().collect::<Vec<_>>();
        entity_archetypes.sort_by_key(|(entity, _)| **entity);
        out.usize(entity_archetypes.len());
        for (entity, archetype) in entity_archetypes {
            out.u32(*entity);
            out.u32(*archetype);
        }

        let mut indexed_fields = self.indexed_fields.iter().collect::<Vec<_>>();
        indexed_fields.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(indexed_fields.len());
        for (component_name, fields) in indexed_fields {
            out.text(component_name);
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort();
            out.usize(fields.len());
            for field in fields {
                out.text(field);
            }
        }
        let mut indices = self.indices.iter().collect::<Vec<_>>();
        indices.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(indices.len());
        for (key, entities) in indices {
            out.text(&key.type_name);
            out.text(&key.field_name);
            match &key.value {
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
            }
            out.usize(entities.len());
            for entity in entities {
                out.u32(*entity);
            }
        }

        let mut resources = self.resources.iter().collect::<Vec<_>>();
        resources.sort_by(|left, right| left.0.cmp(right.0));
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
                out.text(&write.value);
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
                out.text(&crate::relation_runtime::fact_key_transport_hex(
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

    /// Return a copy of this snapshot with resource `name` set to `data`,
    /// leaving entities, in-flight events, delayed timers, and provenance
    /// untouched. Backs `fork_with`: seed a speculative candidate off a fork
    /// without committing to (mutating) the live world (dogfood feature seq
    /// 150). Copy-on-write — only the resource map's `Arc` is cloned.
    pub(crate) fn with_resource(&self, name: &str, mut data: ComponentData) -> WorldSnapshot {
        Value::persist_component_data(&mut data);
        let mut snap = self.clone();
        Arc::make_mut(&mut snap.resources).insert_owned(name.to_string(), data);
        // The override makes this a fresh candidate, not the output of the
        // rollout that (possibly) produced `self` — `fork_seed()` on it is 0.
        snap.rollout_seed = None;
        snap
    }

    /// Renderer/inspector dump of a frozen frame — same shape as
    /// `World::snapshot_json_like`, so RADSCOPE scrubs timelines with the
    /// exact code that renders live worlds.
    pub fn snapshot_json_like(&self) -> String {
        let mut ids: Vec<u32> = self.entity_archetype.keys().copied().collect();
        ids.sort_unstable();
        let mut res_names: Vec<String> = self.resources.keys().cloned().collect();
        res_names.sort();
        dump_world_json(
            &ids,
            |eid| self.id_to_name.get(&eid).cloned(),
            |eid| {
                let Some(&aid) = self.entity_archetype.get(&eid) else {
                    return Vec::new();
                };
                let arch = &self.archetypes[aid as usize];
                let Some(&row) = arch.entity_row.get(&eid) else {
                    return Vec::new();
                };
                let mut out = Vec::new();
                for &tid in &arch.type_set {
                    if let Some(col) = arch.columns.get(&tid) {
                        out.push(col.get(row));
                    }
                }
                out
            },
            &res_names,
            |name| self.resources.get(name).cloned(),
        )
    }

    /// Resolve a named entity within this frozen frame.
    pub fn entity_id_by_name(&self, name: &str) -> Option<u32> {
        self.name_to_id.get(name).copied()
    }

    /// Components of one entity in this frozen frame.
    pub(crate) fn components_of(&self, eid: u32) -> Vec<ComponentData> {
        let Some(&aid) = self.entity_archetype.get(&eid) else {
            return Vec::new();
        };
        let arch = &self.archetypes[aid as usize];
        let Some(&row) = arch.entity_row.get(&eid) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for &tid in &arch.type_set {
            if let Some(col) = arch.columns.get(&tid) {
                out.push(col.get(row));
            }
        }
        out
    }

    pub fn sorted_entity_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.entity_archetype.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub(crate) fn collect_sorted_entity_ids_with_components(
        &self,
        ctypes: &[&str],
        max_entities: usize,
        output: &mut Vec<u32>,
    ) -> Result<(), EntitySelectionError> {
        output.clear();
        let matches = |archetype: &Archetype| {
            ctypes.iter().all(|name| {
                self.type_registry
                    .get(*name)
                    .is_some_and(|type_id| archetype.columns.contains_key(type_id))
            })
        };
        let mut count = 0usize;
        for archetype in &self.archetypes {
            if matches(archetype) {
                count = count
                    .checked_add(archetype.entities.len())
                    .ok_or(EntitySelectionError::LimitExceeded { actual: usize::MAX })?;
            }
        }
        if count > max_entities {
            return Err(EntitySelectionError::LimitExceeded { actual: count });
        }
        output
            .try_reserve(count)
            .map_err(|_| EntitySelectionError::AllocationFailed)?;
        for archetype in &self.archetypes {
            if matches(archetype) {
                output.extend(archetype.entities.iter().copied());
            }
        }
        output.sort_unstable();
        Ok(())
    }

    /// Renderer-facing INCREMENTAL dump: what changed since `prev`.
    /// `{"upsert":[entity rows],"remove":[ids],"resources":{changed only}}`
    /// — the fix for the full-world-JSON-per-keystroke firehose.
    ///
    /// Change detection rides the CoW structure: archetypes whose entity
    /// list and column-field Arcs are pointer-equal are skipped whole (the
    /// overwhelmingly common case); only rows in actually-written columns
    /// get value compares; only changed entities get serialized. No
    /// per-entity ComponentData clones on the compare path.
    pub fn render_delta_json(&self, prev: &WorldSnapshot) -> String {
        use std::fmt::Write;

        fn comp_eq(a: &ComponentData, b: &ComponentData) -> bool {
            if a.type_name != b.type_name || a.values.len() != b.values.len() {
                return false;
            }
            if !Arc::ptr_eq(&a.layout, &b.layout) && a.layout != b.layout {
                return false;
            }
            a.values.iter().zip(b.values.iter()).all(|(x, y)| x == y)
        }

        let mut upserts: Vec<u32> = Vec::new();

        // positional alignment holds for shared-lineage snapshots (these
        // are successive forks of one live world); if it ever doesn't,
        // fall back to treating every current entity as changed-checkable
        // via the slow row lookup below.
        for (i, arch) in self.archetypes.iter().enumerate() {
            let base = prev.archetypes.get(i);
            let Some(base) = base else {
                // brand-new archetype: every entity in it is an upsert
                upserts.extend(arch.entities.iter().copied());
                continue;
            };
            // O(1) skip: same rows, same column data
            let identical = Arc::ptr_eq(&arch.entities, &base.entities)
                && arch.columns.len() == base.columns.len()
                && arch.columns.iter().all(|(tid, col)| {
                    base.columns.get(tid).is_some_and(|bcol| {
                        col.fields.len() == bcol.fields.len()
                            && col
                                .fields
                                .iter()
                                .zip(&bcol.fields)
                                .all(|(a, b)| Arc::ptr_eq(a, b))
                    })
                });
            if identical {
                continue;
            }
            // something in this archetype was written: row-level check
            for (r, &eid) in arch.entities.iter().enumerate() {
                let Some(&rb) = base.entity_row.get(&eid) else {
                    upserts.push(eid); // entered this archetype since prev
                    continue;
                };
                let mut changed = false;
                for (tid, col) in &arch.columns {
                    let Some(bcol) = base.columns.get(tid) else {
                        changed = true;
                        break;
                    };
                    for (fa, fb) in col.fields.iter().zip(&bcol.fields) {
                        if Arc::ptr_eq(fa, fb) && r == rb {
                            continue;
                        }
                        match (fa.as_slice().get(r), fb.as_slice().get(rb)) {
                            (Some(a), Some(b)) => {
                                if a != b {
                                    changed = true;
                                    break;
                                }
                            }
                            _ => {
                                changed = true;
                                break;
                            }
                        }
                    }
                    if changed {
                        break;
                    }
                }
                if changed {
                    upserts.push(eid);
                }
            }
        }
        // entities whose archetype VANISHED (despawn shrank the vec) are
        // covered by `remove` below; entities that MOVED archetypes were
        // caught by the entity_row miss in their new home.
        upserts.sort_unstable();
        upserts.dedup();

        let mut removed: Vec<u32> = prev
            .entity_archetype
            .keys()
            .filter(|eid| !self.entity_archetype.contains_key(eid))
            .copied()
            .collect();
        removed.sort_unstable();

        let mut res_names: Vec<String> = self.resources.keys().cloned().collect();
        res_names.sort();

        // reuse the entity-row encoding from the full dump
        let upsert_json = dump_world_json(
            &upserts,
            |eid| self.id_to_name.get(&eid).cloned(),
            |eid| self.components_of(eid),
            &[],
            |_| None,
        );
        // dump_world_json returns {"entities":[...],"resources":{}} — splice
        let entities_part = upsert_json
            .strip_prefix("{\"entities\":")
            .and_then(|s| s.strip_suffix(",\"resources\":{}}"))
            .unwrap_or("[]")
            .to_string();

        let mut s = String::from("{\"upsert\":");
        s.push_str(&entities_part);
        s.push_str(",\"remove\":[");
        for (i, eid) in removed.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(&mut s, "{}", eid);
        }
        s.push_str("],\"resources\":{");
        let mut first = true;
        for name in &res_names {
            let (Some(cur), prevr) = (self.resources.get(name), prev.resources.get(name)) else {
                continue;
            };
            let changed = match prevr {
                Some(p) => !comp_eq(cur, p),
                None => true,
            };
            if changed {
                if !first {
                    s.push(',');
                }
                first = false;
                let _ = write!(&mut s, "\"{}\":{}", name, resource_fields_json(cur));
            }
        }
        s.push_str("}}");
        s
    }

    pub fn trace(&self, marked: &mut HashSet<usize>) {
        for (_, payload, _) in self.events.iter() {
            payload.trace(marked);
        }
        for (_, _, payload, _) in self.delayed.iter() {
            payload.trace(marked);
        }
        for archetype in &self.archetypes {
            for col in archetype.columns.values() {
                col.trace(marked);
            }
        }
        for res in self.resources.values() {
            for val in &res.values {
                val.trace(marked);
            }
        }
    }

    pub(crate) fn get_component(&self, eid: u32, ctype: &str) -> Option<ComponentData> {
        let &aid = self.entity_archetype.get(&eid)?;
        let tid = self.type_registry.get(ctype).copied()?;
        self.archetypes[aid as usize].get_component(eid, tid)
    }

    pub(crate) fn component_view(&self, eid: u32, ctype: &str) -> Option<ComponentView<'_>> {
        let &aid = self.entity_archetype.get(&eid)?;
        let tid = self.type_registry.get(ctype).copied()?;
        let archetype = &self.archetypes[aid as usize];
        let &row = archetype.entity_row.get(&eid)?;
        let column = archetype.columns.get(&tid)?;
        Some(ComponentView::new(column, row))
    }

    pub(crate) fn get_resource(&self, name: &str) -> Option<ComponentData> {
        self.resources.get(name).cloned()
    }

    /// Resolve a named entity to its id.
    pub fn get_entity_by_name(&self, name: &str) -> Option<u32> {
        self.name_to_id.get(name).copied()
    }

    /// Cheap per-component change summary between two snapshots: component
    /// type name → changed-row count (an upper bound).
    ///
    /// Cost is O(archetypes × columns) `Arc::ptr_eq` comparisons — the CoW
    /// architecture means an untouched column shares its `Arc`ed field
    /// vectors with the base snapshot, so "what changed" is a pointer
    /// comparison, not a scan. When a column's pointer differs we report its
    /// row count (we know *that* it changed, not *which* rows). Resources
    /// count 1 per changed entry.
    ///
    /// Archetypes are paired by index: `restore()` preserves vector order and
    /// mutation only appends, so index i refers to the same archetype in both
    /// snapshots whenever both have one.
    pub fn diff_summary(
        base: &WorldSnapshot,
        new: &WorldSnapshot,
    ) -> std::collections::BTreeMap<String, usize> {
        // The positional fast path below matches archetype i to archetype i,
        // row r to row r — only meaningful for snapshots with shared lineage
        // (CoW forks of one world agree on archetype order and row layout).
        // Worlds rebuilt from scratch — merge results, wire-decoded forks,
        // retro replays — order archetypes and rows differently; positional
        // comparison produces phantom diffs. TypeIds are world-local, so the
        // check compares column type *names* and the row→entity assignment;
        // anything misaligned takes the O(world) semantic path instead.
        let aligned = base
            .archetypes
            .iter()
            .zip(new.archetypes.iter())
            .all(|(a, b)| {
                if !(Arc::ptr_eq(&a.entities, &b.entities) || a.entities == b.entities) {
                    return false;
                }
                fn names(arch: &Archetype) -> Vec<&str> {
                    let mut v: Vec<&str> = arch
                        .columns
                        .values()
                        .map(|c| c.type_name.as_str())
                        .collect();
                    v.sort_unstable();
                    v
                }
                names(a) == names(b)
            });
        if !aligned {
            return Self::diff_summary_by_entity(base, new);
        }
        let mut out = std::collections::BTreeMap::new();
        for (i, arch_new) in new.archetypes.iter().enumerate() {
            let arch_base = base.archetypes.get(i);
            for (tid, col_new) in &arch_new.columns {
                let changed = match arch_base.and_then(|a| a.columns.get(tid)) {
                    Some(col_base) => {
                        let all_shared = col_new.fields.len() == col_base.fields.len()
                            && col_new
                                .fields
                                .iter()
                                .zip(&col_base.fields)
                                .all(|(a, b)| Arc::ptr_eq(a, b));
                        if all_shared {
                            // CoW fast path: shared Arcs mean untouched data.
                            0
                        } else {
                            // Unshared columns happen on CoW writes: compare
                            // row values structurally (`Value ==` is a bit
                            // compare for scalars and a deep structural
                            // compare for heap objects — no allocation).
                            let rows_new = col_new.len();
                            let rows_base = col_base.len();
                            let common = rows_new.min(rows_base);
                            let mut changed = rows_new.max(rows_base) - common;
                            for r in 0..common {
                                let differs =
                                    col_new.fields.iter().zip(&col_base.fields).any(|(fa, fb)| {
                                        if Arc::ptr_eq(fa, fb) {
                                            return false;
                                        }
                                        match (fa.as_slice().get(r), fb.as_slice().get(r)) {
                                            (Some(a), Some(b)) => a != b,
                                            _ => true,
                                        }
                                    });
                                if differs {
                                    changed += 1;
                                }
                            }
                            changed
                        }
                    }
                    // Archetype (or column) absent in base: every row is new.
                    None => col_new.len(),
                };
                if changed > 0 {
                    *out.entry(col_new.type_name.clone()).or_insert(0) += changed;
                }
            }
        }
        // Rows that existed only in base (e.g. all entities of an archetype
        // despawned and the swap-removed columns shrank to zero).
        for (i, arch_base) in base.archetypes.iter().enumerate() {
            if new.archetypes.get(i).is_none() {
                for col in arch_base.columns.values() {
                    if col.len() > 0 {
                        *out.entry(col.type_name.clone()).or_insert(0) += col.len();
                    }
                }
            }
        }
        if !Arc::ptr_eq(&base.resources, &new.resources) {
            for (name, data_new) in new.resources.iter() {
                let changed = match base.resources.get(name) {
                    Some(data_base) => data_base.values != data_new.values,
                    None => true,
                };
                if changed {
                    *out.entry(name.clone()).or_insert(0) += 1;
                }
            }
            for name in base.resources.keys() {
                if !new.resources.contains_key(name) {
                    *out.entry(name.clone()).or_insert(0) += 1;
                }
            }
        }
        out
    }

    /// The set of entities whose state (components, liveness, or name) may
    /// differ between `base` and a CoW `fork` of it — computed by Arc
    /// comparison, so the cost is proportional to the **divergence**, not
    /// the world size. Returns `None` when the snapshots do not share
    /// lineage (archetype lists misaligned), in which case only a full scan
    /// can answer. Conservative: may include entities that turn out equal,
    /// never excludes one that differs.
    pub(crate) fn touched_entities(
        base: &WorldSnapshot,
        fork: &WorldSnapshot,
    ) -> Option<std::collections::BTreeSet<u32>> {
        let mut touched = std::collections::BTreeSet::new();
        let common = base.archetypes.len().min(fork.archetypes.len());
        for i in 0..common {
            let (a, b) = (&base.archetypes[i], &fork.archetypes[i]);
            if a.type_set != b.type_set {
                return None; // not the same lineage; positional pairing is meaningless
            }
            let rows_same = Arc::ptr_eq(&a.entities, &b.entities) || a.entities == b.entities;
            if !rows_same {
                // Spawns/despawns/migrations reorder rows (swap-remove);
                // flag the whole archetype pair rather than chase pairings.
                touched.extend(a.entities.iter().copied());
                touched.extend(b.entities.iter().copied());
                continue;
            }
            for (tid, col_b) in &b.columns {
                let Some(col_a) = a.columns.get(tid) else {
                    touched.extend(b.entities.iter().copied());
                    continue;
                };
                for (fa, fb) in col_a.fields.iter().zip(&col_b.fields) {
                    if Arc::ptr_eq(fa, fb) {
                        continue; // untouched column: zero work
                    }
                    let (va, vb) = (fa.as_slice(), fb.as_slice());
                    if va.len() != vb.len() {
                        touched.extend(b.entities.iter().copied());
                        break;
                    }
                    for r in 0..va.len() {
                        if va[r] != vb[r] {
                            touched.insert(b.entities[r]);
                        }
                    }
                }
            }
        }
        for arch in &fork.archetypes[common..] {
            touched.extend(arch.entities.iter().copied());
        }
        for arch in &base.archetypes[common..] {
            touched.extend(arch.entities.iter().copied());
        }
        // Renames matter too (names are semantic identity for merge).
        if !Arc::ptr_eq(&base.id_to_name, &fork.id_to_name) {
            for (eid, n) in base.id_to_name.iter() {
                if fork.id_to_name.get(eid) != Some(n) {
                    touched.insert(*eid);
                }
            }
            for (eid, n) in fork.id_to_name.iter() {
                if base.id_to_name.get(eid) != Some(n) {
                    touched.insert(*eid);
                }
            }
        }
        Some(touched)
    }

    /// Semantic diff for snapshots without shared lineage: compare per
    /// entity, per component type, structurally (field-name keyed, so layout
    /// order differences don't count). O(world), used only when the
    /// positional fast path would lie.
    fn diff_summary_by_entity(
        base: &WorldSnapshot,
        new: &WorldSnapshot,
    ) -> std::collections::BTreeMap<String, usize> {
        let mut wb = World::new();
        wb.restore(base.clone());
        let mut wn = World::new();
        wn.restore(new.clone());

        fn comp_eq(a: &ComponentData, b: &ComponentData) -> bool {
            if a.layout.len() != b.layout.len() {
                return false;
            }
            if Arc::ptr_eq(&a.layout, &b.layout) || *a.layout == *b.layout {
                return a.values == b.values;
            }
            a.layout.iter().zip(&a.values).all(|(f, v)| {
                b.layout
                    .iter()
                    .position(|n| n == f)
                    .is_some_and(|i| b.values[i] == *v)
            })
        }

        let mut out = std::collections::BTreeMap::new();
        let mut ids: std::collections::BTreeSet<u32> = wb.all_entity_ids().into_iter().collect();
        ids.extend(wn.all_entity_ids());
        for eid in ids {
            let comps_b: std::collections::BTreeMap<String, ComponentData> = wb
                .components_on_entity(eid)
                .into_iter()
                .map(|c| (c.type_name.clone(), c))
                .collect();
            let comps_n: std::collections::BTreeMap<String, ComponentData> = wn
                .components_on_entity(eid)
                .into_iter()
                .map(|c| (c.type_name.clone(), c))
                .collect();
            let mut names: std::collections::BTreeSet<&String> = comps_b.keys().collect();
            names.extend(comps_n.keys());
            for name in names {
                let same = match (comps_b.get(name), comps_n.get(name)) {
                    (Some(a), Some(b)) => comp_eq(a, b),
                    (None, None) => true,
                    _ => false,
                };
                if !same {
                    *out.entry(name.clone()).or_insert(0) += 1;
                }
            }
        }

        let mut res_names: std::collections::BTreeSet<String> =
            wb.resource_names().into_iter().collect();
        res_names.extend(wn.resource_names());
        for rname in res_names {
            let same = match (wb.get_resource(&rname), wn.get_resource(&rname)) {
                (Some(a), Some(b)) => comp_eq(&a, &b),
                (None, None) => true,
                _ => false,
            };
            if !same {
                *out.entry(rname).or_insert(0) += 1;
            }
        }
        out
    }
}

impl World {
    /// Create a copy-on-write snapshot of the world.
    ///
    /// Arc-wrapped fields are shared (O(1) refcount bump) rather than
    /// deep-cloned. Actual data cloning is deferred to first mutation
    /// via `Arc::make_mut`.
    pub fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            next_id: self.next_id,
            fresh_ids_exhausted: self.fresh_ids_exhausted,
            free_ids: self.free_ids.clone(),
            generations: Arc::clone(&self.generations),
            name_to_id: Arc::clone(&self.name_to_id),
            id_to_name: Arc::clone(&self.id_to_name),
            type_registry: Arc::clone(&self.type_registry),
            next_type_id: self.next_type_id,
            archetypes: self.archetypes.clone(),
            archetype_map: Arc::clone(&self.archetype_map),
            entity_archetype: Arc::clone(&self.entity_archetype),
            indexed_fields: Arc::clone(&self.indexed_fields),
            indices: Arc::clone(&self.indices),
            resources: Arc::clone(&self.resources),
            authoritative_relations: self.authoritative_relations.clone(),
            derived_relations: self.derived_relations.clone(),
            // Events live in the VM, not the World; the VM attaches them
            // (`VM::snapshot_with_events`) wherever in-flight state matters.
            events: Arc::new(Vec::new()),
            emit_ids: Arc::new(Vec::new()),
            delayed: Arc::new(Vec::new()),
            provenance: None,
            rollout_seed: None,
        }
    }

    pub fn restore(&mut self, snapshot: WorldSnapshot) {
        // Keep this exhaustive: adding execution-relevant snapshot state must
        // force an explicit restore policy as well as an operational-encoding
        // policy. VM-owned queues/provenance are restored by their owning VM
        // boundary, not by `World`.
        let WorldSnapshot {
            next_id,
            fresh_ids_exhausted,
            free_ids,
            generations,
            name_to_id,
            id_to_name,
            type_registry,
            next_type_id,
            archetypes,
            archetype_map,
            entity_archetype,
            indexed_fields,
            indices,
            resources,
            authoritative_relations,
            derived_relations,
            events: _,
            emit_ids: _,
            delayed: _,
            provenance: _,
            rollout_seed: _,
        } = snapshot;
        self.next_id = next_id;
        self.fresh_ids_exhausted = fresh_ids_exhausted;
        self.free_ids = free_ids;
        self.generations = generations;
        self.name_to_id = name_to_id;
        self.id_to_name = id_to_name;
        self.type_registry = type_registry;
        self.next_type_id = next_type_id;
        self.archetypes = archetypes;
        self.archetype_map = archetype_map;
        self.entity_archetype = entity_archetype;
        self.indexed_fields = indexed_fields;
        self.indices = indices;
        self.resources = resources;
        self.authoritative_relations = authoritative_relations;
        self.derived_relations = derived_relations;
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}
