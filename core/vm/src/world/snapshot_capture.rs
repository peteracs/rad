// Capturing and restoring a copy-on-write world snapshot, and the read
// accessors callers use to inspect one without rebuilding a live world.

pub(crate) enum EntitySelectionError {
    LimitExceeded { actual: usize },
    AllocationFailed,
}

impl WorldSnapshot {
    pub(crate) fn relation_state(&self) -> &crate::relation::runtime::AuthoritativeRelationState {
        &self.authoritative_relations
    }

    pub(crate) fn derived_relation_state(
        &self,
    ) -> &crate::relation::derivation::DerivedRelationState {
        &self.derived_relations
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
            ordered_fields: Arc::clone(&self.ordered_fields),
            ordered_indices: Arc::clone(&self.ordered_indices),
            materialized_views: Arc::clone(&self.materialized_views),
            view_dependents: Arc::clone(&self.view_dependents),
            view_field_dependents: Arc::clone(&self.view_field_dependents),
            entered_phases: Arc::clone(&self.entered_phases),
            lifecycle_traces: Arc::clone(&self.lifecycle_traces),
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
            ordered_fields,
            ordered_indices,
            materialized_views,
            view_dependents,
            view_field_dependents,
            entered_phases,
            lifecycle_traces,
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
        self.ordered_fields = ordered_fields;
        self.ordered_indices = ordered_indices;
        self.materialized_views = materialized_views;
        self.view_dependents = view_dependents;
        self.view_field_dependents = view_field_dependents;
        self.entered_phases = entered_phases;
        self.lifecycle_traces = lifecycle_traces;
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
