

impl IndexValue {
    fn from_value(value: &Value) -> Option<Self> {
        if let Some(i) = value.as_int() {
            return Some(IndexValue::Int(i));
        }
        if let Some(s) = value.as_str() {
            return Some(IndexValue::Str(s.to_string()));
        }
        if let Some(b) = value.as_bool() {
            return Some(IndexValue::Bool(b));
        }
        if let Some(eid) = value.as_entity_id() {
            return Some(IndexValue::Entity(eid));
        }
        if let Some(f) = value.as_float() {
            return Some(IndexValue::Float(f.to_bits()));
        }
        None
    }
}

#[derive(Default)]
struct ValueColumn(Vec<Value>);

impl ValueColumn {
    #[inline]
    fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    fn as_slice(&self) -> &[Value] {
        &self.0
    }
}

impl Clone for ValueColumn {
    fn clone(&self) -> Self {
        let values = self.0.clone();
        // Arc::make_mut clones the column when a snapshot is shared. Retain
        // persistent refs so both copies own the values safely.
        for v in &values {
            unsafe { v.retain_persistent() };
        }
        ValueColumn(values)
    }
}

impl Drop for ValueColumn {
    fn drop(&mut self) {
        for v in &self.0 {
            unsafe { v.release_persistent() };
        }
    }
}

/// Resource storage with the same ownership discipline as internal `ValueColumn` storage:
/// every entry exclusively owns its (persisted) values — clone retains,
/// drop releases, and inserting releases the displaced entry. Without this,
/// `set_resource` leaked the previous payload on every call; with a
/// resource that grows (an audit log, a counter map), that leak is
/// quadratic in a long-running process. Found by the leak lab, paid for by
/// a 1-hour soak.
#[derive(Default)]
pub struct ResourceMap(HashMap<String, ComponentData>);

impl ResourceMap {
    /// Insert an entry whose values this map will own; releases whatever it
    /// displaces.
    fn insert_owned(&mut self, name: String, data: ComponentData) {
        if let Some(old) = self.0.insert(name, data) {
            Value::release_component_data(&old);
        }
    }
}

impl std::ops::Deref for ResourceMap {
    type Target = HashMap<String, ComponentData>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Clone for ResourceMap {
    fn clone(&self) -> Self {
        let m = self.0.clone();
        for data in m.values() {
            for v in &data.values {
                unsafe { v.retain_persistent() };
            }
        }
        ResourceMap(m)
    }
}

impl Drop for ResourceMap {
    fn drop(&mut self) {
        for data in self.0.values() {
            Value::release_component_data(data);
        }
    }
}

/// Structure-of-Arrays column for a single component type.
///
/// Each field is stored in its own `Arc<Vec<Value>>` for copy-on-write
/// semantics.  `Arc::clone` during fork/snapshot is O(1) (pointer bump).
/// Actual data cloning only happens on the first mutation after a fork
/// via `Arc::make_mut`.
#[derive(Clone)]
pub struct SoAColumn {
    pub type_name: String,
    pub layout: Arc<Vec<String>>,
    /// `fields[field_index]` — Arc-wrapped vec, one per field, one element per entity row.
    fields: Vec<Arc<ValueColumn>>,
}

/// Borrowed, allocation-free access to one component row in a frozen world.
/// Presentation and other read-only adapters can inspect fields without
/// materializing an owned `ComponentData` vector for every entity.
pub(crate) struct ComponentView<'a> {
    column: &'a SoAColumn,
    row: usize,
}

impl<'a> ComponentView<'a> {
    fn new(column: &'a SoAColumn, row: usize) -> Self {
        Self { column, row }
    }

    pub(crate) fn field(&self, name: &str) -> Option<Value> {
        let index = self.column.layout.iter().position(|field| field == name)?;
        self.column.fields.get(index)?.as_slice().get(self.row).copied()
    }
}

impl SoAColumn {
    fn new(type_name: String, layout: Arc<Vec<String>>) -> Self {
        let field_count = layout.len();
        SoAColumn {
            type_name,
            layout,
            fields: (0..field_count)
                .map(|_| Arc::new(ValueColumn::default()))
                .collect(),
        }
    }

    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.fields.first().map_or(0, |f| f.len())
    }

    fn push(&mut self, data: ComponentData) {
        let mut values = data.values.into_iter();
        for col in &mut self.fields {
            Arc::make_mut(col)
                .0
                .push(values.next().unwrap_or(Value::NIL));
        }
        // Release extras if payload has more values than declared layout.
        for extra in values {
            unsafe { extra.release_persistent() };
        }
    }

    fn get(&self, row: usize) -> ComponentData {
        ComponentData {
            type_name: self.type_name.clone(),
            layout: self.layout.clone(),
            values: self.fields.iter().map(|col| col.as_slice()[row]).collect(),
        }
    }

    fn take_row(&mut self, row: usize) -> ComponentData {
        let mut values = Vec::with_capacity(self.fields.len());
        for col in &mut self.fields {
            let field = &mut Arc::make_mut(col).0;
            let last = field.len() - 1;
            let removed = if row == last {
                field.pop().unwrap_or(Value::NIL)
            } else {
                field.swap_remove(row)
            };
            values.push(removed);
        }
        ComponentData {
            type_name: self.type_name.clone(),
            layout: self.layout.clone(),
            values,
        }
    }

    fn set(&mut self, row: usize, data: ComponentData) {
        let mut values = data.values.into_iter();
        for col in &mut self.fields {
            let new_val = values.next().unwrap_or(Value::NIL);
            let slot = &mut Arc::make_mut(col).0[row];
            let old = std::mem::replace(slot, new_val);
            unsafe { old.release_persistent() };
        }
        // Release extras if payload has more values than declared layout.
        for extra in values {
            unsafe { extra.release_persistent() };
        }
    }

    /// Trace all values in this column for GC reachability.
    pub(crate) fn trace(&self, marked: &mut HashSet<usize>) {
        for col in &self.fields {
            for val in col.as_slice() {
                val.trace(marked);
            }
        }
    }

    /// Iterate a single field across all rows (for future direct-field iteration).
    #[allow(dead_code)]
    pub(crate) fn field_slice(&self, field_index: usize) -> &[Value] {
        self.fields[field_index].as_slice()
    }
}

#[derive(Clone)]
pub struct Archetype {
    type_set: Vec<TypeId>,
    pub entities: Arc<Vec<u32>>,
    pub(crate) columns: HashMap<TypeId, SoAColumn>,
    entity_row: Arc<HashMap<u32, usize>>,
}

impl Archetype {
    fn new(type_set: Vec<TypeId>) -> Self {
        Archetype {
            type_set,
            entities: Arc::new(Vec::new()),
            columns: HashMap::new(),
            entity_row: Arc::new(HashMap::new()),
        }
    }

    #[allow(dead_code)]
    fn ensure_column(&mut self, tid: TypeId, type_name: &str, layout: &Arc<Vec<String>>) {
        self.columns
            .entry(tid)
            .or_insert_with(|| SoAColumn::new(type_name.to_string(), layout.clone()));
    }

    fn contains_all(&self, tids: &[TypeId]) -> bool {
        tids.iter().all(|t| self.type_set.contains(t))
    }

    fn contains_none(&self, tids: &[TypeId]) -> bool {
        tids.iter().all(|t| !self.type_set.contains(t))
    }

    fn push_entity(
        &mut self,
        eid: u32,
        mut components: HashMap<TypeId, ComponentData>,
    ) -> Result<(), EntityAllocationError> {
        if self.entity_row.contains_key(&eid) {
            return Err(EntityAllocationError::ArchetypeDuplicate(eid));
        }
        let row = self.entities.len();
        Arc::make_mut(&mut self.entities).push(eid);
        Arc::make_mut(&mut self.entity_row).insert(eid, row);

        for i in 0..self.type_set.len() {
            let tid = self.type_set[i];
            let data = components.remove(&tid).unwrap_or_else(|| {
                panic!(
                    "archetype push_entity: missing component data for type_id {}",
                    tid
                )
            });
            let type_name = data.type_name.clone();
            let layout = data.layout.clone();
            self.columns
                .entry(tid)
                .or_insert_with(|| SoAColumn::new(type_name, layout))
                .push(data);
        }
        Ok(())
    }

    fn remove_entity(&mut self, eid: u32) -> Option<HashMap<TypeId, ComponentData>> {
        let row = *self.entity_row.get(&eid)?;
        let last = self.entities.len() - 1;

        let mut removed = HashMap::new();
        for (&tid, col) in &mut self.columns {
            let data = col.take_row(row);
            removed.insert(tid, data);
        }

        if row != last {
            let swapped_eid = self.entities[last];
            Arc::make_mut(&mut self.entities).swap_remove(row);
            Arc::make_mut(&mut self.entity_row).insert(swapped_eid, row);
        } else {
            Arc::make_mut(&mut self.entities).pop();
        }
        Arc::make_mut(&mut self.entity_row).remove(&eid);
        Some(removed)
    }

    fn get_component(&self, eid: u32, tid: TypeId) -> Option<ComponentData> {
        let &row = self.entity_row.get(&eid)?;
        self.columns.get(&tid).map(|col| col.get(row))
    }

    fn set_component(&mut self, eid: u32, tid: TypeId, data: ComponentData) {
        if let Some(&row) = self.entity_row.get(&eid) {
            if let Some(col) = self.columns.get_mut(&tid) {
                col.set(row, data);
            }
        }
    }
}

/// Mutable world storage. Explicit entity identities are deliberately not a
/// public construction surface: transport and merge code must first prove a
/// complete allocator partition.
///
/// ```compile_fail
/// let mut world = rad_vm::world::World::new();
/// world.insert_entity_with_id(42, Some("forged"));
/// ```
pub struct World {
    next_id: u32,
    fresh_ids_exhausted: bool,
    free_ids: Arc<BTreeSet<u32>>,
    /// Current committed lifetime for every allocated entity slot. Relation
    /// values bind `(slot, generation)` so a recycled ECS id cannot silently
    /// retarget an authoritative fact.
    generations: Arc<HashMap<u32, u32>>,
    name_to_id: Arc<HashMap<String, u32>>,
    id_to_name: Arc<HashMap<u32, String>>,
    type_registry: Arc<HashMap<String, TypeId>>,
    next_type_id: TypeId,
    archetypes: Vec<Archetype>,
    archetype_map: Arc<HashMap<Vec<TypeId>, ArchetypeId>>,
    entity_archetype: Arc<HashMap<u32, ArchetypeId>>,
    indexed_fields: Arc<HashMap<String, HashSet<String>>>,
    indices: Arc<HashMap<IndexKey, Vec<u32>>>,
    resources: Arc<ResourceMap>,
    authoritative_relations: crate::relation_runtime::AuthoritativeRelationState,
    derived_relations: crate::relation_derivation::DerivedRelationState,
}
