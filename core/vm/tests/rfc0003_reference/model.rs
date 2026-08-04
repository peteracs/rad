// Authoritative typed values, schemas, relation patches, entity allocation,
// and construction of one atomic component/relation candidate.

type OracleResult<T> = Result<T, &'static str>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct EntityRef {
    slot: u32,
    generation: u32,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum FactValue {
    Int(i64),
    Count(u64),
    Entity(EntityRef),
    Text(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValueKind {
    Int,
    Count,
    Entity,
    Text,
}

impl FactValue {
    fn kind(&self) -> ValueKind {
        match self {
            Self::Int(_) => ValueKind::Int,
            Self::Count(_) => ValueKind::Count,
            Self::Entity(_) => ValueKind::Entity,
            Self::Text(_) => ValueKind::Text,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeletePolicy {
    Restrict,
    Cascade,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ColumnSchema {
    name: String,
    kind: ValueKind,
    on_delete: Option<DeletePolicy>,
}

impl ColumnSchema {
    fn new(name: &str, kind: ValueKind) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            on_delete: (kind == ValueKind::Entity).then_some(DeletePolicy::Restrict),
        }
    }

    fn cascade(mut self) -> Self {
        self.on_delete = Some(DeletePolicy::Cascade);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UniqueConstraint {
    name: String,
    columns: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RelationSchema {
    name: String,
    columns: Vec<ColumnSchema>,
    unique: Vec<UniqueConstraint>,
    symmetric: bool,
}

impl RelationSchema {
    fn new(name: &str, columns: Vec<ColumnSchema>) -> Self {
        Self {
            name: name.to_owned(),
            columns,
            unique: Vec::new(),
            symmetric: false,
        }
    }

    fn unique(mut self, name: &str, columns: &[usize]) -> Self {
        self.unique.push(UniqueConstraint {
            name: name.to_owned(),
            columns: columns.to_vec(),
        });
        self
    }

    fn symmetric(mut self) -> Self {
        self.symmetric = true;
        self
    }

    fn validate_declaration(&self) -> OracleResult<()> {
        if self.name.is_empty() {
            return Err("relation.empty_name");
        }
        let mut column_names = BTreeSet::new();
        for column in &self.columns {
            if column.name.is_empty() {
                return Err("relation.empty_column_name");
            }
            if !column_names.insert(&column.name) {
                return Err("relation.duplicate_column_name");
            }
            if column.kind != ValueKind::Entity && column.on_delete.is_some() {
                return Err("relation.delete_policy_non_entity");
            }
        }
        if self.symmetric {
            if self.columns.len() != 2 || self.columns[0].kind != self.columns[1].kind {
                return Err("relation.symmetric_shape");
            }
            if !self.unique.is_empty() {
                return Err("relation.symmetric_unique_forbidden");
            }
            if self.columns[0].on_delete != self.columns[1].on_delete {
                return Err("relation.symmetric_endpoint_metadata");
            }
        }
        let mut unique_names = BTreeSet::new();
        for unique in &self.unique {
            if unique.name.is_empty() {
                return Err("relation.empty_unique_name");
            }
            if !unique_names.insert(&unique.name) {
                return Err("relation.duplicate_unique_name");
            }
            let unique_columns = unique.columns.iter().copied().collect::<BTreeSet<_>>();
            if unique.columns.is_empty()
                || unique_columns.len() != unique.columns.len()
                || unique
                    .columns
                    .iter()
                    .any(|column| *column >= self.columns.len())
            {
                return Err("relation.unique_shape");
            }
        }
        Ok(())
    }

    fn canonical_tuple(&self, mut tuple: Vec<FactValue>) -> OracleResult<Vec<FactValue>> {
        if tuple.len() != self.columns.len() {
            return Err("relation.arity");
        }
        if tuple
            .iter()
            .zip(&self.columns)
            .any(|(value, column)| value.kind() != column.kind)
        {
            return Err("relation.type");
        }
        if self.symmetric && tuple[1] < tuple[0] {
            tuple.swap(0, 1);
        }
        Ok(tuple)
    }

    fn unique_constraint(&self, name: &str) -> OracleResult<&UniqueConstraint> {
        self.unique
            .iter()
            .find(|unique| unique.name == name)
            .ok_or("relation.unknown_unique")
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FactKey {
    relation: String,
    tuple: Vec<FactValue>,
}

impl FactKey {
    fn new(relation: &str, tuple: Vec<FactValue>) -> Self {
        Self {
            relation: relation.to_owned(),
            tuple,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FactAssertion {
    id: u64,
    key: FactKey,
    causes: BTreeSet<String>,
    required_capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ChangeKind {
    Insert,
    Remove,
    Cascade,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FactChange {
    kind: ChangeKind,
    key: FactKey,
    causes: BTreeSet<String>,
    required_capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct EntityTable {
    live: BTreeSet<EntityRef>,
    generations: BTreeMap<u32, u32>,
    free_slots: BTreeSet<u32>,
    retired_slots: BTreeSet<u32>,
    next_slot: u32,
    fresh_slots_exhausted: bool,
}

impl EntityTable {
    fn spawn(&mut self) -> OracleResult<EntityRef> {
        let mut reusable = None;
        while let Some(slot) = self.free_slots.iter().next().copied() {
            self.free_slots.remove(&slot);
            let generation = self.generations.get(&slot).copied().unwrap_or(0);
            if generation == u32::MAX {
                self.retired_slots.insert(slot);
                continue;
            }
            reusable = Some((slot, generation + 1));
            break;
        }
        let (slot, generation) = match reusable {
            Some(reusable) => reusable,
            None => {
                if self.fresh_slots_exhausted {
                    return Err("entity.id_space_exhausted");
                }
                let slot = self.next_slot;
                if slot == u32::MAX {
                    self.fresh_slots_exhausted = true;
                } else {
                    self.next_slot = slot + 1;
                }
                (slot, 0)
            }
        };
        self.generations.insert(slot, generation);
        let entity = EntityRef { slot, generation };
        self.live.insert(entity);
        Ok(entity)
    }

    fn despawn(&mut self, entity: EntityRef) -> OracleResult<()> {
        if !self.live.remove(&entity) {
            return Err("entity.not_live");
        }
        if entity.generation == u32::MAX {
            self.retired_slots.insert(entity.slot);
        } else {
            self.free_slots.insert(entity.slot);
        }
        Ok(())
    }

    fn contains(&self, entity: EntityRef) -> bool {
        self.live.contains(&entity)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct RelationStore {
    schemas: BTreeMap<String, RelationSchema>,
    assertions: BTreeMap<FactKey, FactAssertion>,
    next_assertion_id: u64,
    last_changes: Vec<FactChange>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct OperationMetadata {
    causes: BTreeSet<String>,
    required_capabilities: BTreeSet<String>,
}

impl OperationMetadata {
    fn cause(cause: &str) -> Self {
        Self {
            causes: BTreeSet::from([cause.to_owned()]),
            required_capabilities: BTreeSet::new(),
        }
    }

    fn with_capability(mut self, capability: &str) -> Self {
        self.required_capabilities.insert(capability.to_owned());
        self
    }

    fn merge(&mut self, other: &Self) {
        self.causes.extend(other.causes.iter().cloned());
        self.required_capabilities
            .extend(other.required_capabilities.iter().cloned());
    }
}

#[derive(Clone, Debug)]
enum RelationOperation {
    Insert(FactKey, OperationMetadata),
    Remove(FactKey, OperationMetadata),
    ReplaceBy {
        relation: String,
        unique_constraint: String,
        selected_key: Vec<FactValue>,
        tuple: Vec<FactValue>,
        metadata: OperationMetadata,
    },
}

#[derive(Clone, Debug, Default)]
struct NormalizedAction {
    insert: Option<OperationMetadata>,
    remove: Option<OperationMetadata>,
}

impl NormalizedAction {
    fn insert(&mut self, metadata: &OperationMetadata) {
        match &mut self.insert {
            Some(existing) => existing.merge(metadata),
            None => self.insert = Some(metadata.clone()),
        }
    }

    fn remove(&mut self, metadata: &OperationMetadata) {
        match &mut self.remove {
            Some(existing) => existing.merge(metadata),
            None => self.remove = Some(metadata.clone()),
        }
    }
}

impl RelationStore {
    fn register(&mut self, schema: RelationSchema) -> OracleResult<()> {
        schema.validate_declaration()?;
        if self.schemas.contains_key(&schema.name) {
            return Err("relation.duplicate_schema");
        }
        self.schemas.insert(schema.name.clone(), schema);
        Ok(())
    }

    fn canonical_key(&self, key: FactKey) -> OracleResult<FactKey> {
        let schema = self.schemas.get(&key.relation).ok_or("relation.unknown")?;
        Ok(FactKey::new(
            &key.relation,
            schema.canonical_tuple(key.tuple)?,
        ))
    }

    fn add_action(
        actions: &mut BTreeMap<FactKey, NormalizedAction>,
        key: FactKey,
        insert: bool,
        metadata: &OperationMetadata,
    ) {
        let action = actions.entry(key).or_default();
        if insert {
            action.insert(metadata);
        } else {
            action.remove(metadata);
        }
    }

    fn apply_provisional_operations(
        &mut self,
        operations: Vec<RelationOperation>,
    ) -> OracleResult<()> {
        self.last_changes.clear();
        let mut actions = BTreeMap::<FactKey, NormalizedAction>::new();
        let mut replacements =
            BTreeMap::<(String, String, Vec<FactValue>), (FactKey, OperationMetadata)>::new();

        for operation in operations {
            match operation {
                RelationOperation::Insert(key, metadata) => {
                    let key = self.canonical_key(key)?;
                    Self::add_action(&mut actions, key, true, &metadata);
                }
                RelationOperation::Remove(key, metadata) => {
                    let key = self.canonical_key(key)?;
                    Self::add_action(&mut actions, key, false, &metadata);
                }
                RelationOperation::ReplaceBy {
                    relation,
                    unique_constraint,
                    selected_key,
                    tuple,
                    metadata,
                } => {
                    let schema = self.schemas.get(&relation).ok_or("relation.unknown")?;
                    let unique = schema.unique_constraint(&unique_constraint)?;
                    if selected_key.len() != unique.columns.len() {
                        return Err("relation.replace_key_shape");
                    }
                    let target = FactKey::new(&relation, schema.canonical_tuple(tuple)?);
                    let target_key = unique
                        .columns
                        .iter()
                        .map(|column| target.tuple[*column].clone())
                        .collect::<Vec<_>>();
                    if target_key != selected_key {
                        return Err("relation.replace_key_mismatch");
                    }
                    let identity = (relation, unique_constraint, selected_key);
                    match replacements.get_mut(&identity) {
                        Some((existing_target, existing_metadata)) => {
                            if *existing_target != target {
                                return Err("relation.replacement_conflict");
                            }
                            existing_metadata.merge(&metadata);
                        }
                        None => {
                            replacements.insert(identity, (target, metadata));
                        }
                    }
                }
            }
        }

        for ((relation, unique_name, selected_key), (target, metadata)) in replacements {
            let schema = &self.schemas[&relation];
            let unique = schema.unique_constraint(&unique_name)?;
            let existing = self.assertions.keys().find(|fact| {
                fact.relation == relation
                    && unique
                        .columns
                        .iter()
                        .map(|column| fact.tuple[*column].clone())
                        .collect::<Vec<_>>()
                        == selected_key
            });
            if existing == Some(&target) {
                continue;
            }
            if let Some(existing) = existing {
                Self::add_action(&mut actions, existing.clone(), false, &metadata);
            }
            Self::add_action(&mut actions, target, true, &metadata);
        }

        if actions
            .values()
            .any(|action| action.insert.is_some() && action.remove.is_some())
        {
            return Err("relation.operation_conflict");
        }

        let mut candidate = self.assertions.clone();
        let mut changes = Vec::new();
        for (key, action) in &actions {
            if let Some(metadata) = &action.remove {
                if candidate.remove(key).is_some() {
                    changes.push(FactChange {
                        kind: ChangeKind::Remove,
                        key: key.clone(),
                        causes: metadata.causes.clone(),
                        required_capabilities: metadata.required_capabilities.clone(),
                    });
                }
            }
        }
        for (key, action) in &actions {
            if let Some(metadata) = &action.insert {
                if !candidate.contains_key(key) {
                    candidate.insert(
                        key.clone(),
                        FactAssertion {
                            // Candidate-only identity. WorldModel allocates a
                            // durable assertion version only after cascades and
                            // final schema validation.
                            id: 0,
                            key: key.clone(),
                            causes: metadata.causes.clone(),
                            required_capabilities: metadata.required_capabilities.clone(),
                        },
                    );
                    changes.push(FactChange {
                        kind: ChangeKind::Insert,
                        key: key.clone(),
                        causes: metadata.causes.clone(),
                        required_capabilities: metadata.required_capabilities.clone(),
                    });
                }
            }
        }

        self.assertions = candidate;
        self.last_changes = changes;
        Ok(())
    }

    fn validate_unique(&self, candidate: &BTreeMap<FactKey, FactAssertion>) -> OracleResult<()> {
        for schema in self.schemas.values() {
            for unique in &schema.unique {
                let mut seen = BTreeSet::new();
                for fact in candidate.keys().filter(|fact| fact.relation == schema.name) {
                    let key = unique
                        .columns
                        .iter()
                        .map(|column| fact.tuple[*column].clone())
                        .collect::<Vec<_>>();
                    if !seen.insert(key) {
                        return Err("relation.unique_conflict");
                    }
                }
            }
        }
        Ok(())
    }

    fn remove_cascade(&mut self, key: &FactKey, metadata: &OperationMetadata) {
        if self.assertions.remove(key).is_some() {
            self.last_changes.push(FactChange {
                kind: ChangeKind::Cascade,
                key: key.clone(),
                causes: metadata.causes.clone(),
                required_capabilities: metadata.required_capabilities.clone(),
            });
        }
    }

    fn logical_rows(&self, relation: &str) -> OracleResult<Vec<LogicalRow>> {
        self.logical_rows_metered(relation, None)
    }

    fn logical_rows_metered(
        &self,
        relation: &str,
        mut meter: Option<&mut DerivationMeter>,
    ) -> OracleResult<Vec<LogicalRow>> {
        let schema = self.schemas.get(relation).ok_or("relation.unknown")?;
        let mut rows = Vec::new();
        for assertion in self
            .assertions
            .values()
            .filter(|assertion| assertion.key.relation == relation)
        {
            let bytes = encoded_authoritative_row_len(assertion)?;
            if let Some(meter) = meter.as_deref_mut() {
                meter.charge_intermediate_bytes(bytes)?;
            }
            let support = SupportRef::Authoritative {
                key: assertion.key.clone(),
                assertion_id: assertion.id,
                required_capabilities: assertion.required_capabilities.clone(),
            };
            rows.push(LogicalRow {
                tuple: assertion.key.tuple.clone(),
                support: support.clone(),
            });
            if schema.symmetric && assertion.key.tuple[0] != assertion.key.tuple[1] {
                if let Some(meter) = meter.as_deref_mut() {
                    meter.charge_intermediate_bytes(bytes)?;
                }
                rows.push(LogicalRow {
                    tuple: vec![
                        assertion.key.tuple[1].clone(),
                        assertion.key.tuple[0].clone(),
                    ],
                    support,
                });
            }
        }
        rows.sort();
        Ok(rows)
    }

    fn contains_ground(&self, key: FactKey) -> OracleResult<bool> {
        Ok(self.assertions.contains_key(&self.canonical_key(key)?))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum EntityOperand {
    Existing(EntityRef),
    Candidate(u32),
}

#[derive(Clone, Debug)]
enum PendingValue {
    Int(i64),
    Count(u64),
    Entity(EntityOperand),
    Text(String),
}

impl PendingValue {
    fn resolve(&self, handles: &BTreeMap<u32, EntityRef>) -> OracleResult<FactValue> {
        Ok(match self {
            Self::Int(value) => FactValue::Int(*value),
            Self::Count(value) => FactValue::Count(*value),
            Self::Text(value) => FactValue::Text(value.clone()),
            Self::Entity(EntityOperand::Existing(entity)) => FactValue::Entity(*entity),
            Self::Entity(EntityOperand::Candidate(handle)) => FactValue::Entity(
                *handles
                    .get(handle)
                    .ok_or("entity.unknown_candidate_handle")?,
            ),
        })
    }
}

#[derive(Clone, Debug)]
struct PendingFactKey {
    relation: String,
    tuple: Vec<PendingValue>,
}

impl PendingFactKey {
    fn resolve(&self, handles: &BTreeMap<u32, EntityRef>) -> OracleResult<FactKey> {
        Ok(FactKey::new(
            &self.relation,
            self.tuple
                .iter()
                .map(|value| value.resolve(handles))
                .collect::<OracleResult<Vec<_>>>()?,
        ))
    }
}

#[derive(Clone, Debug)]
enum PendingOperation {
    Insert(PendingFactKey, OperationMetadata),
    Remove(PendingFactKey, OperationMetadata),
    ReplaceBy {
        relation: String,
        unique_constraint: String,
        selected_key: Vec<PendingValue>,
        tuple: Vec<PendingValue>,
        metadata: OperationMetadata,
    },
}

impl PendingOperation {
    fn resolve(&self, handles: &BTreeMap<u32, EntityRef>) -> OracleResult<RelationOperation> {
        Ok(match self {
            Self::Insert(key, metadata) => {
                RelationOperation::Insert(key.resolve(handles)?, metadata.clone())
            }
            Self::Remove(key, metadata) => {
                RelationOperation::Remove(key.resolve(handles)?, metadata.clone())
            }
            Self::ReplaceBy {
                relation,
                unique_constraint,
                selected_key,
                tuple,
                metadata,
            } => RelationOperation::ReplaceBy {
                relation: relation.clone(),
                unique_constraint: unique_constraint.clone(),
                selected_key: selected_key
                    .iter()
                    .map(|value| value.resolve(handles))
                    .collect::<OracleResult<Vec<_>>>()?,
                tuple: tuple
                    .iter()
                    .map(|value| value.resolve(handles))
                    .collect::<OracleResult<Vec<_>>>()?,
                metadata: metadata.clone(),
            },
        })
    }
}

#[derive(Clone, Debug, Default)]
struct Transaction {
    spawn_handles: BTreeSet<u32>,
    despawns: Vec<PendingDespawn>,
    component_writes: Vec<PendingComponentWrite>,
    operations: Vec<PendingOperation>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct WorldModel {
    entities: EntityTable,
    components: BTreeMap<(EntityRef, String), FactValue>,
    relations: RelationStore,
}

#[derive(Clone, Debug)]
struct PendingComponentWrite {
    entity: EntityOperand,
    component: String,
    value: FactValue,
}

#[derive(Clone, Debug)]
struct PendingDespawn {
    entity: EntityRef,
    metadata: OperationMetadata,
}

fn despawn(entity: EntityRef, cause: &str) -> PendingDespawn {
    PendingDespawn {
        entity,
        metadata: OperationMetadata::cause(cause),
    }
}

impl WorldModel {
    fn apply_transaction(
        &mut self,
        transaction: Transaction,
    ) -> OracleResult<BTreeMap<u32, EntityRef>> {
        let mut candidate = self.clone();
        let base_fact_keys = self
            .relations
            .assertions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let base_next_assertion_id = self.relations.next_assertion_id;
        let mut handles = BTreeMap::new();
        for handle in transaction.spawn_handles {
            handles.insert(handle, candidate.entities.spawn()?);
        }
        let mut component_writes = BTreeMap::<(EntityRef, String), FactValue>::new();
        for write in transaction.component_writes {
            let entity = match write.entity {
                EntityOperand::Existing(entity) => entity,
                EntityOperand::Candidate(handle) => *handles
                    .get(&handle)
                    .ok_or("entity.unknown_candidate_handle")?,
            };
            if !candidate.entities.contains(entity) {
                return Err("component.entity_not_live");
            }
            let key = (entity, write.component);
            match component_writes.get(&key) {
                Some(value) if value != &write.value => {
                    return Err("component.write_conflict");
                }
                Some(_) => {}
                None => {
                    component_writes.insert(key, write.value);
                }
            }
        }
        for (key, value) in component_writes {
            candidate.components.insert(key, value);
        }
        let operations = transaction
            .operations
            .iter()
            .map(|operation| operation.resolve(&handles))
            .collect::<OracleResult<Vec<_>>>()?;
        candidate
            .relations
            .apply_provisional_operations(operations)?;

        let mut despawns = BTreeMap::<EntityRef, OperationMetadata>::new();
        for despawn in transaction.despawns {
            if !candidate.entities.contains(despawn.entity) {
                return Err("entity.not_live");
            }
            despawns
                .entry(despawn.entity)
                .or_default()
                .merge(&despawn.metadata);
        }

        // Classify the complete candidate against the complete despawn set
        // before applying any implicit cascade. A cascade caused by one
        // endpoint can therefore never hide a restricting endpoint.
        let mut cascades = BTreeMap::<FactKey, OperationMetadata>::new();
        for fact in candidate.relations.assertions.keys() {
            let schema = &candidate.relations.schemas[&fact.relation];
            let mut cascade_metadata = OperationMetadata::default();
            let mut referenced = false;
            let mut restricted = false;
            for (value, column) in fact.tuple.iter().zip(&schema.columns) {
                let FactValue::Entity(entity) = value else {
                    continue;
                };
                let Some(metadata) = despawns.get(entity) else {
                    continue;
                };
                referenced = true;
                restricted |= column.on_delete == Some(DeletePolicy::Restrict);
                cascade_metadata.merge(metadata);
            }
            if referenced {
                if restricted {
                    return Err("entity.delete_restricted");
                }
                cascades.insert(fact.clone(), cascade_metadata);
            }
        }
        for (fact, metadata) in cascades {
            candidate.relations.remove_cascade(&fact, &metadata);
        }
        for entity in despawns.keys().copied() {
            candidate.entities.despawn(entity)?;
            candidate
                .components
                .retain(|(component_entity, _), _| *component_entity != entity);
        }

        for fact in candidate.relations.assertions.keys() {
            for value in &fact.tuple {
                if let FactValue::Entity(entity) = value {
                    if !candidate.entities.contains(*entity) {
                        return Err("relation.dangling_entity");
                    }
                }
            }
        }
        candidate
            .relations
            .validate_unique(&candidate.relations.assertions)?;

        // Assertion versions name committed lifetimes. Candidate-only rows
        // removed by a same-candidate cascade consume no assertion ID and
        // leave no durable insert/remove record.
        let new_fact_keys = candidate
            .relations
            .assertions
            .keys()
            .filter(|key| !base_fact_keys.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        for (offset, key) in new_fact_keys.iter().enumerate() {
            candidate.relations.assertions.get_mut(key).unwrap().id = base_next_assertion_id
                .checked_add(u64::try_from(offset).map_err(|_| "relation.assertion_id_overflow")?)
                .ok_or("relation.assertion_id_overflow")?;
        }
        candidate.relations.next_assertion_id = base_next_assertion_id
            .checked_add(
                u64::try_from(new_fact_keys.len()).map_err(|_| "relation.assertion_id_overflow")?,
            )
            .ok_or("relation.assertion_id_overflow")?;
        let final_fact_keys = candidate
            .relations
            .assertions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        candidate.relations.last_changes.retain(|change| {
            base_fact_keys.contains(&change.key) || final_fact_keys.contains(&change.key)
        });
        *self = candidate;
        Ok(handles)
    }
}

fn pending_key(relation: &str, tuple: Vec<PendingValue>) -> PendingFactKey {
    PendingFactKey {
        relation: relation.to_owned(),
        tuple,
    }
}

fn existing(entity: EntityRef) -> PendingValue {
    PendingValue::Entity(EntityOperand::Existing(entity))
}

fn candidate(handle: u32) -> PendingValue {
    PendingValue::Entity(EntityOperand::Candidate(handle))
}

fn int(value: i64) -> PendingValue {
    PendingValue::Int(value)
}

fn insert(key: PendingFactKey, cause: &str) -> PendingOperation {
    PendingOperation::Insert(key, OperationMetadata::cause(cause))
}

fn remove(key: PendingFactKey, cause: &str) -> PendingOperation {
    PendingOperation::Remove(key, OperationMetadata::cause(cause))
}
