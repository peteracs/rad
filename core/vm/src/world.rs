use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::value::{ComponentData, Value};

type ArchetypeId = u32;
type TypeId = u32;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct IndexKey {
    type_name: String,
    field_name: String,
    value: IndexValue,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum IndexValue {
    Int(i64),
    Str(String),
    Bool(bool),
    Entity(u32),
    Float(u64),
}

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

    fn push_entity(&mut self, eid: u32, mut components: HashMap<TypeId, ComponentData>) {
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

pub struct World {
    next_id: u32,
    fresh_ids_exhausted: bool,
    free_ids: Vec<u32>,
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
}

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
            free_ids: Vec::new(),
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

    pub fn try_spawn_entity(&mut self, name: Option<&str>) -> Result<u32, &'static str> {
        let (eid, generation) = loop {
            let reusable_index = self
                .free_ids
                .iter()
                .enumerate()
                .min_by_key(|(_, id)| **id)
                .map(|(index, _)| index);
            let reusable = reusable_index.map(|index| self.free_ids.swap_remove(index));
            match reusable {
                Some(reused) => {
                    let previous = self.generations.get(&reused).copied().unwrap_or(0);
                    if let Some(generation) = previous.checked_add(1) {
                        break (reused, generation);
                    }
                    // An exhausted slot is retired permanently. Continue in
                    // exact allocator order instead of wrapping its lifetime.
                }
                None => {
                    if self.fresh_ids_exhausted {
                        return Err("entity.id_space_exhausted");
                    }
                    let fresh = self.next_id;
                    if fresh == u32::MAX {
                        self.fresh_ids_exhausted = true;
                    } else {
                        self.next_id = fresh + 1;
                    }
                    break (fresh, 0);
                }
            }
        };
        Arc::make_mut(&mut self.generations).insert(eid, generation);
        let aid = self.get_or_create_archetype(Vec::new());
        self.archetypes[aid as usize].push_entity(eid, HashMap::new());
        Arc::make_mut(&mut self.entity_archetype).insert(eid, aid);
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
        Ok(eid)
    }

    pub fn spawn_entity(&mut self, name: Option<&str>) -> u32 {
        self.try_spawn_entity(name)
            .expect("Entity ID overflow: exceeded 2^32 entity lifetimes")
    }

    /// Insert an entity under a **caller-chosen id** (world merge, #7).
    /// Whether `eid` is currently live in the world.
    pub fn entity_exists(&self, eid: u32) -> bool {
        self.entity_archetype.contains_key(&eid)
    }

    pub fn entity_ref(&self, eid: u32) -> Option<crate::relation_runtime::EntityRef> {
        self.entity_exists(eid)
            .then(|| crate::relation_runtime::EntityRef {
                slot: eid,
                generation: self.generations.get(&eid).copied().unwrap_or(0),
            })
    }

    pub fn relation_state(&self) -> &crate::relation_runtime::AuthoritativeRelationState {
        &self.authoritative_relations
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
    /// Maintains the id-allocator invariants: ids skipped over become free,
    /// reused free ids leave the free list. Returns false if `eid` is live.
    pub fn insert_entity_with_id(&mut self, eid: u32, name: Option<&str>) -> bool {
        if self.entity_archetype.contains_key(&eid) {
            return false;
        }
        let Some(generation) = self
            .generations
            .get(&eid)
            .copied()
            .map_or(Some(0), |generation| generation.checked_add(1))
        else {
            return false;
        };
        if self.fresh_ids_exhausted {
            self.free_ids.retain(|&f| f != eid);
        } else if eid >= self.next_id {
            for skipped in self.next_id..eid {
                self.free_ids.push(skipped);
            }
            if eid == u32::MAX {
                self.fresh_ids_exhausted = true;
            } else {
                self.next_id = eid + 1;
            }
        } else {
            self.free_ids.retain(|&f| f != eid);
        }
        Arc::make_mut(&mut self.generations).insert(eid, generation);
        let aid = self.get_or_create_archetype(Vec::new());
        self.archetypes[aid as usize].push_entity(eid, HashMap::new());
        Arc::make_mut(&mut self.entity_archetype).insert(eid, aid);
        self.set_entity_name(eid, name);
        true
    }

    /// Insert a fresh entity with all of its components in **one archetype
    /// hop** — the bulk path for world reconstruction (wire decode, merge,
    /// load). The incremental path (`insert_entity_with_id` + N×
    /// `add_component`) costs N archetype migrations, each copying every
    /// already-attached row; this costs one row push.
    pub(crate) fn insert_entity_with_components(
        &mut self,
        eid: u32,
        name: Option<&str>,
        components: Vec<ComponentData>,
    ) -> bool {
        if self.entity_archetype.contains_key(&eid) {
            return false;
        }
        let Some(generation) = self
            .generations
            .get(&eid)
            .copied()
            .map_or(Some(0), |generation| generation.checked_add(1))
        else {
            return false;
        };
        if self.fresh_ids_exhausted {
            self.free_ids.retain(|&f| f != eid);
        } else if eid >= self.next_id {
            for skipped in self.next_id..eid {
                self.free_ids.push(skipped);
            }
            if eid == u32::MAX {
                self.fresh_ids_exhausted = true;
            } else {
                self.next_id = eid + 1;
            }
        } else {
            self.free_ids.retain(|&f| f != eid);
        }
        Arc::make_mut(&mut self.generations).insert(eid, generation);

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

        self.archetypes[aid as usize].push_entity(eid, by_tid);
        Arc::make_mut(&mut self.entity_archetype).insert(eid, aid);
        self.set_entity_name(eid, name);
        for data in &indexed {
            self.add_component_indices(eid, data);
        }
        true
    }

    /// Attach a batch of components to a live entity in **one archetype
    /// hop** (vs one hop per component via `add_component`). Replaced
    /// components release their persistent payloads like `add_component`.
    pub(crate) fn add_components_bulk(&mut self, eid: u32, components: Vec<ComponentData>) -> bool {
        let Some(&old_aid) = self.entity_archetype.get(&eid) else {
            return false;
        };
        let mut row = self.archetypes[old_aid as usize]
            .remove_entity(eid)
            .unwrap_or_default();
        let mut new_type_set = self.archetypes[old_aid as usize].type_set.clone();
        let mut indexed: Vec<String> = Vec::new();
        for mut data in components {
            Value::persist_component_data(&mut data);
            if self.indexed_fields.contains_key(&data.type_name) {
                indexed.push(data.type_name.clone());
            }
            let tid = self.type_id(&data.type_name);
            if let Some(old) = row.insert(tid, data) {
                self.remove_component_indices(eid, &old);
                Value::release_component_data(&old);
            } else {
                new_type_set.push(tid);
            }
        }
        let new_aid = self.get_or_create_archetype(new_type_set);
        self.archetypes[new_aid as usize].push_entity(eid, row);
        Arc::make_mut(&mut self.entity_archetype).insert(eid, new_aid);
        for tname in indexed {
            if let Some(c) = self.get_component(eid, &tname) {
                self.add_component_indices(eid, &c);
            }
        }
        true
    }

    /// Overwrite the id-allocator state (wire codec: `fork_from_bytes`
    /// reconstructs the sender's exact allocator so post-transfer spawns
    /// allocate identically on both sides). Rejects state that contradicts
    /// the live entity set.
    pub fn set_id_allocator(&mut self, next_id: u32, free_ids: Vec<u32>) -> Result<(), String> {
        for &eid in self.entity_archetype.keys() {
            if eid >= next_id {
                return Err(format!(
                    "id allocator: next_id {} but entity {} exists",
                    next_id, eid
                ));
            }
        }
        for &fid in &free_ids {
            if self.entity_archetype.contains_key(&fid) {
                return Err(format!("id allocator: free id {} is a live entity", fid));
            }
            if fid >= next_id {
                return Err(format!(
                    "id allocator: free id {} >= next_id {}",
                    fid, next_id
                ));
            }
        }
        self.next_id = next_id;
        self.fresh_ids_exhausted = false;
        self.free_ids = free_ids;
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

        let mut components = self.archetypes[old_aid as usize]
            .remove_entity(eid)
            .unwrap_or_default();

        let mut new_type_set = self.archetypes[old_aid as usize].type_set.clone();
        new_type_set.push(tid);

        let new_aid = self.get_or_create_archetype(new_type_set);
        components.insert(tid, data);
        self.archetypes[new_aid as usize].push_entity(eid, components);
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

        if let Some(old_component) = self.get_component(eid, ctype) {
            self.remove_component_indices(eid, &old_component);
        }

        let mut components = self.archetypes[old_aid as usize]
            .remove_entity(eid)
            .unwrap_or_default();
        if let Some(removed_component) = components.remove(&tid) {
            Value::release_component_data(&removed_component);
        }

        let new_type_set: Vec<TypeId> = self.archetypes[old_aid as usize]
            .type_set
            .iter()
            .filter(|&&t| t != tid)
            .copied()
            .collect();

        let new_aid = self.get_or_create_archetype(new_type_set);
        self.archetypes[new_aid as usize].push_entity(eid, components);
        Arc::make_mut(&mut self.entity_archetype).insert(eid, new_aid);
        true
    }

    fn destroy_entity_storage(&mut self, eid: u32) -> bool {
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
        self.free_ids.push(eid);
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
                .try_spawn_entity(spawn.name.as_deref())
                .map_err(|code| crate::relation_runtime::RelationRuntimeError {
                    code,
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
    /// declarations first means `insert_entity_with_components` populates
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

/// `{"field":value,...}` for one component/resource row (Display-encoded
/// values, escaped keys) — shared by full dumps and render deltas.
fn resource_fields_json(data: &ComponentData) -> String {
    use std::fmt::Write;
    let mut out = String::from("{");
    for (i, (k, v)) in data.layout.iter().zip(data.values.iter()).enumerate() {
        if i > 0 {
            out.push(',');
        }
        let mut key = String::with_capacity(k.len());
        for ch in k.chars() {
            match ch {
                '"' => key.push_str("\\\""),
                '\\' => key.push_str("\\\\"),
                c => key.push(c),
            }
        }
        let _ = write!(&mut out, "\"{}\":{}", key, v);
    }
    out.push('}');
    out
}

/// Shared renderer-facing dump for live worlds and timeline snapshots:
/// `{"entities":[{id,name,components:[{type,fields}]}],"resources":{..}}`.
fn dump_world_json(
    ids: &[u32],
    name_of: impl Fn(u32) -> Option<String>,
    comps_of: impl Fn(u32) -> Vec<ComponentData>,
    resource_names: &[String],
    resource_get: impl Fn(&str) -> Option<ComponentData>,
) -> String {
    use std::fmt::Write;

    fn json_escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => {
                    let _ = write!(&mut out, "\\u{:04x}", c as u32);
                }
                c => out.push(c),
            }
        }
        out
    }

    fn fields_json(layout: &[String], values: &[Value]) -> String {
        let mut out = String::from("{");
        for (i, (k, v)) in layout.iter().zip(values.iter()).enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(&mut out, "\"{}\":{}", json_escape(k), v);
        }
        out.push('}');
        out
    }

    let mut s = String::from("{\"entities\":[");
    for (i, &eid) in ids.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push('{');
        let _ = write!(&mut s, "\"id\":{}", eid);
        if let Some(name) = name_of(eid) {
            let _ = write!(&mut s, ",\"name\":\"{}\"", json_escape(&name));
        } else {
            s.push_str(",\"name\":null");
        }
        s.push_str(",\"components\":[");
        let comps = comps_of(eid);
        for (j, c) in comps.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            s.push('{');
            let _ = write!(
                &mut s,
                "\"type\":\"{}\",\"fields\":{}",
                json_escape(&c.type_name),
                fields_json(&c.layout, &c.values)
            );
            s.push('}');
        }
        s.push_str("]}");
    }
    // Resources are program state too — a GUI renderer reading the world
    // needs e.g. its UiConfig. Same field encoding as components.
    s.push_str("],\"resources\":{");
    for (i, name) in resource_names.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        if let Some(data) = resource_get(name) {
            let _ = write!(
                &mut s,
                "\"{}\":{}",
                json_escape(name),
                fields_json(&data.layout, &data.values)
            );
        }
    }
    s.push_str("}}");
    s
}

#[derive(Clone)]
pub struct WorldSnapshot {
    pub next_id: u32,
    pub fresh_ids_exhausted: bool,
    pub free_ids: Vec<u32>,
    pub generations: Arc<HashMap<u32, u32>>,
    pub name_to_id: Arc<HashMap<String, u32>>,
    pub id_to_name: Arc<HashMap<u32, String>>,
    pub type_registry: Arc<HashMap<String, TypeId>>,
    pub next_type_id: TypeId,
    pub(crate) archetypes: Vec<Archetype>,
    pub archetype_map: Arc<HashMap<Vec<TypeId>, ArchetypeId>>,
    pub entity_archetype: Arc<HashMap<u32, ArchetypeId>>,
    indexed_fields: Arc<HashMap<String, HashSet<String>>>,
    indices: Arc<HashMap<IndexKey, Vec<u32>>>,
    resources: Arc<ResourceMap>,
    authoritative_relations: crate::relation_runtime::AuthoritativeRelationState,
    /// In-flight events at capture time: `(event, payload, trace_id)`.
    /// Events are program state — a snapshot that drops them is not a
    /// snapshot. Payloads are persisted on capture. `fork()` fills this,
    /// `commit()` restores it, `simulate()`/sandbox guests seed from it,
    /// and `merge_forks` three-way merges it.
    pub(crate) events: Arc<Vec<(String, Value, u64)>>,
    /// Causality emit-record ids, parallel to `events` (provenance survives
    /// the fork/commit roundtrip).
    pub emit_ids: Arc<Vec<u64>>,
    /// Delayed (`emit … after N`) timers at capture time: `(ticks_left,
    /// event, payload, emit_id)`. Same principle as `events` — timers are
    /// program state; a snapshot that drops them loses every scheduled
    /// respawn, and the emit id keeps timer causality intact.
    pub(crate) delayed: Arc<Vec<(i64, String, Value, u64)>>,
    /// Foreign provenance riding the snapshot: set by `fork_from_bytes`
    /// (the sender's ledger closure), carried through `merge_forks`, and
    /// ingested into the local ledger by `commit()`. `None` for local forks
    /// — their provenance already lives in the VM's ledger.
    pub provenance: Option<Arc<crate::causality::WireProvenance>>,
    /// The effective RNG seed the rollout that produced this snapshot ran
    /// under, when it came out of the simulate family (`simulate_par`,
    /// `simulate_many`, `simulate_seeded`). `fork_seed()` reads it, making a
    /// single outlier rollout reproducible in isolation (dogfood feature seq
    /// 150). Local-only debug metadata: never serialized to the world wire or
    /// included in the content digest, but included in operational replay
    /// identity because `fork_seed()` makes it observable. Cleared by
    /// `with_resource` (an overridden copy is a new candidate, not the
    /// rollout's output).
    pub rollout_seed: Option<u64>,
}

/// Versioned sink for the complete execution-relevant state of a
/// [`WorldSnapshot`]. The snapshot owns the inventory; replay hashing and
/// `WorldFork` graph identity only supply the sink. This prevents the restore
/// and identity paths from growing independent, renderer-shaped field lists.
pub(crate) trait OperationalWorldEncoder {
    fn byte(&mut self, value: u8);
    fn u32(&mut self, value: u32);
    fn u64(&mut self, value: u64);
    fn i64(&mut self, value: i64);
    fn usize(&mut self, value: usize);
    fn bool(&mut self, value: bool);
    fn text(&mut self, value: &str);
    fn value(&mut self, value: Value);

    fn optional_u64(&mut self, value: Option<u64>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.u64(value);
        }
    }

    fn optional_u32(&mut self, value: Option<u32>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.u32(value);
        }
    }

    fn optional_text(&mut self, value: Option<&str>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.text(value);
        }
    }
}

impl WorldSnapshot {
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

        out.text("rad-operational-world/v2");
        out.u32(self.next_id);
        out.bool(self.fresh_ids_exhausted);
        out.usize(self.free_ids.len());
        for id in &self.free_ids {
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
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(name: &str) -> ComponentData {
        ComponentData {
            type_name: name.to_string(),
            layout: std::sync::Arc::new(Vec::new()),
            values: Vec::new(),
        }
    }

    #[test]
    fn spawn_entity_with_name_sets_bidirectional_maps() {
        let mut w = World::new();
        let e = w.spawn_entity(Some("hero"));
        assert_eq!(w.name_to_id.get("hero"), Some(&e));
        assert_eq!(w.id_to_name.get(&e), Some(&"hero".to_string()));
    }

    #[test]
    fn duplicate_name_cleans_up_old_entity_mapping() {
        let mut w = World::new();
        let e0 = w.spawn_entity(Some("player"));
        let e1 = w.spawn_entity(Some("player"));
        assert_ne!(e0, e1);
        assert_eq!(w.name_to_id.get("player"), Some(&e1));
        assert_eq!(w.id_to_name.get(&e1), Some(&"player".to_string()));
        assert!(!w.id_to_name.contains_key(&e0));
    }

    #[test]
    fn destroy_after_name_reuse_cleans_up_correctly() {
        let mut w = World::new();
        let e0 = w.spawn_entity(Some("npc"));
        let e1 = w.spawn_entity(Some("npc"));
        w.destroy_entity(e1);
        assert!(!w.name_to_id.contains_key("npc"));
        assert!(!w.id_to_name.contains_key(&e1));
        assert!(!w.id_to_name.contains_key(&e0));
    }

    #[test]
    fn destroy_old_entity_after_name_reuse_does_not_remove_new_mapping() {
        let mut w = World::new();
        let e0 = w.spawn_entity(Some("npc"));
        let _e1 = w.spawn_entity(Some("npc"));
        w.destroy_entity(e0);
        assert_eq!(w.name_to_id.get("npc"), Some(&_e1));
        assert_eq!(w.id_to_name.get(&_e1), Some(&"npc".to_string()));
    }

    #[test]
    fn spawn_unnamed_entity_does_not_pollute_name_maps() {
        let mut w = World::new();
        let e = w.spawn_entity(None);
        assert!(w.name_to_id.is_empty());
        assert!(w.id_to_name.is_empty());
        assert!(w.entity_archetype.contains_key(&e));
    }

    #[test]
    fn spawn_empty_name_does_not_pollute_name_maps() {
        let mut w = World::new();
        w.spawn_entity(Some(""));
        assert!(w.name_to_id.is_empty());
        assert!(w.id_to_name.is_empty());
    }

    #[test]
    fn destroyed_entity_id_is_reused() {
        let mut w = World::new();
        let e0 = w.spawn_entity(None);
        w.destroy_entity(e0);
        let e1 = w.spawn_entity(None);
        assert_eq!(e0, e1);
    }

    #[test]
    fn add_and_get_component() {
        let mut w = World::new();
        let e = w.spawn_entity(Some("hero"));
        w.add_component(
            e,
            ComponentData {
                type_name: "Health".to_string(),
                layout: std::sync::Arc::new(vec!["hp".to_string()]),
                values: vec![crate::value::Value::int(100)],
            },
        );
        let c = w.get_component(e, "Health").unwrap();
        assert_eq!(c.type_name, "Health");
    }

    #[test]
    fn add_component_on_invalid_entity_is_noop() {
        let mut w = World::new();
        w.add_component(999, comp("Health"));
        assert!(w.get_component(999, "Health").is_none());
    }

    #[test]
    fn remove_component_clears_from_query() {
        let mut w = World::new();
        let e = w.spawn_entity(None);
        w.add_component(e, comp("Pos"));
        assert!(w.has_component(e, "Pos"));
        w.remove_component(e, "Pos");
        assert!(!w.has_component(e, "Pos"));
        assert!(w.query(&["Pos".to_string()], &[]).is_empty());
    }

    #[test]
    fn query_returns_entities_with_all_requested_components() {
        let mut w = World::new();
        let e0 = w.spawn_entity(None);
        let e1 = w.spawn_entity(None);
        w.add_component(e0, comp("Pos"));
        w.add_component(e0, comp("Vel"));
        w.add_component(e1, comp("Pos"));
        let both = w.query(&["Pos".to_string(), "Vel".to_string()], &[]);
        assert_eq!(both, vec![e0]);
        let just_pos = w.query(&["Pos".to_string()], &[]);
        assert!(just_pos.contains(&e0));
        assert!(just_pos.contains(&e1));
    }

    #[test]
    fn destroy_entity_removes_from_all_queries() {
        let mut w = World::new();
        let e = w.spawn_entity(Some("tmp"));
        w.add_component(e, comp("Pos"));
        w.destroy_entity(e);
        assert!(w.query(&["Pos".to_string()], &[]).is_empty());
        assert!(w.get_component(e, "Pos").is_none());
        assert!(!w.name_to_id.contains_key("tmp"));
    }

    #[test]
    fn archetype_migration_preserves_existing_components() {
        let mut w = World::new();
        let e = w.spawn_entity(None);
        w.add_component(
            e,
            ComponentData {
                type_name: "Pos".to_string(),
                layout: std::sync::Arc::new(vec!["x".to_string()]),
                values: vec![crate::value::Value::int(10)],
            },
        );
        w.add_component(e, comp("Vel"));
        let c = w.get_component(e, "Pos").unwrap();
        assert_eq!(c.values[0].as_int(), Some(10));
        assert!(w.has_component(e, "Vel"));
    }

    #[test]
    fn set_component_overwrites_in_place() {
        let mut w = World::new();
        let e = w.spawn_entity(None);
        let layout = std::sync::Arc::new(vec!["hp".to_string()]);
        w.add_component(
            e,
            ComponentData {
                type_name: "Health".to_string(),
                layout: layout.clone(),
                values: vec![crate::value::Value::int(100)],
            },
        );
        w.set_component(
            e,
            ComponentData {
                type_name: "Health".to_string(),
                layout,
                values: vec![crate::value::Value::int(50)],
            },
        );
        let c = w.get_component(e, "Health").unwrap();
        assert_eq!(c.values[0].as_int(), Some(50));
    }

    #[test]
    fn query_with_many_archetypes() {
        let mut w = World::new();
        let e0 = w.spawn_entity(None);
        let e1 = w.spawn_entity(None);
        let e2 = w.spawn_entity(None);
        w.add_component(e0, comp("A"));
        w.add_component(e0, comp("B"));
        w.add_component(e1, comp("A"));
        w.add_component(e1, comp("B"));
        w.add_component(e1, comp("C"));
        w.add_component(e2, comp("A"));
        let q_a = w.query(&["A".to_string()], &[]);
        assert_eq!(q_a.len(), 3);
        let q_ab = w.query(&["A".to_string(), "B".to_string()], &[]);
        assert_eq!(q_ab.len(), 2);
        let q_abc = w.query(&["A".to_string(), "B".to_string(), "C".to_string()], &[]);
        assert_eq!(q_abc, vec![e1]);
    }

    #[test]
    fn snapshot_keeps_string_field_alive_after_overwrite() {
        let mut w = World::new();
        let mut gc = crate::gc::GcHeap::new();
        let e = w.spawn_entity(None);
        let layout = std::sync::Arc::new(vec!["name".to_string()]);

        w.add_component(
            e,
            ComponentData {
                type_name: "Name".to_string(),
                layout: layout.clone(),
                values: vec![crate::value::Value::from_string(&mut gc, "old".to_string())],
            },
        );

        let snap = w.snapshot();

        w.set_component(
            e,
            ComponentData {
                type_name: "Name".to_string(),
                layout: layout.clone(),
                values: vec![crate::value::Value::from_string(&mut gc, "new".to_string())],
            },
        );

        let live = w.get_component(e, "Name").unwrap();
        assert_eq!(live.values[0].as_str(), Some("new"));

        let snap_val = snap.get_component(e, "Name").unwrap();
        assert_eq!(snap_val.values[0].as_str(), Some("old"));
    }

    #[test]
    fn indexed_lookup_finds_entity_by_component_field() {
        let mut w = World::new();
        let mut indexed = std::collections::HashMap::new();
        indexed.insert(
            "Tag".to_string(),
            std::collections::HashSet::from(["name".to_string()]),
        );
        w.set_indexed_fields(indexed);
        let e = w.spawn_entity(None);
        w.add_component(
            e,
            ComponentData {
                type_name: "Tag".to_string(),
                layout: std::sync::Arc::new(vec!["name".to_string()]),
                values: vec![crate::value::Value::int(7)],
            },
        );
        assert_eq!(
            w.index_lookup("Tag", "name", crate::value::Value::int(7)),
            Some(e)
        );
    }

    #[test]
    fn indexed_lookup_updates_after_component_overwrite() {
        let mut w = World::new();
        let mut indexed = std::collections::HashMap::new();
        indexed.insert(
            "Tag".to_string(),
            std::collections::HashSet::from(["name".to_string()]),
        );
        w.set_indexed_fields(indexed);
        let e = w.spawn_entity(None);
        let layout = std::sync::Arc::new(vec!["name".to_string()]);
        w.add_component(
            e,
            ComponentData {
                type_name: "Tag".to_string(),
                layout: layout.clone(),
                values: vec![crate::value::Value::int(1)],
            },
        );
        w.set_component(
            e,
            ComponentData {
                type_name: "Tag".to_string(),
                layout,
                values: vec![crate::value::Value::int(2)],
            },
        );
        assert_eq!(
            w.index_lookup("Tag", "name", crate::value::Value::int(1)),
            None
        );
        assert_eq!(
            w.index_lookup("Tag", "name", crate::value::Value::int(2)),
            Some(e)
        );
    }

    #[test]
    fn indexed_lookup_supports_float_values() {
        let mut w = World::new();
        let mut indexed = std::collections::HashMap::new();
        indexed.insert(
            "Tag".to_string(),
            std::collections::HashSet::from(["score".to_string()]),
        );
        w.set_indexed_fields(indexed);
        let e = w.spawn_entity(None);
        w.add_component(
            e,
            ComponentData {
                type_name: "Tag".to_string(),
                layout: std::sync::Arc::new(vec!["score".to_string()]),
                values: vec![crate::value::Value::from_float(3.5)],
            },
        );
        assert_eq!(
            w.index_lookup("Tag", "score", crate::value::Value::from_float(3.5)),
            Some(e)
        );
    }
}
