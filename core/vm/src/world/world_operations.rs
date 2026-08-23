impl World {
    pub fn trace(&self, gc: &mut crate::gc::GcHeap) {
        for archetype in &self.archetypes {
            for col in archetype.columns.values() {
                col.trace(gc);
            }
        }
        for res in self.resources.values() {
            for val in &res.values {
                val.trace(gc);
            }
        }
    }

    pub fn new() -> Self {
        World {
            next_id: 0,
            fresh_ids_exhausted: false,
            free_ids: Arc::new(BTreeSet::new()),
            generations: Arc::new(HashMap::new()),
            name_to_id: Arc::new(HashMap::new()),
            id_to_name: Arc::new(HashMap::new()),
            type_registry: Arc::new(FastMap::default()),
            next_type_id: 0,
            archetypes: Vec::new(),
            archetype_map: Arc::new(HashMap::new()),
            entity_archetype: Arc::new(FastMap::default()),
            indexed_fields: Arc::new(HashMap::new()),
            indices: Arc::new(HashMap::new()),
            ordered_fields: Arc::new(HashMap::new()),
            ordered_indices: Arc::new(BTreeMap::new()),
            materialized_views: Arc::new(HashMap::new()),
            view_dependents: Arc::new(HashMap::new()),
            view_field_dependents: Arc::new(HashMap::new()),
            entered_phases: Arc::new(Vec::new()),
            lifecycle_traces: Arc::new(HashMap::new()),
            resources: Arc::new(ResourceMap::default()),
            authoritative_relations: crate::relation::runtime::AuthoritativeRelationState::default(
            ),
            derived_relations: crate::relation::derivation::DerivedRelationState::default(),
        }
    }

    pub(crate) fn enter_phase(&mut self, phase: &str) -> Result<(), String> {
        if self.entered_phases.iter().any(|entered| entered == phase) {
            return Err(format!(
                "Lifecycle phase '{phase}' was entered more than once"
            ));
        }
        Arc::make_mut(&mut self.entered_phases).push(phase.to_string());
        Ok(())
    }

    pub(crate) fn phase_entered(&self, phase: &str) -> bool {
        self.entered_phases.iter().any(|entered| entered == phase)
    }

    pub(crate) fn mark_lifecycle_phase(&mut self, entity: u32, phase: &str) -> Result<(), String> {
        if !self.contains_entity(entity) {
            return Err(format!(
                "Cannot mark lifecycle phase for missing entity {entity}"
            ));
        }
        let trace = Arc::make_mut(&mut self.lifecycle_traces)
            .entry(entity)
            .or_default();
        if trace.last().is_some_and(|last| last == phase) {
            return Err(format!(
                "Entity {entity} entered lifecycle phase '{phase}' twice"
            ));
        }
        trace.push(phase.to_string());
        Ok(())
    }

    pub(crate) fn lifecycle_trace(&self, entity: u32) -> &[String] {
        self.lifecycle_traces
            .get(&entity)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn type_id(&mut self, name: &str) -> TypeId {
        if let Some(&id) = self.type_registry.get(name) {
            return id;
        }
        let id = self.next_type_id;
        self.next_type_id += 1;
        Arc::make_mut(&mut self.type_registry).insert(name.to_string(), id);
        id
    }

    fn type_id_lookup(&self, name: &str) -> Option<TypeId> {
        self.type_registry.get(name).copied()
    }

    fn get_or_create_archetype(&mut self, mut type_set: Vec<TypeId>) -> ArchetypeId {
        type_set.sort_unstable();
        if let Some(&aid) = self.archetype_map.get(&type_set) {
            return aid;
        }
        let aid = self.archetypes.len() as ArchetypeId;
        self.archetypes.push(Archetype::new(type_set.clone()));
        Arc::make_mut(&mut self.archetype_map).insert(type_set, aid);
        aid
    }

    pub fn get_entity_by_name(&self, name: &str) -> Option<u32> {
        self.name_to_id.get(name).copied()
    }

    /// Insert an entity under a **caller-chosen id** (world merge, #7).
    /// Whether `eid` is currently live in the world.
    pub fn entity_exists(&self, eid: u32) -> bool {
        self.entity_archetype.contains_key(&eid)
    }

    /// Forks assign ids independently; when merging, entities that exist in
    /// only one fork must keep the id every value in that fork refers to.
    /// Maintains the id-allocator invariants for the bounded internal merge
    /// path. Full transport restoration installs a validated allocator state
    /// instead of inferring and expanding gaps from entity rows.
    pub(crate) fn insert_entity_with_id(
        &mut self,
        eid: u32,
        name: Option<&str>,
    ) -> Result<(), EntityAllocationError> {
        if self.entity_archetype.contains_key(&eid) {
            return Err(EntityAllocationError::IdAlreadyLive(eid));
        }
        if self
            .archetype_map
            .get(&Vec::new())
            .is_some_and(|aid| self.archetypes[*aid as usize].entity_row.contains_key(&eid))
        {
            return Err(EntityAllocationError::ArchetypeDuplicate(eid));
        }
        self.claim_explicit_entity_id(eid)?;
        let aid = self.get_or_create_archetype(Vec::new());
        self.archetypes[aid as usize].push_entity(eid, HashMap::new())?;
        Arc::make_mut(&mut self.entity_archetype).insert(eid, aid);
        self.set_entity_name(eid, name);
        Ok(())
    }

    /// Reconstruct one already-validated transport row in one archetype hop,
    /// without inferring allocator history from its numeric ID. The sealed
    /// allocator partition is installed after every row has been decoded.
    pub(crate) fn restore_entity_with_components(
        &mut self,
        eid: u32,
        name: Option<&str>,
        components: Vec<ComponentData>,
    ) -> Result<(), EntityAllocationError> {
        self.insert_entity_components_storage(eid, name, components)
    }

    /// Install every component of a freshly spawned entity in one archetype
    /// transition. Values are already persistent and ownership transfers to
    /// the world.
    ///
    /// The old spawn path inserted an empty row and then called
    /// `add_component_owned` once per component. A k-component spawn therefore
    /// moved the growing row through k archetypes and copied all prior fields
    /// at every step. Apart from being quadratic, that briefly published every
    /// partial shape to transaction view maintenance. Keeping identity
    /// allocation separate from shape installation preserves generation/name
    /// semantics while making the initial shape atomic and O(k).
    pub(crate) fn install_spawn_components_owned(
        &mut self,
        eid: u32,
        components: Vec<ComponentData>,
    ) -> Result<(), String> {
        if components.is_empty() {
            return Ok(());
        }
        let old_aid = *self
            .entity_archetype
            .get(&eid)
            .ok_or_else(|| format!("spawn component installation missing entity {eid}"))?;
        if !self.archetypes[old_aid as usize].type_set.is_empty() {
            return Err(format!(
                "spawn component installation requires an empty entity, but entity {eid} already has components"
            ));
        }

        let mut by_tid = HashMap::with_capacity(components.len());
        for data in components {
            let tid = self.type_id(&data.type_name);
            if let Some(displaced) = by_tid.insert(tid, data) {
                Value::release_component_data(&displaced);
            }
        }
        let type_set = by_tid.keys().copied().collect::<Vec<_>>();
        let new_aid = self.get_or_create_archetype(type_set);
        if self.archetypes[new_aid as usize]
            .entity_row
            .contains_key(&eid)
        {
            for component in by_tid.values() {
                Value::release_component_data(component);
            }
            return Err(format!(
                "spawn component installation found duplicate entity {eid} in target archetype"
            ));
        }

        let indexed_types = by_tid
            .values()
            .filter(|data| self.indexed_fields.contains_key(&data.type_name))
            .map(|data| data.type_name.clone())
            .collect::<Vec<_>>();
        self.archetypes[old_aid as usize]
            .remove_entity(eid)
            .ok_or_else(|| format!("spawn component installation lost empty row for entity {eid}"))?;
        self.archetypes[new_aid as usize]
            .push_entity(eid, by_tid)
            .map_err(|error| error.to_string())?;
        Arc::make_mut(&mut self.entity_archetype).insert(eid, new_aid);
        for component in indexed_types {
            if let Some(data) = self.get_component(eid, &component) {
                self.add_component_indices(eid, &data);
            }
        }
        Ok(())
    }

    /// Reverse one transactional despawn. `components` already own persistent
    /// values captured before destruction, so this path transfers them into
    /// storage without retaining a second copy.
    pub(crate) fn restore_transaction_despawn(
        &mut self,
        eid: u32,
        generation: u32,
        name: Option<&str>,
        components: Vec<ComponentData>,
    ) -> Result<(), EntityAllocationError> {
        if self.entity_archetype.contains_key(&eid) {
            for component in &components {
                Value::release_component_data(component);
            }
            return Err(EntityAllocationError::IdAlreadyLive(eid));
        }

        let mut by_tid = HashMap::with_capacity(components.len());
        for data in components {
            let tid = self.type_id(&data.type_name);
            by_tid.insert(tid, data);
        }
        let type_set = by_tid.keys().copied().collect::<Vec<_>>();
        let aid = self.get_or_create_archetype(type_set);
        let indexed = by_tid
            .values()
            .filter(|data| self.indexed_fields.contains_key(&data.type_name))
            .cloned()
            .collect::<Vec<_>>();
        self.archetypes[aid as usize].push_entity(eid, by_tid)?;
        Arc::make_mut(&mut self.entity_archetype).insert(eid, aid);
        Arc::make_mut(&mut self.free_ids).remove(&eid);
        self.set_entity_generation(eid, generation);
        self.set_entity_name(eid, name);
        for data in &indexed {
            self.add_component_indices(eid, data);
        }
        Ok(())
    }

    fn insert_entity_components_storage(
        &mut self,
        eid: u32,
        name: Option<&str>,
        components: Vec<ComponentData>,
    ) -> Result<(), EntityAllocationError> {
        if self.entity_archetype.contains_key(&eid) {
            return Err(EntityAllocationError::IdAlreadyLive(eid));
        }

        let mut by_tid: HashMap<TypeId, ComponentData> = HashMap::with_capacity(components.len());
        for mut data in components {
            Value::persist_component_data(&mut data);
            let tid = self.type_id(&data.type_name);
            by_tid.insert(tid, data);
        }
        let type_set: Vec<TypeId> = by_tid.keys().copied().collect();
        let aid = self.get_or_create_archetype(type_set);

        // Index bookkeeping needs the data after it lands; snapshot the
        // indexed ones up front (cheap: indices are opt-in per type).
        let indexed: Vec<ComponentData> = by_tid
            .values()
            .filter(|d| self.indexed_fields.contains_key(&d.type_name))
            .cloned()
            .collect();

        self.archetypes[aid as usize].push_entity(eid, by_tid)?;
        Arc::make_mut(&mut self.entity_archetype).insert(eid, aid);
        self.set_entity_name(eid, name);
        for data in &indexed {
            self.add_component_indices(eid, data);
        }
        Ok(())
    }

    /// Set, change, or clear (None) an entity's name, keeping both name maps
    /// consistent. A reused name is stolen from its previous owner, matching
    /// `spawn_entity` semantics.
    pub fn set_entity_name(&mut self, eid: u32, name: Option<&str>) {
        if let Some(old) = Arc::make_mut(&mut self.id_to_name).remove(&eid) {
            Arc::make_mut(&mut self.name_to_id).remove(&old);
        }
        if let Some(n) = name {
            if !n.is_empty() {
                if let Some(old_eid) =
                    Arc::make_mut(&mut self.name_to_id).insert(n.to_string(), eid)
                {
                    Arc::make_mut(&mut self.id_to_name).remove(&old_eid);
                }
                Arc::make_mut(&mut self.id_to_name).insert(eid, n.to_string());
            }
        }
    }

    pub(crate) fn add_component(&mut self, eid: u32, mut data: ComponentData) -> bool {
        if !self.entity_archetype.contains_key(&eid) {
            return false;
        }
        Value::persist_component_data(&mut data);
        self.add_component_owned(eid, data)
    }

    /// Like [`Self::add_component`] but takes data whose values are
    /// **already persisted** and owned by the caller — ownership transfers
    /// to the world without another deep copy. This is the sink for the
    /// command buffer (which persists at buffering time so deferred values
    /// survive worker GC); persisting again here would abandon one full
    /// persistent copy per write, which is how the syncdesk soak leaked.
    pub(crate) fn add_component_owned(&mut self, eid: u32, data: ComponentData) -> bool {
        let Some(&old_aid) = self.entity_archetype.get(&eid) else {
            // The caller owns persisted values; nothing will hold them.
            Value::release_component_data(&data);
            return false;
        };
        let existing_component = self.get_component(eid, &data.type_name);
        if let Some(old_component) = existing_component.as_ref() {
            self.remove_component_indices(eid, old_component);
        }
        let type_name = data.type_name.clone();
        let tid = self.type_id(&data.type_name);

        if self.archetypes[old_aid as usize].type_set.contains(&tid) {
            self.archetypes[old_aid as usize].set_component(eid, tid, data);
            if let Some(updated) = self.get_component(eid, &type_name) {
                self.add_component_indices(eid, &updated);
            }
            return true;
        }

        let mut new_type_set = self.archetypes[old_aid as usize].type_set.clone();
        new_type_set.push(tid);
        let new_aid = self.get_or_create_archetype(new_type_set);
        if self.archetypes[new_aid as usize]
            .entity_row
            .contains_key(&eid)
        {
            Value::release_component_data(&data);
            return false;
        }

        let mut components = self.archetypes[old_aid as usize]
            .remove_entity(eid)
            .unwrap_or_default();
        components.insert(tid, data);
        if self.archetypes[new_aid as usize]
            .push_entity(eid, components)
            .is_err()
        {
            return false;
        }
        Arc::make_mut(&mut self.entity_archetype).insert(eid, new_aid);
        if let Some(updated) = self.get_component(eid, &type_name) {
            self.add_component_indices(eid, &updated);
        }
        true
    }

    pub(crate) fn get_component(&self, eid: u32, ctype: &str) -> Option<ComponentData> {
        let &aid = self.entity_archetype.get(&eid)?;
        let tid = self.type_id_lookup(ctype)?;
        self.archetypes[aid as usize].get_component(eid, tid)
    }

    pub(crate) fn component_field_value(
        &self,
        eid: u32,
        ctype: &str,
        field_name: &str,
    ) -> Option<Value> {
        let &aid = self.entity_archetype.get(&eid)?;
        let tid = self.type_id_lookup(ctype)?;
        let archetype = &self.archetypes[aid as usize];
        let &row = archetype.entity_row.get(&eid)?;
        let column = archetype.columns.get(&tid)?;
        ComponentView::new(column, row).field(field_name)
    }

    pub(crate) fn resolve_field_address(
        &self,
        component: &str,
        field_name: &str,
        field_index: usize,
    ) -> Option<ResolvedFieldAddress> {
        let type_id = self.type_id_lookup(component)?;
        Some(ResolvedFieldAddress {
            type_id,
            field_index,
            indexed: self
                .indexed_field_names(component)
                .is_some_and(|fields| fields.contains(field_name)),
        })
    }

    #[inline]
    pub(crate) fn resolve_entity_address(&self, entity: u32) -> Option<ResolvedEntityAddress> {
        let &archetype = self.entity_archetype.get(&entity)?;
        let row = *self
            .archetypes
            .get(archetype as usize)?
            .entity_row
            .get(&entity)?;
        Some(ResolvedEntityAddress { archetype, row })
    }

    #[inline]
    pub(crate) fn component_field_value_resolved(
        &self,
        entity: ResolvedEntityAddress,
        field: ResolvedFieldAddress,
    ) -> Option<Value> {
        self.archetypes
            .get(entity.archetype as usize)?
            .columns
            .get(&field.type_id)?
            .field_at(entity.row, field.field_index)
    }

    /// Replace an unindexed SoA cell through a pre-resolved address. Indexed
    /// writes deliberately stay on the ordinary path so index publication can
    /// never be bypassed by a kernel optimization.
    #[inline]
    pub(crate) fn set_component_field_resolved_owned(
        &mut self,
        entity: ResolvedEntityAddress,
        field: ResolvedFieldAddress,
        value: Value,
    ) -> bool {
        if field.indexed {
            unsafe { value.release_persistent() };
            return false;
        }
        let Some(archetype) = self.archetypes.get_mut(entity.archetype as usize) else {
            unsafe { value.release_persistent() };
            return false;
        };
        let Some(column) = archetype.columns.get_mut(&field.type_id) else {
            unsafe { value.release_persistent() };
            return false;
        };
        column.set_field_at_owned(entity.row, field.field_index, value)
    }

    /// Replace one SoA cell without constructing a whole component value.
    /// `value` must already belong to the persistent store; this method takes
    /// ownership on success and releases it on every failure path.
    pub(crate) fn set_component_field_owned(
        &mut self,
        eid: u32,
        ctype: &str,
        field_name: &str,
        value: Value,
    ) -> bool {
        let Some(&aid) = self.entity_archetype.get(&eid) else {
            unsafe { value.release_persistent() };
            return false;
        };
        let Some(tid) = self.type_id_lookup(ctype) else {
            unsafe { value.release_persistent() };
            return false;
        };
        let old_index = self
            .indexed_field_names(ctype)
            .is_some_and(|fields| fields.contains(field_name))
            .then(|| self.component_field_value(eid, ctype, field_name))
            .flatten()
            .and_then(|old| IndexValue::from_value(&old));
        let new_index = old_index
            .as_ref()
            .and_then(|_| IndexValue::from_value(&value));
        if old_index.is_some() && new_index.is_none() {
            unsafe { value.release_persistent() };
            return false;
        }
        if let Some(old) = old_index.clone() {
            self.remove_field_index(eid, ctype, field_name, old);
        }
        let changed =
            self.archetypes[aid as usize].set_component_field_owned(eid, tid, field_name, value);
        if !changed {
            if let Some(old) = old_index {
                self.add_field_index(eid, ctype, field_name, old);
            }
            return false;
        }
        if let Some(new) = new_index {
            self.add_field_index(eid, ctype, field_name, new);
        }
        true
    }

    pub(crate) fn set_component(&mut self, eid: u32, data: ComponentData) -> bool {
        self.add_component(eid, data)
    }

    pub fn remove_component(&mut self, eid: u32, ctype: &str) -> bool {
        let Some(tid) = self.type_id_lookup(ctype) else {
            return false;
        };
        let Some(&old_aid) = self.entity_archetype.get(&eid) else {
            return false;
        };
        if !self.archetypes[old_aid as usize].type_set.contains(&tid) {
            return false;
        }

        let new_type_set: Vec<TypeId> = self.archetypes[old_aid as usize]
            .type_set
            .iter()
            .filter(|&&t| t != tid)
            .copied()
            .collect();
        let new_aid = self.get_or_create_archetype(new_type_set);
        if self.archetypes[new_aid as usize]
            .entity_row
            .contains_key(&eid)
        {
            return false;
        }

        if let Some(old_component) = self.get_component(eid, ctype) {
            self.remove_component_indices(eid, &old_component);
        }

        let mut components = self.archetypes[old_aid as usize]
            .remove_entity(eid)
            .unwrap_or_default();
        if let Some(removed_component) = components.remove(&tid) {
            Value::release_component_data(&removed_component);
        }

        if self.archetypes[new_aid as usize]
            .push_entity(eid, components)
            .is_err()
        {
            return false;
        }
        Arc::make_mut(&mut self.entity_archetype).insert(eid, new_aid);
        true
    }

    /// Remove an explicit component set with one archetype transition.
    ///
    /// This is the exact-authority bulk counterpart of repeated `remove`.
    /// It preserves per-component index and storage lifecycles while avoiding
    /// the O(k) shape migrations (and O(k^2) column copying) caused by moving
    /// the same entity once per removed component.
    pub(crate) fn remove_components(&mut self, eid: u32, ctypes: &[String]) -> Vec<String> {
        let Some(&old_aid) = self.entity_archetype.get(&eid) else {
            return Vec::new();
        };
        let old_type_set = self.archetypes[old_aid as usize].type_set.clone();
        let mut removals = Vec::with_capacity(ctypes.len());
        for ctype in ctypes {
            let Some(tid) = self.type_id_lookup(ctype) else {
                continue;
            };
            if old_type_set.contains(&tid)
                && !removals
                    .iter()
                    .any(|(existing, _): &(TypeId, String)| *existing == tid)
            {
                removals.push((tid, ctype.clone()));
            }
        }
        if removals.is_empty() {
            return Vec::new();
        }

        let new_type_set = old_type_set
            .into_iter()
            .filter(|tid| !removals.iter().any(|(removed, _)| removed == tid))
            .collect::<Vec<_>>();
        let new_aid = self.get_or_create_archetype(new_type_set);
        if self.archetypes[new_aid as usize]
            .entity_row
            .contains_key(&eid)
        {
            return Vec::new();
        }

        for (_, ctype) in &removals {
            if let Some(old_component) = self.get_component(eid, ctype) {
                self.remove_component_indices(eid, &old_component);
            }
        }
        let mut components = self.archetypes[old_aid as usize]
            .remove_entity(eid)
            .unwrap_or_default();
        for (tid, _) in &removals {
            if let Some(component) = components.remove(tid) {
                Value::release_component_data(&component);
            }
        }
        if self.archetypes[new_aid as usize]
            .push_entity(eid, components)
            .is_err()
        {
            return Vec::new();
        }
        Arc::make_mut(&mut self.entity_archetype).insert(eid, new_aid);
        removals.into_iter().map(|(_, name)| name).collect()
    }

    pub(crate) fn destroy_entity_storage(&mut self, eid: u32) -> bool {
        let Some(&aid) = self.entity_archetype.get(&eid) else {
            return false;
        };
        if let Some(removed) = self.archetypes[aid as usize].remove_entity(eid) {
            for comp in removed.values() {
                self.remove_component_indices(eid, comp);
                Value::release_component_data(comp);
            }
        }
        Arc::make_mut(&mut self.entity_archetype).remove(&eid);
        if let Some(name) = Arc::make_mut(&mut self.id_to_name).remove(&eid) {
            Arc::make_mut(&mut self.name_to_id).remove(&name);
        }
        if self.generations.get(&eid).copied().unwrap_or(0) != u32::MAX {
            Arc::make_mut(&mut self.free_ids).insert(eid);
        }
        true
    }

    pub fn destroy_entity(&mut self, eid: u32) -> bool {
        let Some(entity) = self.entity_ref(eid) else {
            return false;
        };
        if self.authoritative_relations.manifest().is_none() {
            return self.destroy_entity_storage(eid);
        }
        let transaction = crate::relation::runtime::RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: Vec::new(),
            despawns: vec![crate::relation::runtime::PendingDespawn {
                entity,
                metadata: crate::relation::runtime::OperationMetadata::cause("entity.despawn"),
            }],
        };
        self.apply_relation_transaction(&transaction).is_ok()
    }

    pub fn has_component(&self, eid: u32, ctype: &str) -> bool {
        let Some(tid) = self.type_id_lookup(ctype) else {
            return false;
        };
        let Some(&aid) = self.entity_archetype.get(&eid) else {
            return false;
        };
        self.archetypes[aid as usize].type_set.contains(&tid)
    }

    pub fn entity_name(&self, eid: u32) -> Option<String> {
        self.id_to_name.get(&eid).cloned()
    }

    /// Sorted resource names (deterministic iteration for `save_world`).
    pub fn resource_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.resources.keys().cloned().collect();
        names.sort_unstable();
        names
    }

    pub fn query(&self, with: &[String], without: &[String]) -> Vec<u32> {
        let with_tids: Vec<TypeId> = match with
            .iter()
            .map(|c| self.type_id_lookup(c))
            .collect::<Option<Vec<_>>>()
        {
            Some(v) => v,
            None => return Vec::new(),
        };

        // If a `without` type is unknown, it means no entity has it, so it's a no-op for exclusion.
        let without_tids: Vec<TypeId> = without
            .iter()
            .filter_map(|c| self.type_id_lookup(c))
            .collect();

        let mut result = Vec::new();
        for arch in &self.archetypes {
            if arch.contains_all(&with_tids) && arch.contains_none(&without_tids) {
                result.extend_from_slice(&arch.entities);
            }
        }
        result.sort_unstable();
        result
    }

    /// O(archetypes), allocation-free membership probe used by temporal
    /// monitors. This avoids materializing and sorting an entity list merely
    /// to answer whether one component is currently present anywhere.
    pub(crate) fn any_with_component(&self, component: &str) -> bool {
        let Some(type_id) = self.type_id_lookup(component) else {
            return false;
        };
        self.archetypes
            .iter()
            .any(|archetype| !archetype.entities.is_empty() && archetype.contains_all(&[type_id]))
    }

    pub(crate) fn get_resource(&self, name: &str) -> Option<ComponentData> {
        self.resources.get(name).cloned()
    }

    pub(crate) fn set_resource(&mut self, name: &str, mut data: ComponentData) {
        Value::persist_component_data(&mut data);
        self.set_resource_owned(name, data);
    }

    /// Like [`Self::set_resource`] but for **already-persisted** data whose
    /// ownership transfers to the world — no second deep copy. The
    /// displaced entry releases its values (see [`ResourceMap`]).
    pub(crate) fn set_resource_owned(&mut self, name: &str, data: ComponentData) {
        Arc::make_mut(&mut self.resources).insert_owned(name.to_string(), data);
    }

    pub(crate) fn remove_resource(&mut self, name: &str) -> bool {
        Arc::make_mut(&mut self.resources).remove_and_release(name)
    }

    pub(crate) fn init_resource(&mut self, name: &str, mut data: ComponentData) {
        if self.resources.contains_key(name) {
            return;
        }
        Value::persist_component_data(&mut data);
        Arc::make_mut(&mut self.resources).insert_owned(name.to_string(), data);
    }

    /// Collect all values for a single field of a component type across every
    /// archetype that contains the component.  Returns a flat `Vec<Value>`.
    pub(crate) fn get_column_values(
        &self,
        ctype: &str,
        field_index: usize,
    ) -> Result<Vec<Value>, String> {
        let tid = self
            .type_id_lookup(ctype)
            .ok_or_else(|| format!("LoadColumn: unknown component `{}`", ctype))?;

        let mut values = Vec::new();
        for archetype in &self.archetypes {
            if let Some(col) = archetype.columns.get(&tid) {
                if field_index >= col.fields.len() {
                    return Err(format!(
                        "LoadColumn: field index {} out of bounds for `{}`",
                        field_index, ctype
                    ));
                }
                values.extend_from_slice(col.fields[field_index].as_slice());
            }
        }
        Ok(values)
    }

    pub fn contains_entity(&self, eid: u32) -> bool {
        self.entity_archetype.contains_key(&eid)
    }

    pub fn max_live_entity_id(&self) -> Option<u32> {
        self.entity_archetype.keys().copied().max()
    }

    pub fn all_entity_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.entity_archetype.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub(crate) fn components_on_entity(&self, eid: u32) -> Vec<ComponentData> {
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

    /// Deterministic blake3 digest of the full world content: entities (in id
    /// order, component fields in layout order) plus resources (in name
    /// order). Two worlds with identical content produce identical digests in
    /// any process on any platform — the foundation for record/replay
    /// divergence checks and the determinism CI tripwire.
    pub fn content_digest(&self) -> String {
        use std::fmt::Write;
        let mut canon = self.snapshot_json_like();
        let mut res_names: Vec<&String> = self.resources.keys().collect();
        res_names.sort_unstable();
        for name in res_names {
            let data = &self.resources[name];
            let _ = write!(&mut canon, "|res:{}", name);
            for (k, v) in data.layout.iter().zip(data.values.iter()) {
                let _ = write!(&mut canon, ",{}={}", k, v);
            }
        }
        canon.push_str("|relations:");
        canon.push_str(&hex::encode(
            self.authoritative_relations.semantic_content_bytes(),
        ));
        canon.push_str("|derived:");
        canon.push_str(&hex::encode(self.derived_relations.canonical_bytes()));
        blake3::hash(canon.as_bytes()).to_hex().to_string()
    }

    /// JSON-like string of all live entities, optional names, and component payloads (for WASM / tooling).
    pub fn snapshot_json_like(&self) -> String {
        dump_world_json(
            &self.all_entity_ids(),
            |eid| self.id_to_name.get(&eid).cloned(),
            |eid| self.components_on_entity(eid),
            &self.resource_names(),
            |name| self.get_resource(name),
        )
    }
}
