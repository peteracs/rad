// Field indexes: the hashed `indexed` lookups and the ordered `ordered`
// traversals, plus the index maintenance every component write drives.

impl World {

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
        let ordered = self
            .ordered_fields
            .get(&data.type_name)
            .cloned()
            .unwrap_or_default();
        for (field_name, value) in entries {
            let key = IndexKey {
                type_name: data.type_name.clone(),
                field_name: field_name.clone(),
                value,
            };
            let entity_ids = Arc::make_mut(&mut self.indices)
                .entry(key.clone())
                .or_default();
            if !entity_ids.contains(&eid) {
                entity_ids.push(eid);
            }
            if ordered.contains(&field_name) {
                let ordered_ids = Arc::make_mut(&mut self.ordered_indices)
                    .entry(key)
                    .or_default();
                match ordered_ids.binary_search(&eid) {
                    Ok(_) => {}
                    Err(position) => ordered_ids.insert(position, eid),
                }
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
        for key in keys {
            let indices = Arc::make_mut(&mut self.indices);
            if let Some(entity_ids) = indices.get_mut(&key) {
                entity_ids.retain(|id| *id != eid);
                if entity_ids.is_empty() {
                    indices.remove(&key);
                }
            }
            let ordered_indices = Arc::make_mut(&mut self.ordered_indices);
            if let Some(entity_ids) = ordered_indices.get_mut(&key) {
                if let Ok(position) = entity_ids.binary_search(&eid) {
                    entity_ids.remove(position);
                }
                if entity_ids.is_empty() {
                    ordered_indices.remove(&key);
                }
            }
        }
    }

    fn add_field_index(&mut self, eid: u32, type_name: &str, field_name: &str, value: IndexValue) {
        let key = IndexKey {
            type_name: type_name.to_string(),
            field_name: field_name.to_string(),
            value,
        };
        let ids = Arc::make_mut(&mut self.indices)
            .entry(key.clone())
            .or_default();
        if !ids.contains(&eid) {
            ids.push(eid);
        }
        if self
            .ordered_fields
            .get(type_name)
            .is_some_and(|fields| fields.contains(field_name))
        {
            let ordered = Arc::make_mut(&mut self.ordered_indices)
                .entry(key)
                .or_default();
            if let Err(position) = ordered.binary_search(&eid) {
                ordered.insert(position, eid);
            }
        }
    }

    fn remove_field_index(
        &mut self,
        eid: u32,
        type_name: &str,
        field_name: &str,
        value: IndexValue,
    ) {
        let key = IndexKey {
            type_name: type_name.to_string(),
            field_name: field_name.to_string(),
            value,
        };
        let indices = Arc::make_mut(&mut self.indices);
        if let Some(ids) = indices.get_mut(&key) {
            ids.retain(|id| *id != eid);
            if ids.is_empty() {
                indices.remove(&key);
            }
        }
        let ordered = Arc::make_mut(&mut self.ordered_indices);
        if let Some(ids) = ordered.get_mut(&key) {
            if let Ok(position) = ids.binary_search(&eid) {
                ids.remove(position);
            }
            if ids.is_empty() {
                ordered.remove(&key);
            }
        }
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

    pub fn set_ordered_fields_arc(
        &mut self,
        ordered_fields: Arc<HashMap<String, HashSet<String>>>,
    ) {
        self.ordered_fields = ordered_fields;
        Arc::make_mut(&mut self.ordered_indices).clear();
        for entity in self.all_entity_ids() {
            for component in self.components_on_entity(entity) {
                self.add_component_indices(entity, &component);
            }
        }
    }

    pub fn is_field_ordered(&self, component: &str, field: &str) -> bool {
        self.ordered_fields
            .get(component)
            .is_some_and(|fields| fields.contains(field))
    }

    fn ordered_prefix_bounds(component: &str, field: &str) -> (IndexKey, IndexKey) {
        (
            IndexKey {
                type_name: component.to_string(),
                field_name: field.to_string(),
                value: IndexValue::Min,
            },
            IndexKey {
                type_name: component.to_string(),
                field_name: field.to_string(),
                value: IndexValue::Max,
            },
        )
    }

    pub(crate) fn ordered_first(&self, component: &str, field: &str) -> Option<u32> {
        let (start, end) = Self::ordered_prefix_bounds(component, field);
        self.ordered_indices
            .range(start..=end)
            .next()
            .and_then(|(_, entities)| entities.first().copied())
    }

    pub(crate) fn ordered_last(&self, component: &str, field: &str) -> Option<u32> {
        let (start, end) = Self::ordered_prefix_bounds(component, field);
        self.ordered_indices
            .range(start..=end)
            .next_back()
            .and_then(|(_, entities)| entities.last().copied())
    }

    pub(crate) fn ordered_bound(
        &self,
        component: &str,
        field: &str,
        value: &Value,
        strict: bool,
    ) -> Option<u32> {
        let value = IndexValue::from_value(value)?;
        let start = IndexKey {
            type_name: component.to_string(),
            field_name: field.to_string(),
            value: value.clone(),
        };
        for (key, entities) in self.ordered_indices.range(start..) {
            if key.type_name != component || key.field_name != field {
                break;
            }
            if strict && key.value == value {
                continue;
            }
            if let Some(entity) = entities.first() {
                return Some(*entity);
            }
        }
        None
    }

    pub(crate) fn ordered_previous(
        &self,
        component: &str,
        field: &str,
        value: &Value,
    ) -> Option<u32> {
        let value = IndexValue::from_value(value)?;
        let (prefix_start, _) = Self::ordered_prefix_bounds(component, field);
        let end = IndexKey {
            type_name: component.to_string(),
            field_name: field.to_string(),
            value,
        };
        self.ordered_indices
            .range(prefix_start..end)
            .next_back()
            .and_then(|(_, entities)| entities.last().copied())
    }

    pub(crate) fn ordered_range(
        &self,
        component: &str,
        field: &str,
        lower: &Value,
        upper: &Value,
    ) -> Option<Vec<u32>> {
        let lower = IndexValue::from_value(lower)?;
        let upper = IndexValue::from_value(upper)?;
        let start = IndexKey {
            type_name: component.to_string(),
            field_name: field.to_string(),
            value: lower,
        };
        let end = IndexKey {
            type_name: component.to_string(),
            field_name: field.to_string(),
            value: upper,
        };
        let mut entities = Vec::new();
        for (_, bucket) in self.ordered_indices.range(start..end) {
            entities.extend(bucket.iter().copied());
        }
        Some(entities)
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
        self.ordered_fields = Arc::clone(&other.ordered_fields);
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

    pub fn ensure_ordered_fields(&mut self, declared: &Arc<HashMap<String, HashSet<String>>>) {
        if Arc::ptr_eq(&self.ordered_fields, declared) || *self.ordered_fields == **declared {
            return;
        }
        self.set_ordered_fields_arc(Arc::clone(declared));
    }
}
