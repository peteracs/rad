

impl World {
    pub fn trace(&self, marked: &mut HashSet<usize>) {
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

    pub fn new() -> Self {
        World {
            next_id: 0,
            fresh_ids_exhausted: false,
            free_ids: Arc::new(BTreeSet::new()),
            generations: Arc::new(HashMap::new()),
            name_to_id: Arc::new(HashMap::new()),
            id_to_name: Arc::new(HashMap::new()),
            type_registry: Arc::new(HashMap::new()),
            next_type_id: 0,
            archetypes: Vec::new(),
            archetype_map: Arc::new(HashMap::new()),
            entity_archetype: Arc::new(HashMap::new()),
            indexed_fields: Arc::new(HashMap::new()),
            indices: Arc::new(HashMap::new()),
            resources: Arc::new(ResourceMap::default()),
            authoritative_relations: crate::relation_runtime::AuthoritativeRelationState::default(),
        }
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

    fn field_value(data: &ComponentData, field_name: &str) -> Option<IndexValue> {
        let idx = data.layout.iter().position(|n| n == field_name)?;
        let raw = data.values.get(idx).copied()?;
        IndexValue::from_value(&raw)
    }

    fn indexed_field_names<'a>(&'a self, type_name: &str) -> Option<&'a HashSet<String>> {
        self.indexed_fields.get(type_name)
    }

    fn add_component_indices(&mut self, eid: u32, data: &ComponentData) {
        let Some(fields) = self.indexed_field_names(&data.type_name) else {
            return;
        };
        let mut entries: Vec<(String, IndexValue)> = Vec::new();
        for field_name in fields {
            if let Some(value) = Self::field_value(data, field_name) {
                entries.push((field_name.clone(), value));
            }
        }
        if entries.is_empty() {
            return;
        }
        let indices = Arc::make_mut(&mut self.indices);
        for (field_name, value) in entries {
            let key = IndexKey {
                type_name: data.type_name.clone(),
                field_name,
                value,
            };
            let entity_ids = indices.entry(key).or_default();
            if !entity_ids.contains(&eid) {
                entity_ids.push(eid);
            }
        }
    }

    fn remove_component_indices(&mut self, eid: u32, data: &ComponentData) {
        let Some(fields) = self.indexed_field_names(&data.type_name) else {
            return;
        };
        let mut keys = Vec::new();
        for field_name in fields {
            if let Some(value) = Self::field_value(data, field_name) {
                keys.push(IndexKey {
                    type_name: data.type_name.clone(),
                    field_name: field_name.clone(),
                    value,
                });
            }
        }
        if keys.is_empty() {
            return;
        }
        let indices = Arc::make_mut(&mut self.indices);
        for key in keys {
            if let Some(entity_ids) = indices.get_mut(&key) {
                entity_ids.retain(|id| *id != eid);
                if entity_ids.is_empty() {
                    indices.remove(&key);
                }
            }
        }
    }

    pub fn get_entity_by_name(&self, name: &str) -> Option<u32> {
        self.name_to_id.get(name).copied()
    }

    /// Insert an entity under a **caller-chosen id** (world merge, #7).
    /// Whether `eid` is currently live in the world.
    pub fn entity_exists(&self, eid: u32) -> bool {
        self.entity_archetype.contains_key(&eid)
    }

    pub fn relation_state(&self) -> &crate::relation_runtime::AuthoritativeRelationState {
        &self.authoritative_relations
    }

    pub(crate) fn restore_relation_transport(
        &mut self,
        encoded: &str,
        manifest: std::sync::Arc<crate::relation_runtime::RelationRuntimeManifest>,
    ) -> crate::relation_runtime::RelationRuntimeResult<()> {
        let state = crate::relation_runtime::AuthoritativeRelationState::from_transport_hex(
            encoded, manifest,
        )?;
        state.validate_live_entity_set(&self.live_relation_entities())?;
        self.authoritative_relations = state;
        Ok(())
    }

    pub fn install_relation_manifest(
        &mut self,
        manifest: std::sync::Arc<crate::relation_runtime::RelationRuntimeManifest>,
        expected: crate::relation_frontend::FrontendManifestDigest,
    ) -> crate::relation_runtime::RelationRuntimeResult<()> {
        self.authoritative_relations
            .install_manifest(manifest, expected)
    }

    pub(crate) fn live_relation_entities(
        &self,
    ) -> std::collections::BTreeSet<crate::relation_runtime::EntityRef> {
        self.entity_archetype
            .keys()
            .filter_map(|id| self.entity_ref(*id))
            .collect()
    }

    pub(crate) fn prepare_relation_candidate(
        &self,
        transaction: &crate::relation_runtime::RelationTransaction,
        live_after: std::collections::BTreeSet<crate::relation_runtime::EntityRef>,
        handles: std::collections::BTreeMap<u32, crate::relation_runtime::EntityRef>,
    ) -> crate::relation_runtime::RelationRuntimeResult<crate::relation_runtime::RelationCandidate>
    {
        self.authoritative_relations.prepare_candidate(
            transaction,
            &crate::relation_runtime::CandidateEntityState {
                live_after,
                candidate_handles: handles,
            },
        )
    }

    pub(crate) fn adopt_relation_candidate(
        &mut self,
        candidate: crate::relation_runtime::RelationCandidate,
    ) -> Vec<crate::relation_runtime::FactChange> {
        self.authoritative_relations.adopt(candidate)
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
        if self.archetype_map.get(&Vec::new()).is_some_and(|aid| {
            self.archetypes[*aid as usize].entity_row.contains_key(&eid)
        }) {
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
        let transaction = crate::relation_runtime::RelationTransaction {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations: Vec::new(),
            despawns: vec![crate::relation_runtime::PendingDespawn {
                entity,
                metadata: crate::relation_runtime::OperationMetadata::cause("entity.despawn"),
            }],
        };
        self.apply_relation_transaction(&transaction).is_ok()
    }

    /// Apply authoritative relation operations and entity deletion as one
    /// copy-on-write world candidate. A relation failure leaves ECS rows,
    /// assertion identities, indexes, and provenance untouched.
    pub fn apply_relation_transaction(
        &mut self,
        transaction: &crate::relation_runtime::RelationTransaction,
    ) -> crate::relation_runtime::RelationRuntimeResult<Vec<crate::relation_runtime::FactChange>>
    {
        // Construct the complete ECS + relation candidate in an isolated CoW
        // world. No allocator, component, entity, assertion, or index state is
        // adopted unless every phase succeeds.
        let mut candidate_world = World::new();
        candidate_world.restore(self.snapshot());
        let mut handles = std::collections::BTreeMap::new();
        let mut spawns = transaction.spawns.clone();
        spawns.sort_by_key(|spawn| spawn.handle);
        for pair in spawns.windows(2) {
            if pair[0].handle == pair[1].handle {
                return Err(crate::relation_runtime::RelationRuntimeError {
                    code: "entity.duplicate_candidate_handle",
                    detail: pair[0].handle.to_string(),
                });
            }
        }
        for spawn in spawns {
            let slot = candidate_world
                .spawn_entity(spawn.name.as_deref())
                .map_err(|error| crate::relation_runtime::RelationRuntimeError {
                    code: error.code(),
                    detail: "candidate entity allocation failed".into(),
                })?;
            handles.insert(
                spawn.handle,
                candidate_world
                    .entity_ref(slot)
                    .expect("new entity is live"),
            );
        }

        let mut component_writes = std::collections::BTreeMap::<
            (crate::relation_runtime::EntityRef, String),
            crate::value::ComponentData,
        >::new();
        for write in &transaction.component_writes {
            let entity = match write.entity {
                crate::relation_runtime::EntityOperand::Existing(entity) => entity,
                crate::relation_runtime::EntityOperand::Candidate(handle) => *handles
                    .get(&handle)
                    .ok_or_else(|| crate::relation_runtime::RelationRuntimeError {
                        code: "entity.unknown_candidate_handle",
                        detail: handle.to_string(),
                    })?,
            };
            if candidate_world.entity_ref(entity.slot) != Some(entity) {
                return Err(crate::relation_runtime::RelationRuntimeError {
                    code: "component.entity_not_live",
                    detail: format!("{}:{}", entity.slot, entity.generation),
                });
            }
            let key = (entity, write.component.type_name.clone());
            match component_writes.get(&key) {
                Some(existing) if existing != &write.component => {
                    return Err(crate::relation_runtime::RelationRuntimeError {
                        code: "component.write_conflict",
                        detail: format!("{}:{}::{}", entity.slot, entity.generation, key.1),
                    });
                }
                Some(_) => {}
                None => {
                    component_writes.insert(key, write.component.clone());
                }
            }
        }
        for ((entity, _), component) in component_writes {
            if !candidate_world.add_component(entity.slot, component) {
                return Err(crate::relation_runtime::RelationRuntimeError {
                    code: "component.write_failed",
                    detail: format!("{}:{}", entity.slot, entity.generation),
                });
            }
        }

        let mut live_after = candidate_world.live_relation_entities();
        let despawn_entities = transaction
            .despawns
            .iter()
            .map(|despawn| despawn.entity)
            .collect::<std::collections::BTreeSet<_>>();
        for entity in &despawn_entities {
            if !live_after.remove(entity) {
                return Err(crate::relation_runtime::RelationRuntimeError {
                    code: "entity.not_live",
                    detail: format!(
                        "{}:{} is not a live entity lifetime",
                        entity.slot, entity.generation
                    ),
                });
            }
        }
        let candidate =
            candidate_world.prepare_relation_candidate(transaction, live_after, handles)?;
        for entity in despawn_entities {
            // Exact lifetime membership was checked above; the raw slot is
            // now safe to remove only after the complete relation candidate
            // has passed restrict/cascade, foreign-key, and unique checks.
            let removed = candidate_world.destroy_entity_storage(entity.slot);
            debug_assert!(removed);
        }
        let changes = candidate_world.adopt_relation_candidate(candidate);
        self.restore(candidate_world.snapshot());
        Ok(changes)
    }

    /// Apply a host-admitted transaction. The complete envelope was checked
    /// before this method can clone or mutate the candidate world.
    pub fn apply_bounded_relation_transaction(
        &mut self,
        transaction: &crate::relation_runtime::BoundedRelationTransaction,
    ) -> crate::relation_runtime::RelationRuntimeResult<Vec<crate::relation_runtime::FactChange>>
    {
        self.apply_relation_transaction(transaction.transaction())
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
        names.sort();
        names
    }

    pub fn set_indexed_fields(&mut self, indexed_fields: HashMap<String, HashSet<String>>) {
        self.set_indexed_fields_arc(Arc::new(indexed_fields));
    }

    pub fn set_indexed_fields_arc(
        &mut self,
        indexed_fields: Arc<HashMap<String, HashSet<String>>>,
    ) {
        self.indexed_fields = indexed_fields;
        Arc::make_mut(&mut self.indices).clear();
        for eid in self.all_entity_ids() {
            let components = self.components_on_entity(eid);
            for component in components {
                self.add_component_indices(eid, &component);
            }
        }
    }

    pub fn is_field_indexed(&self, ctype: &str, field: &str) -> bool {
        self.indexed_fields
            .get(ctype)
            .map(|fields| fields.contains(field))
            .unwrap_or(false)
    }

    pub(crate) fn index_lookup(&self, ctype: &str, field: &str, value: Value) -> Option<u32> {
        let value_key = IndexValue::from_value(&value)?;
        let key = IndexKey {
            type_name: ctype.to_string(),
            field_name: field.to_string(),
            value: value_key,
        };
        // Lowest id, not "first inserted": bucket order is insertion order,
        // which differs between a live world (chronological) and one rebuilt
        // from a save or wire payload (id order). With duplicate keys,
        // min-id is the only answer that survives a save/load round trip.
        self.indices
            .get(&key)
            .and_then(|ids| ids.iter().min().copied())
    }

    /// Every entity whose indexed `ctype.field` equals `value`, sorted by
    /// id — the deterministic multi-match query ("all open tickets").
    pub(crate) fn index_lookup_all(&self, ctype: &str, field: &str, value: Value) -> Vec<u32> {
        let Some(value_key) = IndexValue::from_value(&value) else {
            return Vec::new();
        };
        let key = IndexKey {
            type_name: ctype.to_string(),
            field_name: field.to_string(),
            value: value_key,
        };
        let mut ids = self.indices.get(&key).cloned().unwrap_or_default();
        ids.sort_unstable();
        ids
    }

    /// Share another world's index *declarations* (cheap Arc clone, no
    /// rebuild). Decode paths build worlds from scratch; seeding the
    /// declarations first means `restore_entity_with_components` populates
    /// the indices as rows land, so a snapshot that crossed a wire carries
    /// working indices instead of silently wiping them on commit.
    pub fn share_indexed_fields_from(&mut self, other: &World) {
        self.indexed_fields = Arc::clone(&other.indexed_fields);
    }

    /// Reconcile the live world's index declarations with the program's
    /// (the compile result is the source of truth; snapshots only carry
    /// derived state). A no-op when they already agree — the rebuild only
    /// runs when a commit adopted a snapshot from a foreign or pre-fix
    /// lineage, which already paid O(world) to decode.
    pub fn ensure_indexed_fields(&mut self, declared: &Arc<HashMap<String, HashSet<String>>>) {
        if Arc::ptr_eq(&self.indexed_fields, declared) || *self.indexed_fields == **declared {
            return;
        }
        self.set_indexed_fields_arc(Arc::clone(declared));
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
        res_names.sort();
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
