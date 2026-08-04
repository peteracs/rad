//! Executable semantic oracle for RFC-0003.
//!
//! This deliberately contains no parser, bytecode, VM, GC, or ECS code. It is
//! a generic typed relation model. Full recomputation defines derivation
//! semantics; the dependency maintainer is differential-tested against it.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

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
    next_slot: u32,
}

impl EntityTable {
    fn spawn(&mut self) -> EntityRef {
        let reusable = self.free_slots.iter().next().copied();
        let slot = match reusable {
            Some(slot) => {
                self.free_slots.remove(&slot);
                slot
            }
            None => {
                let slot = self.next_slot;
                self.next_slot = self.next_slot.checked_add(1).expect("fixture slot space");
                slot
            }
        };
        let generation = self.generations.entry(slot).or_insert(0);
        if reusable.is_some() {
            *generation = generation.checked_add(1).expect("fixture generation space");
        }
        let entity = EntityRef {
            slot,
            generation: *generation,
        };
        self.live.insert(entity);
        entity
    }

    fn despawn(&mut self, entity: EntityRef) -> OracleResult<()> {
        if !self.live.remove(&entity) {
            return Err("entity.not_live");
        }
        self.free_slots.insert(entity.slot);
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
        let handles = transaction
            .spawn_handles
            .into_iter()
            .map(|handle| (handle, candidate.entities.spawn()))
            .collect::<BTreeMap<_, _>>();
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
            candidate
                .components
                .insert((entity, write.component), write.value);
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SupportRef {
    Authoritative {
        key: FactKey,
        assertion_id: u64,
        required_capabilities: BTreeSet<String>,
    },
    Derived {
        key: FactKey,
        proof_id: String,
        required_capabilities: BTreeSet<String>,
        proof_depth: usize,
    },
}

impl SupportRef {
    fn required_capabilities(&self) -> &BTreeSet<String> {
        match self {
            Self::Authoritative {
                required_capabilities,
                ..
            }
            | Self::Derived {
                required_capabilities,
                ..
            } => required_capabilities,
        }
    }

    fn key(&self) -> &FactKey {
        match self {
            Self::Authoritative { key, .. } | Self::Derived { key, .. } => key,
        }
    }

    fn depth(&self) -> usize {
        match self {
            Self::Authoritative { .. } => 1,
            Self::Derived { proof_depth, .. } => *proof_depth,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct LogicalRow {
    tuple: Vec<FactValue>,
    support: SupportRef,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProofAlternative {
    rule: String,
    bindings: BTreeMap<String, FactValue>,
    supports: BTreeSet<SupportRef>,
    aggregate_group: Option<Vec<FactValue>>,
    required_capabilities: BTreeSet<String>,
    depth: usize,
}

impl ProofAlternative {
    fn canonical_len(&self) -> OracleResult<usize> {
        let mut length = encoded_text_len(&self.rule)?;
        length = checked_len_add(length, 8)?;
        for (name, value) in &self.bindings {
            length = checked_len_add(length, encoded_text_len(name)?)?;
            length = checked_len_add(length, encoded_value_len(value)?)?;
        }
        length = checked_len_add(length, 1)?;
        if let Some(group) = &self.aggregate_group {
            length = checked_len_add(length, 8)?;
            for value in group {
                length = checked_len_add(length, encoded_value_len(value)?)?;
            }
        }
        length = checked_len_add(length, 8)?;
        for support in &self.supports {
            length = checked_len_add(length, encoded_fact_key_len(support.key())?)?;
            length = checked_len_add(length, 1)?;
            length = checked_len_add(
                length,
                match support {
                    SupportRef::Authoritative { .. } => 8,
                    SupportRef::Derived { proof_id, .. } => encoded_text_len(proof_id)?,
                },
            )?;
        }
        length = checked_len_add(length, 8)?;
        for capability in &self.required_capabilities {
            length = checked_len_add(length, encoded_text_len(capability)?)?;
        }
        checked_len_add(length, 8)
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_text(&mut bytes, &self.rule);
        write_u64(&mut bytes, self.bindings.len() as u64);
        for (name, value) in &self.bindings {
            write_text(&mut bytes, name);
            encode_value(&mut bytes, value);
        }
        match &self.aggregate_group {
            Some(group) => {
                bytes.push(1);
                write_u64(&mut bytes, group.len() as u64);
                for value in group {
                    encode_value(&mut bytes, value);
                }
            }
            None => bytes.push(0),
        }
        write_u64(&mut bytes, self.supports.len() as u64);
        for support in &self.supports {
            encode_fact_key(&mut bytes, support.key());
            match support {
                SupportRef::Authoritative { assertion_id, .. } => {
                    bytes.push(b'A');
                    write_u64(&mut bytes, *assertion_id);
                }
                SupportRef::Derived { proof_id, .. } => {
                    bytes.push(b'D');
                    write_text(&mut bytes, proof_id);
                }
            }
        }
        write_u64(&mut bytes, self.required_capabilities.len() as u64);
        for capability in &self.required_capabilities {
            write_text(&mut bytes, capability);
        }
        write_u64(&mut bytes, self.depth as u64);
        bytes
    }

    fn identity(&self) -> String {
        hex(&Sha256::digest(self.canonical_bytes()))
    }
}

type DerivationResult = BTreeMap<FactKey, BTreeSet<ProofAlternative>>;

#[derive(Clone, Debug)]
enum Term {
    Variable(String),
    Constant(FactValue),
}

impl Term {
    fn var(name: &str) -> Self {
        Self::Variable(name.to_owned())
    }
}

#[derive(Clone, Debug)]
struct Atom {
    relation: String,
    terms: Vec<Term>,
}

impl Atom {
    fn new(relation: &str, terms: Vec<Term>) -> Self {
        Self {
            relation: relation.to_owned(),
            terms,
        }
    }
}

#[derive(Clone, Debug)]
enum Predicate {
    Greater(String, String),
}

#[derive(Clone, Copy, Debug)]
enum AggregateKind {
    Count,
    Sum,
    Min,
    Max,
}

#[derive(Clone, Debug)]
struct AggregateSpec {
    kind: AggregateKind,
    input: Option<String>,
    output: String,
    group_by: Vec<String>,
}

#[derive(Clone, Debug)]
struct RulePlan {
    id: String,
    head_relation: String,
    head: Vec<Term>,
    atoms: Vec<Atom>,
    predicates: Vec<Predicate>,
    aggregate: Option<AggregateSpec>,
}

impl RulePlan {
    fn validate(
        &self,
        store: &RelationStore,
        derived_schemas: &BTreeMap<String, RelationSchema>,
    ) -> OracleResult<()> {
        if self.id.is_empty() {
            return Err("derivation.empty_rule_id");
        }
        if !self.id.contains('.') && !self.id.contains("::") {
            return Err("derivation.unqualified_rule_id");
        }
        let head_schema = derived_schemas
            .get(&self.head_relation)
            .ok_or("derivation.unknown_head")?;
        if self.head.len() != head_schema.columns.len() {
            return Err("derivation.head_arity");
        }

        let mut variable_types = BTreeMap::<String, ValueKind>::new();
        let mut atoms = self.atoms.iter().collect::<Vec<_>>();
        atoms.sort_by_key(|atom| atom_bytes(atom));
        for atom in atoms {
            let schema = store
                .schemas
                .get(&atom.relation)
                .or_else(|| derived_schemas.get(&atom.relation))
                .ok_or("derivation.unknown_atom")?;
            if atom.terms.len() != schema.columns.len() {
                return Err("derivation.atom_arity");
            }
            for (term, column) in atom.terms.iter().zip(&schema.columns) {
                match term {
                    Term::Constant(value) if value.kind() != column.kind => {
                        return Err("derivation.atom_type");
                    }
                    Term::Constant(_) => {}
                    Term::Variable(name) => match variable_types.get(name) {
                        Some(kind) if *kind != column.kind => {
                            return Err("derivation.variable_type");
                        }
                        Some(_) => {}
                        None => {
                            variable_types.insert(name.clone(), column.kind);
                        }
                    },
                }
            }
        }

        let mut predicates = self.predicates.iter().collect::<Vec<_>>();
        predicates.sort_by_key(|predicate| predicate_bytes(predicate));
        for predicate in predicates {
            match predicate {
                Predicate::Greater(left, right) => {
                    if variable_types.get(left) != Some(&ValueKind::Int)
                        || variable_types.get(right) != Some(&ValueKind::Int)
                    {
                        return Err("derivation.predicate_type");
                    }
                }
            }
        }

        let mut head_variable_types = variable_types.clone();
        if let Some(aggregate) = &self.aggregate {
            if self.atoms.is_empty() {
                return Err("derivation.aggregate_requires_positive_input");
            }
            let groups = aggregate.group_by.iter().cloned().collect::<BTreeSet<_>>();
            if groups.len() != aggregate.group_by.len() {
                return Err("derivation.duplicate_group");
            }
            if groups
                .iter()
                .any(|group| !variable_types.contains_key(group))
            {
                return Err("derivation.unbound_group");
            }
            if variable_types.contains_key(&aggregate.output)
                || groups.contains(&aggregate.output)
                || aggregate.input.as_ref() == Some(&aggregate.output)
            {
                return Err("derivation.aggregate_output_not_fresh");
            }
            let output_kind = match aggregate.kind {
                AggregateKind::Count => {
                    if aggregate.input.is_some() {
                        return Err("derivation.count_input");
                    }
                    ValueKind::Count
                }
                AggregateKind::Sum | AggregateKind::Min | AggregateKind::Max => {
                    let input = aggregate
                        .input
                        .as_ref()
                        .ok_or("derivation.aggregate_input")?;
                    if variable_types.get(input) != Some(&ValueKind::Int) {
                        return Err("derivation.aggregate_type");
                    }
                    ValueKind::Int
                }
            };
            head_variable_types.insert(aggregate.output.clone(), output_kind);
            let head_variables = self
                .head
                .iter()
                .filter_map(|term| match term {
                    Term::Variable(name) => Some(name.clone()),
                    Term::Constant(_) => None,
                })
                .collect::<BTreeSet<_>>();
            let mut expected = groups;
            expected.insert(aggregate.output.clone());
            if head_variables != expected
                || self
                    .head
                    .iter()
                    .filter(
                        |term| matches!(term, Term::Variable(name) if name == &aggregate.output),
                    )
                    .count()
                    != 1
            {
                return Err("derivation.aggregate_head_projection");
            }
        }

        for (term, column) in self.head.iter().zip(&head_schema.columns) {
            let kind = match term {
                Term::Constant(value) => value.kind(),
                Term::Variable(name) => *head_variable_types
                    .get(name)
                    .ok_or("derivation.unbound_variable")?,
            };
            if kind != column.kind {
                return Err("derivation.head_type");
            }
        }
        Ok(())
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = b"rfc0003.rule-plan.v1".to_vec();
        write_text(&mut out, &self.id);
        write_text(&mut out, &self.head_relation);
        write_terms(&mut out, &self.head);
        let mut atoms = self.atoms.iter().map(atom_bytes).collect::<Vec<_>>();
        atoms.sort();
        write_u64(&mut out, atoms.len() as u64);
        for atom in atoms {
            write_u64(&mut out, atom.len() as u64);
            out.extend_from_slice(&atom);
        }
        let mut predicates = self
            .predicates
            .iter()
            .map(predicate_bytes)
            .collect::<Vec<_>>();
        predicates.sort();
        write_u64(&mut out, predicates.len() as u64);
        for predicate in predicates {
            write_u64(&mut out, predicate.len() as u64);
            out.extend_from_slice(&predicate);
        }
        match &self.aggregate {
            None => out.push(0),
            Some(aggregate) => {
                out.push(1);
                out.push(match aggregate.kind {
                    AggregateKind::Count => b'c',
                    AggregateKind::Sum => b's',
                    AggregateKind::Min => b'n',
                    AggregateKind::Max => b'x',
                });
                match &aggregate.input {
                    None => out.push(0),
                    Some(input) => {
                        out.push(1);
                        write_text(&mut out, input);
                    }
                }
                write_text(&mut out, &aggregate.output);
                let mut groups = aggregate.group_by.clone();
                groups.sort();
                write_u64(&mut out, groups.len() as u64);
                for group in groups {
                    write_text(&mut out, &group);
                }
            }
        }
        out
    }
}

fn write_term(out: &mut Vec<u8>, term: &Term) {
    match term {
        Term::Variable(name) => {
            out.push(b'v');
            write_text(out, name);
        }
        Term::Constant(value) => {
            out.push(b'c');
            encode_value(out, value);
        }
    }
}

fn write_terms(out: &mut Vec<u8>, terms: &[Term]) {
    write_u64(out, terms.len() as u64);
    for term in terms {
        write_term(out, term);
    }
}

fn atom_bytes(atom: &Atom) -> Vec<u8> {
    let mut out = Vec::new();
    write_text(&mut out, &atom.relation);
    write_terms(&mut out, &atom.terms);
    out
}

fn predicate_bytes(predicate: &Predicate) -> Vec<u8> {
    let mut out = Vec::new();
    match predicate {
        Predicate::Greater(left, right) => {
            out.push(b'g');
            write_text(&mut out, left);
            write_text(&mut out, right);
        }
    }
    out
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct BindingState {
    bindings: BTreeMap<String, FactValue>,
    supports: BTreeSet<SupportRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DerivationLimits {
    max_bindings: usize,
    max_facts: usize,
    max_proofs_per_fact: usize,
    max_total_proofs: usize,
    max_support_nodes: usize,
    max_proof_depth: usize,
    max_capability_alternatives: usize,
    max_canonical_bytes: usize,
    max_rows_scanned: usize,
    max_join_attempts: usize,
    max_intermediate_states: usize,
    max_intermediate_bytes: usize,
    max_proof_combination_attempts: usize,
    max_aggregate_group_entries: usize,
}

impl DerivationLimits {
    fn generous() -> Self {
        Self {
            max_bindings: 4_096,
            max_facts: 4_096,
            max_proofs_per_fact: 256,
            max_total_proofs: 16_384,
            max_support_nodes: 131_072,
            max_proof_depth: 64,
            max_capability_alternatives: 256,
            max_canonical_bytes: 16 * 1024 * 1024,
            max_rows_scanned: 1_000_000,
            max_join_attempts: 1_000_000,
            max_intermediate_states: 131_072,
            max_intermediate_bytes: 64 * 1024 * 1024,
            max_proof_combination_attempts: 131_072,
            max_aggregate_group_entries: 131_072,
        }
    }
}

fn limits(max_bindings: usize) -> DerivationLimits {
    DerivationLimits {
        max_bindings,
        ..DerivationLimits::generous()
    }
}

struct DerivationMeter {
    limits: DerivationLimits,
    facts: BTreeSet<FactKey>,
    proofs: BTreeSet<(FactKey, ProofAlternative)>,
    proofs_per_fact: BTreeMap<FactKey, usize>,
    capability_sets: BTreeMap<FactKey, BTreeSet<BTreeSet<String>>>,
    support_nodes: usize,
    canonical_bytes: usize,
    rows_scanned: usize,
    join_attempts: usize,
    intermediate_states: usize,
    intermediate_bytes: usize,
    proof_combination_attempts: usize,
    aggregate_group_entries: usize,
}

impl DerivationMeter {
    fn new(limits: DerivationLimits) -> Self {
        Self {
            limits,
            facts: BTreeSet::new(),
            proofs: BTreeSet::new(),
            proofs_per_fact: BTreeMap::new(),
            capability_sets: BTreeMap::new(),
            support_nodes: 0,
            canonical_bytes: b"rfc0003.derivation.v1".len() + 8,
            rows_scanned: 0,
            join_attempts: 0,
            intermediate_states: 0,
            intermediate_bytes: 0,
            proof_combination_attempts: 0,
            aggregate_group_entries: 0,
        }
    }

    fn charge_rows_scanned(&mut self) -> OracleResult<()> {
        self.rows_scanned = self
            .rows_scanned
            .checked_add(1)
            .ok_or("derivation.rows_scanned_limit")?;
        if self.rows_scanned > self.limits.max_rows_scanned {
            return Err("derivation.rows_scanned_limit");
        }
        Ok(())
    }

    fn charge_join_attempt(&mut self) -> OracleResult<()> {
        self.join_attempts = self
            .join_attempts
            .checked_add(1)
            .ok_or("derivation.join_attempt_limit")?;
        if self.join_attempts > self.limits.max_join_attempts {
            return Err("derivation.join_attempt_limit");
        }
        Ok(())
    }

    fn charge_intermediate_bytes(&mut self, bytes: usize) -> OracleResult<()> {
        self.intermediate_bytes = self
            .intermediate_bytes
            .checked_add(bytes)
            .ok_or("derivation.intermediate_byte_limit")?;
        if self.intermediate_bytes > self.limits.max_intermediate_bytes {
            return Err("derivation.intermediate_byte_limit");
        }
        Ok(())
    }

    fn charge_intermediate_state(&mut self, bytes: usize) -> OracleResult<()> {
        self.intermediate_states = self
            .intermediate_states
            .checked_add(1)
            .ok_or("derivation.intermediate_state_limit")?;
        if self.intermediate_states > self.limits.max_intermediate_states {
            return Err("derivation.intermediate_state_limit");
        }
        self.charge_intermediate_bytes(bytes)
    }

    fn charge_proof_combination(&mut self, bytes: usize) -> OracleResult<()> {
        self.proof_combination_attempts = self
            .proof_combination_attempts
            .checked_add(1)
            .ok_or("derivation.proof_combination_limit")?;
        if self.proof_combination_attempts > self.limits.max_proof_combination_attempts {
            return Err("derivation.proof_combination_limit");
        }
        self.charge_intermediate_bytes(bytes)
    }

    fn charge_aggregate_group_entry(&mut self, bytes: usize) -> OracleResult<()> {
        self.aggregate_group_entries = self
            .aggregate_group_entries
            .checked_add(1)
            .ok_or("derivation.aggregate_group_limit")?;
        if self.aggregate_group_entries > self.limits.max_aggregate_group_entries {
            return Err("derivation.aggregate_group_limit");
        }
        self.charge_intermediate_bytes(bytes)
    }

    fn retain(
        &mut self,
        result: &mut DerivationResult,
        fact: FactKey,
        proof: ProofAlternative,
    ) -> OracleResult<()> {
        if self.proofs.contains(&(fact.clone(), proof.clone())) {
            return Ok(());
        }
        if !self.facts.contains(&fact) && self.facts.len() >= self.limits.max_facts {
            return Err("derivation.fact_limit");
        }
        let fact_proofs = self.proofs_per_fact.get(&fact).copied().unwrap_or(0);
        if fact_proofs >= self.limits.max_proofs_per_fact {
            return Err("derivation.proofs_per_fact_limit");
        }
        if self.proofs.len() >= self.limits.max_total_proofs {
            return Err("derivation.total_proof_limit");
        }
        let support_nodes = self
            .support_nodes
            .checked_add(proof.supports.len())
            .ok_or("derivation.support_limit")?;
        if support_nodes > self.limits.max_support_nodes {
            return Err("derivation.support_limit");
        }
        if proof.depth > self.limits.max_proof_depth {
            return Err("derivation.depth_limit");
        }
        let mut capability_sets = self.capability_sets.get(&fact).cloned().unwrap_or_default();
        capability_sets.insert(proof.required_capabilities.clone());
        if capability_sets.len() > self.limits.max_capability_alternatives {
            return Err("derivation.capability_alternative_limit");
        }
        let mut additional = 8_usize
            .checked_add(proof.canonical_len()?)
            .ok_or("derivation.canonical_byte_limit")?;
        if !self.facts.contains(&fact) {
            additional = additional
                .checked_add(encoded_fact_key_len(&fact)?)
                .and_then(|bytes| bytes.checked_add(8))
                .ok_or("derivation.canonical_byte_limit")?;
        }
        let encoded = self
            .canonical_bytes
            .checked_add(additional)
            .ok_or("derivation.canonical_byte_limit")?;
        if encoded > self.limits.max_canonical_bytes {
            return Err("derivation.canonical_byte_limit");
        }

        self.facts.insert(fact.clone());
        self.proofs.insert((fact.clone(), proof.clone()));
        self.proofs_per_fact.insert(fact.clone(), fact_proofs + 1);
        self.capability_sets.insert(fact.clone(), capability_sets);
        self.support_nodes = support_nodes;
        self.canonical_bytes = encoded;
        result.entry(fact).or_default().insert(proof);
        Ok(())
    }
}

fn derived_logical_rows(
    relation: &str,
    schemas: &BTreeMap<String, RelationSchema>,
    derived: &DerivationResult,
    meter: &mut DerivationMeter,
) -> OracleResult<Vec<LogicalRow>> {
    let schema = schemas.get(relation).ok_or("relation.unknown")?;
    let mut rows = Vec::new();
    for (key, proofs) in derived.iter().filter(|(key, _)| key.relation == relation) {
        for proof in proofs {
            let bytes = encoded_derived_row_len(key, proof)?;
            meter.charge_intermediate_bytes(bytes)?;
            let support = SupportRef::Derived {
                key: key.clone(),
                proof_id: proof.identity(),
                required_capabilities: proof.required_capabilities.clone(),
                proof_depth: proof.depth,
            };
            rows.push(LogicalRow {
                tuple: key.tuple.clone(),
                support: support.clone(),
            });
            if schema.symmetric && key.tuple[0] != key.tuple[1] {
                meter.charge_intermediate_bytes(bytes)?;
                rows.push(LogicalRow {
                    tuple: vec![key.tuple[1].clone(), key.tuple[0].clone()],
                    support,
                });
            }
        }
    }
    rows.sort();
    Ok(rows)
}

fn unify(
    state: &BindingState,
    terms: &[Term],
    row: &LogicalRow,
    meter: &mut DerivationMeter,
) -> OracleResult<Option<BindingState>> {
    if terms.len() != row.tuple.len() {
        return Ok(None);
    }
    for (index, (term, value)) in terms.iter().zip(&row.tuple).enumerate() {
        match term {
            Term::Constant(expected) if expected != value => return Ok(None),
            Term::Constant(_) => {}
            Term::Variable(name) => match state.bindings.get(name) {
                Some(existing) if existing != value => return Ok(None),
                Some(_) => {}
                None => {
                    for (prior_term, prior_value) in terms[..index].iter().zip(&row.tuple[..index])
                    {
                        if matches!(prior_term, Term::Variable(prior) if prior == name)
                            && prior_value != value
                        {
                            return Ok(None);
                        }
                    }
                }
            },
        }
    }
    let mut bytes = encoded_binding_state_len(state)?;
    for (index, (term, value)) in terms.iter().zip(&row.tuple).enumerate() {
        let Term::Variable(name) = term else {
            continue;
        };
        let first_new_occurrence = !state.bindings.contains_key(name)
            && !terms[..index]
                .iter()
                .any(|prior| matches!(prior, Term::Variable(prior) if prior == name));
        if first_new_occurrence {
            bytes = checked_len_add(bytes, encoded_text_len(name)?)?;
            bytes = checked_len_add(bytes, encoded_value_len(value)?)?;
        }
    }
    if !state.supports.contains(&row.support) {
        bytes = checked_len_add(bytes, encoded_support_len(&row.support)?)?;
    }
    meter.charge_intermediate_state(bytes)?;
    let mut next = state.clone();
    for (term, value) in terms.iter().zip(&row.tuple) {
        if let Term::Variable(name) = term {
            if !next.bindings.contains_key(name) {
                next.bindings.insert(name.clone(), value.clone());
            }
        }
    }
    next.supports.insert(row.support.clone());
    Ok(Some(next))
}

fn predicate_matches(predicate: &Predicate, bindings: &BTreeMap<String, FactValue>) -> bool {
    match predicate {
        Predicate::Greater(left, right) => match (bindings.get(left), bindings.get(right)) {
            (Some(FactValue::Int(left)), Some(FactValue::Int(right))) => left > right,
            _ => false,
        },
    }
}

fn aggregate_values(kind: AggregateKind, values: &[FactValue]) -> OracleResult<Option<FactValue>> {
    if values.is_empty() {
        return Ok(None);
    }
    match kind {
        AggregateKind::Count => Ok(Some(FactValue::Count(
            u64::try_from(values.len()).map_err(|_| "derivation.count_overflow")?,
        ))),
        AggregateKind::Sum => {
            let mut sum = 0_i64;
            for value in values {
                let FactValue::Int(value) = value else {
                    return Err("derivation.aggregate_type");
                };
                sum = sum.checked_add(*value).ok_or("derivation.sum_overflow")?;
            }
            Ok(Some(FactValue::Int(sum)))
        }
        AggregateKind::Min | AggregateKind::Max => {
            let ints = values
                .iter()
                .map(|value| match value {
                    FactValue::Int(value) => Ok(*value),
                    _ => Err("derivation.aggregate_type"),
                })
                .collect::<OracleResult<Vec<_>>>()?;
            let value = if matches!(kind, AggregateKind::Min) {
                ints.into_iter().min()
            } else {
                ints.into_iter().max()
            };
            Ok(value.map(FactValue::Int))
        }
    }
}

fn checked_count(current: u64, additional: u64) -> OracleResult<u64> {
    current
        .checked_add(additional)
        .ok_or("derivation.count_overflow")
}

fn build_head(
    rule: &RulePlan,
    schemas: &BTreeMap<String, RelationSchema>,
    bindings: &BTreeMap<String, FactValue>,
) -> OracleResult<FactKey> {
    let tuple = rule
        .head
        .iter()
        .map(|term| match term {
            Term::Constant(value) => Ok(value.clone()),
            Term::Variable(name) => bindings.get(name).cloned().ok_or("derivation.unbound_head"),
        })
        .collect::<OracleResult<Vec<_>>>()?;
    let schema = schemas.get(&rule.head_relation).ok_or("relation.unknown")?;
    Ok(FactKey::new(
        &rule.head_relation,
        schema.canonical_tuple(tuple)?,
    ))
}

fn encoded_head_len(
    rule: &RulePlan,
    bindings: &BTreeMap<String, FactValue>,
) -> OracleResult<usize> {
    let mut length = encoded_text_len(&rule.head_relation)?;
    length = checked_len_add(length, 8)?;
    for term in &rule.head {
        let value = match term {
            Term::Constant(value) => value,
            Term::Variable(name) => bindings.get(name).ok_or("derivation.unbound_head")?,
        };
        length = checked_len_add(length, encoded_value_len(value)?)?;
    }
    Ok(length)
}

fn evaluate_rule(
    rule: &RulePlan,
    store: &RelationStore,
    schemas: &BTreeMap<String, RelationSchema>,
    derived: &DerivationResult,
    meter: &mut DerivationMeter,
) -> OracleResult<DerivationResult> {
    rule.validate(store, schemas)?;
    meter.charge_intermediate_state(16)?;
    let mut states = BTreeSet::from([BindingState {
        bindings: BTreeMap::new(),
        supports: BTreeSet::new(),
    }]);
    let mut atoms = rule.atoms.iter().collect::<Vec<_>>();
    atoms.sort_by_key(|atom| atom_bytes(atom));
    for atom in atoms {
        let rows = if store.schemas.contains_key(&atom.relation) {
            store.logical_rows_metered(&atom.relation, Some(meter))?
        } else {
            derived_logical_rows(&atom.relation, schemas, derived, meter)?
        };
        let mut next = BTreeSet::new();
        for state in &states {
            for row in &rows {
                meter.charge_rows_scanned()?;
                meter.charge_join_attempt()?;
                if let Some(joined) = unify(state, &atom.terms, row, meter)? {
                    if !next.contains(&joined) && next.len() >= meter.limits.max_bindings {
                        return Err("derivation.binding_limit");
                    }
                    next.insert(joined);
                }
            }
        }
        states = next;
    }
    let mut predicates = rule.predicates.iter().collect::<Vec<_>>();
    predicates.sort_by_key(|predicate| predicate_bytes(predicate));
    states.retain(|state| {
        predicates
            .iter()
            .all(|predicate| predicate_matches(predicate, &state.bindings))
    });

    let mut result = DerivationResult::new();
    if let Some(aggregate) = &rule.aggregate {
        let mut group_names = aggregate.group_by.clone();
        group_names.sort();
        let mut groups = BTreeMap::<
            Vec<FactValue>,
            BTreeMap<BTreeMap<String, FactValue>, Vec<BindingState>>,
        >::new();
        for state in states {
            let mut group_bytes = 8;
            for name in &group_names {
                let value = state.bindings.get(name).ok_or("derivation.unbound_group")?;
                group_bytes = checked_len_add(group_bytes, encoded_value_len(value)?)?;
            }
            let entry_bytes = checked_len_add(group_bytes, encoded_binding_state_len(&state)?)?;
            meter.charge_aggregate_group_entry(entry_bytes)?;
            let group = group_names
                .iter()
                .map(|name| state.bindings.get(name).cloned().unwrap())
                .collect::<Vec<_>>();
            groups
                .entry(group)
                .or_default()
                .entry(state.bindings.clone())
                .or_default()
                .push(state);
        }
        for (group, logical_bindings) in groups {
            let values = match &aggregate.input {
                Some(input) => {
                    let mut bytes = 8;
                    for bindings in logical_bindings.keys() {
                        let value = bindings.get(input).ok_or("derivation.unbound_aggregate")?;
                        bytes = checked_len_add(bytes, encoded_value_len(value)?)?;
                    }
                    meter.charge_intermediate_bytes(bytes)?;
                    logical_bindings
                        .keys()
                        .map(|bindings| bindings.get(input).cloned().unwrap())
                        .collect()
                }
                None => {
                    let bytes = checked_len_add(
                        8,
                        logical_bindings
                            .len()
                            .checked_mul(encoded_value_len(&FactValue::Count(1))?)
                            .ok_or("derivation.intermediate_byte_limit")?,
                    )?;
                    meter.charge_intermediate_bytes(bytes)?;
                    vec![FactValue::Count(1); logical_bindings.len()]
                }
            };
            let Some(value) = aggregate_values(aggregate.kind, &values)? else {
                continue;
            };
            let mut binding_bytes = 8;
            for (name, group_value) in group_names.iter().zip(&group) {
                binding_bytes = checked_len_add(binding_bytes, encoded_text_len(name)?)?;
                binding_bytes = checked_len_add(binding_bytes, encoded_value_len(group_value)?)?;
            }
            binding_bytes = checked_len_add(binding_bytes, encoded_text_len(&aggregate.output)?)?;
            binding_bytes = checked_len_add(binding_bytes, encoded_value_len(&value)?)?;
            meter.charge_intermediate_bytes(binding_bytes)?;
            let mut bindings = group_names
                .iter()
                .cloned()
                .zip(group.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            bindings.insert(aggregate.output.clone(), value);
            meter.charge_intermediate_bytes(encoded_head_len(rule, &bindings)?)?;
            let key = build_head(rule, schemas, &bindings)?;

            meter.charge_intermediate_bytes(8)?;
            let mut support_combinations = BTreeSet::from([BTreeSet::new()]);
            for alternatives in logical_bindings.values() {
                let mut next = BTreeSet::new();
                let mut capability_sets = BTreeSet::new();
                for accumulated in &support_combinations {
                    for alternative in alternatives {
                        let attempt_bytes = checked_len_add(
                            encoded_support_set_len(accumulated)?,
                            encoded_support_set_len(&alternative.supports)?,
                        )?;
                        meter.charge_proof_combination(attempt_bytes)?;
                        let supports = accumulated
                            .union(&alternative.supports)
                            .cloned()
                            .collect::<BTreeSet<_>>();
                        if !next.contains(&supports) {
                            if next.len() >= meter.limits.max_proofs_per_fact {
                                return Err("derivation.proofs_per_fact_limit");
                            }
                            let required_capabilities = supports
                                .iter()
                                .flat_map(|support| support.required_capabilities().iter().cloned())
                                .collect::<BTreeSet<_>>();
                            capability_sets.insert(required_capabilities);
                            if capability_sets.len() > meter.limits.max_capability_alternatives {
                                return Err("derivation.capability_alternative_limit");
                            }
                        }
                        next.insert(supports);
                    }
                }
                support_combinations = next;
            }
            for supports in support_combinations {
                meter.charge_intermediate_bytes(encoded_combined_capabilities_len(&supports)?)?;
                let required_capabilities = supports
                    .iter()
                    .flat_map(|support| support.required_capabilities().iter().cloned())
                    .collect();
                let depth = 1 + supports.iter().map(SupportRef::depth).max().unwrap_or(0);
                meter.retain(
                    &mut result,
                    key.clone(),
                    ProofAlternative {
                        rule: rule.id.clone(),
                        bindings: bindings.clone(),
                        supports,
                        aggregate_group: Some(group.clone()),
                        required_capabilities,
                        depth,
                    },
                )?;
            }
        }
    } else {
        for state in states {
            meter.charge_intermediate_bytes(encoded_head_len(rule, &state.bindings)?)?;
            let key = build_head(rule, schemas, &state.bindings)?;
            meter.charge_intermediate_bytes(encoded_combined_capabilities_len(&state.supports)?)?;
            let required_capabilities = state
                .supports
                .iter()
                .flat_map(|support| support.required_capabilities().iter().cloned())
                .collect();
            let depth = 1 + state
                .supports
                .iter()
                .map(SupportRef::depth)
                .max()
                .unwrap_or(0);
            meter.retain(
                &mut result,
                key,
                ProofAlternative {
                    rule: rule.id.clone(),
                    bindings: state.bindings,
                    supports: state.supports,
                    aggregate_group: None,
                    required_capabilities,
                    depth,
                },
            )?;
        }
    }
    Ok(result)
}

fn derive_all(
    store: &RelationStore,
    schemas: &BTreeMap<String, RelationSchema>,
    rules: &[RulePlan],
    limits: DerivationLimits,
) -> OracleResult<DerivationResult> {
    for schema in schemas.values() {
        schema.validate_declaration()?;
        if store.schemas.contains_key(&schema.name) {
            return Err("derivation.relation_namespace_collision");
        }
    }
    let mut rule_ids = BTreeSet::new();
    for rule in rules {
        if rule.id.is_empty() {
            return Err("derivation.empty_rule_id");
        }
        if !rule.id.contains('.') && !rule.id.contains("::") {
            return Err("derivation.unqualified_rule_id");
        }
        if !rule_ids.insert(&rule.id) {
            return Err("derivation.duplicate_rule_id");
        }
    }
    let mut canonical_rules = rules.iter().collect::<Vec<_>>();
    canonical_rules.sort_by(|left, right| {
        (
            &left.head_relation,
            &left.id,
            Sha256::digest(left.canonical_bytes()),
        )
            .cmp(&(
                &right.head_relation,
                &right.id,
                Sha256::digest(right.canonical_bytes()),
            ))
    });
    for rule in &canonical_rules {
        rule.validate(store, schemas)?;
    }
    let mut meter = DerivationMeter::new(limits);
    let heads = canonical_rules
        .iter()
        .map(|rule| rule.head_relation.clone())
        .collect::<BTreeSet<_>>();
    let mut pending = heads.clone();
    let mut completed = BTreeSet::new();
    let mut result = DerivationResult::new();
    while !pending.is_empty() {
        let ready = pending.iter().find(|head| {
            canonical_rules
                .iter()
                .filter(|rule| rule.head_relation.as_str() == head.as_str())
                .flat_map(|rule| &rule.atoms)
                .filter(|atom| heads.contains(&atom.relation))
                .all(|atom| completed.contains(&atom.relation))
        });
        let Some(head) = ready.cloned() else {
            return Err("derivation.cycle");
        };
        for rule in canonical_rules
            .iter()
            .filter(|rule| rule.head_relation == head)
        {
            let produced = evaluate_rule(rule, store, schemas, &result, &mut meter)?;
            for (fact, proofs) in produced {
                result.entry(fact).or_default().extend(proofs);
            }
        }
        pending.remove(&head);
        completed.insert(head);
    }
    Ok(result)
}

#[derive(Clone)]
struct DependencyMaintainer {
    model: WorldModel,
    derived_schemas: BTreeMap<String, RelationSchema>,
    rules: Vec<RulePlan>,
    derived: DerivationResult,
    limits: DerivationLimits,
}

impl DependencyMaintainer {
    fn new(
        model: WorldModel,
        derived_schemas: BTreeMap<String, RelationSchema>,
        rules: Vec<RulePlan>,
        limits: DerivationLimits,
    ) -> OracleResult<Self> {
        let derived = derive_all(&model.relations, &derived_schemas, &rules, limits)?;
        Ok(Self {
            model,
            derived_schemas,
            rules,
            derived,
            limits,
        })
    }

    fn apply(&mut self, transaction: Transaction) -> OracleResult<()> {
        let mut candidate_model = self.model.clone();
        let before = self
            .model
            .relations
            .assertions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        candidate_model.apply_transaction(transaction)?;
        let after = candidate_model
            .relations
            .assertions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let changed_inputs = before
            .symmetric_difference(&after)
            .map(|fact| fact.relation.clone())
            .collect::<BTreeSet<_>>();
        let mut affected = BTreeSet::new();
        loop {
            let previous = affected.len();
            for rule in &self.rules {
                if rule.atoms.iter().any(|atom| {
                    changed_inputs.contains(&atom.relation) || affected.contains(&atom.relation)
                }) {
                    affected.insert(rule.head_relation.clone());
                }
            }
            if previous == affected.len() {
                break;
            }
        }
        let full = derive_all(
            &candidate_model.relations,
            &self.derived_schemas,
            &self.rules,
            self.limits,
        )?;
        let mut incremental = self.derived.clone();
        incremental.retain(|fact, _| !affected.contains(&fact.relation));
        for (fact, proofs) in full
            .iter()
            .filter(|(fact, _)| affected.contains(&fact.relation))
        {
            incremental.insert(fact.clone(), proofs.clone());
        }
        if incremental != full {
            return Err("derivation.incremental_mismatch");
        }
        self.model = candidate_model;
        self.derived = incremental;
        Ok(())
    }
}

fn checked_len_add(left: usize, right: usize) -> OracleResult<usize> {
    left.checked_add(right)
        .ok_or("derivation.canonical_byte_limit")
}

fn encoded_text_len(value: &str) -> OracleResult<usize> {
    checked_len_add(8, value.len())
}

fn encoded_value_len(value: &FactValue) -> OracleResult<usize> {
    match value {
        FactValue::Int(_) | FactValue::Count(_) | FactValue::Entity(_) => Ok(9),
        FactValue::Text(value) => checked_len_add(1, encoded_text_len(value)?),
    }
}

fn encoded_fact_key_len(key: &FactKey) -> OracleResult<usize> {
    let mut length = checked_len_add(encoded_text_len(&key.relation)?, 8)?;
    for value in &key.tuple {
        length = checked_len_add(length, encoded_value_len(value)?)?;
    }
    Ok(length)
}

fn encoded_tuple_len(tuple: &[FactValue]) -> OracleResult<usize> {
    let mut length = 8;
    for value in tuple {
        length = checked_len_add(length, encoded_value_len(value)?)?;
    }
    Ok(length)
}

fn encoded_capability_set_len(capabilities: &BTreeSet<String>) -> OracleResult<usize> {
    let mut length = 8;
    for capability in capabilities {
        length = checked_len_add(length, encoded_text_len(capability)?)?;
    }
    Ok(length)
}

fn encoded_support_len(support: &SupportRef) -> OracleResult<usize> {
    let mut length = encoded_fact_key_len(support.key())?;
    length = checked_len_add(length, 1)?;
    length = checked_len_add(
        length,
        match support {
            SupportRef::Authoritative { .. } => 8,
            SupportRef::Derived { proof_id, .. } => encoded_text_len(proof_id)?,
        },
    )?;
    length = checked_len_add(
        length,
        encoded_capability_set_len(support.required_capabilities())?,
    )?;
    if matches!(support, SupportRef::Derived { .. }) {
        length = checked_len_add(length, 8)?;
    }
    Ok(length)
}

fn encoded_support_set_len(supports: &BTreeSet<SupportRef>) -> OracleResult<usize> {
    let mut length = 8;
    for support in supports {
        length = checked_len_add(length, encoded_support_len(support)?)?;
    }
    Ok(length)
}

fn encoded_combined_capabilities_len(supports: &BTreeSet<SupportRef>) -> OracleResult<usize> {
    let mut length = 8;
    for support in supports {
        for capability in support.required_capabilities() {
            // Duplicates deliberately overcharge: this is a conservative
            // pre-allocation quote for constructing the deduplicated set.
            length = checked_len_add(length, encoded_text_len(capability)?)?;
        }
    }
    Ok(length)
}

fn encoded_binding_map_len(bindings: &BTreeMap<String, FactValue>) -> OracleResult<usize> {
    let mut length = 8;
    for (name, value) in bindings {
        length = checked_len_add(length, encoded_text_len(name)?)?;
        length = checked_len_add(length, encoded_value_len(value)?)?;
    }
    Ok(length)
}

fn encoded_binding_state_len(state: &BindingState) -> OracleResult<usize> {
    checked_len_add(
        encoded_binding_map_len(&state.bindings)?,
        encoded_support_set_len(&state.supports)?,
    )
}

fn encoded_authoritative_row_len(assertion: &FactAssertion) -> OracleResult<usize> {
    let mut length = encoded_fact_key_len(&assertion.key)?;
    length = checked_len_add(length, encoded_tuple_len(&assertion.key.tuple)?)?;
    length = checked_len_add(length, 8)?;
    checked_len_add(
        length,
        encoded_capability_set_len(&assertion.required_capabilities)?,
    )
}

fn encoded_derived_row_len(key: &FactKey, proof: &ProofAlternative) -> OracleResult<usize> {
    let mut length = encoded_fact_key_len(key)?;
    length = checked_len_add(length, encoded_tuple_len(&key.tuple)?)?;
    length = checked_len_add(length, proof.canonical_len()?)?;
    // SHA-256 proof IDs are rendered as 64 lowercase hexadecimal bytes.
    length = checked_len_add(length, 8 + 64)?;
    length = checked_len_add(
        length,
        encoded_capability_set_len(&proof.required_capabilities)?,
    )?;
    checked_len_add(length, 8)
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_text(out: &mut Vec<u8>, value: &str) {
    write_u64(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn encode_value(out: &mut Vec<u8>, value: &FactValue) {
    match value {
        FactValue::Int(value) => {
            out.push(b'i');
            out.extend_from_slice(&value.to_be_bytes());
        }
        FactValue::Count(value) => {
            out.push(b'c');
            write_u64(out, *value);
        }
        FactValue::Entity(entity) => {
            out.push(b'e');
            out.extend_from_slice(&entity.slot.to_be_bytes());
            out.extend_from_slice(&entity.generation.to_be_bytes());
        }
        FactValue::Text(value) => {
            out.push(b't');
            write_text(out, value);
        }
    }
}

fn encode_fact_key(out: &mut Vec<u8>, key: &FactKey) {
    write_text(out, &key.relation);
    write_u64(out, key.tuple.len() as u64);
    for value in &key.tuple {
        encode_value(out, value);
    }
}

fn semantic_relation_bytes(store: &RelationStore) -> Vec<u8> {
    let mut out = b"rfc0003.semantic.v1".to_vec();
    write_u64(&mut out, store.assertions.len() as u64);
    for key in store.assertions.keys() {
        encode_fact_key(&mut out, key);
    }
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OperationalRelationState {
    entity_allocator: EntityTable,
    assertion_allocator_next: u64,
    components: BTreeMap<(EntityRef, String), FactValue>,
    assertions: BTreeMap<FactKey, FactAssertion>,
}

impl OperationalRelationState {
    fn capture(model: &WorldModel) -> Self {
        Self {
            entity_allocator: model.entities.clone(),
            assertion_allocator_next: model.relations.next_assertion_id,
            components: model.components.clone(),
            assertions: model.relations.assertions.clone(),
        }
    }

    fn restore(&self, schemas: BTreeMap<String, RelationSchema>) -> WorldModel {
        WorldModel {
            entities: self.entity_allocator.clone(),
            components: self.components.clone(),
            relations: RelationStore {
                schemas,
                assertions: self.assertions.clone(),
                next_assertion_id: self.assertion_allocator_next,
                last_changes: Vec::new(),
            },
        }
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = b"rfc0003.operational.v2".to_vec();
        write_u64(&mut out, self.entity_allocator.next_slot as u64);
        write_u64(&mut out, self.entity_allocator.generations.len() as u64);
        for (slot, generation) in &self.entity_allocator.generations {
            out.extend_from_slice(&slot.to_be_bytes());
            out.extend_from_slice(&generation.to_be_bytes());
        }
        write_u64(&mut out, self.entity_allocator.live.len() as u64);
        for entity in &self.entity_allocator.live {
            out.extend_from_slice(&entity.slot.to_be_bytes());
            out.extend_from_slice(&entity.generation.to_be_bytes());
        }
        write_u64(&mut out, self.entity_allocator.free_slots.len() as u64);
        for slot in &self.entity_allocator.free_slots {
            out.extend_from_slice(&slot.to_be_bytes());
        }
        write_u64(&mut out, self.assertion_allocator_next);
        write_u64(&mut out, self.components.len() as u64);
        for ((entity, component), value) in &self.components {
            out.extend_from_slice(&entity.slot.to_be_bytes());
            out.extend_from_slice(&entity.generation.to_be_bytes());
            write_text(&mut out, component);
            encode_value(&mut out, value);
        }
        write_u64(&mut out, self.assertions.len() as u64);
        for assertion in self.assertions.values() {
            encode_fact_key(&mut out, &assertion.key);
            write_u64(&mut out, assertion.id);
            write_u64(&mut out, assertion.causes.len() as u64);
            for cause in &assertion.causes {
                write_text(&mut out, cause);
            }
            write_u64(&mut out, assertion.required_capabilities.len() as u64);
            for capability in &assertion.required_capabilities {
                write_text(&mut out, capability);
            }
        }
        out
    }
}

fn operational_checkpoint_bytes(model: &WorldModel) -> Vec<u8> {
    OperationalRelationState::capture(model).canonical_bytes()
}

fn canonical_derivation_bytes(derived: &DerivationResult) -> Vec<u8> {
    let mut out = b"rfc0003.derivation.v1".to_vec();
    write_u64(&mut out, derived.len() as u64);
    for (fact, proofs) in derived {
        encode_fact_key(&mut out, fact);
        write_u64(&mut out, proofs.len() as u64);
        for proof in proofs {
            let proof = proof.canonical_bytes();
            write_u64(&mut out, proof.len() as u64);
            out.extend_from_slice(&proof);
        }
    }
    out
}

fn derivation_checkpoint_bytes(derived: &DerivationResult) -> Vec<u8> {
    canonical_derivation_bytes(derived)
}

fn portable_checkpoint_bytes(model: &WorldModel, derived: &DerivationResult) -> Vec<u8> {
    let mut out = b"rfc0003.portable.v1".to_vec();
    let world = operational_checkpoint_bytes(model);
    write_u64(&mut out, world.len() as u64);
    out.extend_from_slice(&world);
    let proofs = derivation_checkpoint_bytes(derived);
    write_u64(&mut out, proofs.len() as u64);
    out.extend_from_slice(&proofs);
    out
}

fn read_u64(input: &mut &[u8]) -> OracleResult<u64> {
    let bytes = input.get(..8).ok_or("wire.truncated")?;
    *input = &input[8..];
    Ok(u64::from_be_bytes(bytes.try_into().expect("eight bytes")))
}

fn read_text(input: &mut &[u8]) -> OracleResult<String> {
    let len = usize::try_from(read_u64(input)?).map_err(|_| "wire.length")?;
    let bytes = input.get(..len).ok_or("wire.truncated")?;
    *input = &input[len..];
    String::from_utf8(bytes.to_vec()).map_err(|_| "wire.utf8")
}

fn decode_value(input: &mut &[u8]) -> OracleResult<FactValue> {
    let tag = *input.first().ok_or("wire.truncated")?;
    *input = &input[1..];
    match tag {
        b'i' => {
            let bytes = input.get(..8).ok_or("wire.truncated")?;
            *input = &input[8..];
            Ok(FactValue::Int(i64::from_be_bytes(
                bytes.try_into().expect("eight bytes"),
            )))
        }
        b'c' => Ok(FactValue::Count(read_u64(input)?)),
        b'e' => {
            let slot = input.get(..4).ok_or("wire.truncated")?;
            let generation = input.get(4..8).ok_or("wire.truncated")?;
            *input = &input[8..];
            Ok(FactValue::Entity(EntityRef {
                slot: u32::from_be_bytes(slot.try_into().expect("four bytes")),
                generation: u32::from_be_bytes(generation.try_into().expect("four bytes")),
            }))
        }
        b't' => Ok(FactValue::Text(read_text(input)?)),
        _ => Err("wire.tag"),
    }
}

fn fact_key_bytes(key: &FactKey) -> Vec<u8> {
    let mut out = Vec::new();
    encode_fact_key(&mut out, key);
    out
}

fn decode_fact_key_from(input: &mut &[u8]) -> OracleResult<FactKey> {
    let relation = read_text(input)?;
    let count = usize::try_from(read_u64(input)?).map_err(|_| "wire.length")?;
    let tuple = (0..count)
        .map(|_| decode_value(input))
        .collect::<OracleResult<Vec<_>>>()?;
    Ok(FactKey { relation, tuple })
}

fn decode_fact_key(mut input: &[u8]) -> OracleResult<FactKey> {
    let key = decode_fact_key_from(&mut input)?;
    if !input.is_empty() {
        return Err("wire.trailing");
    }
    Ok(key)
}

fn decode_semantic_relation_bytes(
    mut input: &[u8],
    schemas: &BTreeMap<String, RelationSchema>,
    entities: &EntityTable,
) -> OracleResult<BTreeSet<FactKey>> {
    const DOMAIN: &[u8] = b"rfc0003.semantic.v1";
    if input.get(..DOMAIN.len()) != Some(DOMAIN) {
        return Err("wire.domain");
    }
    input = &input[DOMAIN.len()..];
    let count = usize::try_from(read_u64(&mut input)?).map_err(|_| "wire.length")?;
    let mut facts = BTreeSet::new();
    let mut previous = None;
    for _ in 0..count {
        let fact = decode_fact_key_from(&mut input)?;
        let schema = schemas.get(&fact.relation).ok_or("wire.unknown_relation")?;
        if fact.tuple.len() != schema.columns.len() {
            return Err("wire.relation_arity");
        }
        if fact
            .tuple
            .iter()
            .zip(&schema.columns)
            .any(|(value, column)| value.kind() != column.kind)
        {
            return Err("wire.relation_type");
        }
        if schema.canonical_tuple(fact.tuple.clone())? != fact.tuple {
            return Err("wire.noncanonical_tuple");
        }
        if fact
            .tuple
            .iter()
            .any(|value| matches!(value, FactValue::Entity(entity) if !entities.contains(*entity)))
        {
            return Err("wire.entity_not_live");
        }
        if previous.as_ref().is_some_and(|previous| previous >= &fact) {
            return Err("wire.noncanonical_fact_order");
        }
        previous = Some(fact.clone());
        facts.insert(fact);
    }
    if !input.is_empty() {
        return Err("wire.trailing");
    }
    Ok(facts)
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

fn render_visible(derived: &DerivationResult, capabilities: &BTreeSet<String>) -> Vec<String> {
    let mut rendered = Vec::new();
    for (fact, proofs) in derived {
        let visible = proofs
            .iter()
            .filter(|proof| proof.required_capabilities.is_subset(capabilities))
            .collect::<Vec<_>>();
        if visible.is_empty() {
            continue;
        }
        let mut bytes = fact_key_bytes(fact);
        for proof in visible {
            write_text(&mut bytes, &proof.identity());
        }
        rendered.push(hex(&bytes));
    }
    rendered
}

struct PortableAttempt {
    checkpoint: Vec<u8>,
}

fn replay_portable(
    model: &WorldModel,
    derived: &DerivationResult,
    attempt: &PortableAttempt,
    instructions_executed: &mut usize,
) -> OracleResult<()> {
    if portable_checkpoint_bytes(model, derived) != attempt.checkpoint {
        return Err("attempt.checkpoint_mismatch");
    }
    *instructions_executed += 1;
    Ok(())
}

fn entity_column(name: &str) -> ColumnSchema {
    ColumnSchema::new(name, ValueKind::Entity)
}

fn int_column(name: &str) -> ColumnSchema {
    ColumnSchema::new(name, ValueKind::Int)
}

fn count_column(name: &str) -> ColumnSchema {
    ColumnSchema::new(name, ValueKind::Count)
}

fn register_authoritative_schemas(model: &mut WorldModel) {
    model
        .relations
        .register(
            RelationSchema::new(
                "Owns",
                vec![entity_column("owner"), entity_column("item").cascade()],
            )
            .unique("item", &[1]),
        )
        .unwrap();
    model
        .relations
        .register(
            RelationSchema::new(
                "ItemWeight",
                vec![entity_column("item").cascade(), int_column("weight")],
            )
            .unique("item", &[0]),
        )
        .unwrap();
    model
        .relations
        .register(
            RelationSchema::new(
                "CarryCapacity",
                vec![entity_column("person").cascade(), int_column("capacity")],
            )
            .unique("person", &[0]),
        )
        .unwrap();
}

fn derived_schemas() -> BTreeMap<String, RelationSchema> {
    [
        RelationSchema::new(
            "TotalWeight",
            vec![entity_column("person"), int_column("total")],
        ),
        RelationSchema::new("Encumbered", vec![entity_column("person")]),
        RelationSchema::new("HasAlly", vec![entity_column("person")]),
        RelationSchema::new(
            "CountAllies",
            vec![entity_column("person"), count_column("count")],
        ),
        RelationSchema::new("Marked", vec![int_column("value")]),
    ]
    .into_iter()
    .map(|schema| (schema.name.clone(), schema))
    .collect()
}

fn ownership_rules() -> Vec<RulePlan> {
    vec![
        RulePlan {
            id: "derive.TotalWeight".to_owned(),
            head_relation: "TotalWeight".to_owned(),
            head: vec![Term::var("person"), Term::var("total")],
            atoms: vec![
                Atom::new("Owns", vec![Term::var("person"), Term::var("item")]),
                Atom::new("ItemWeight", vec![Term::var("item"), Term::var("weight")]),
            ],
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind: AggregateKind::Sum,
                input: Some("weight".to_owned()),
                output: "total".to_owned(),
                group_by: vec!["person".to_owned()],
            }),
        },
        RulePlan {
            id: "derive.Encumbered".to_owned(),
            head_relation: "Encumbered".to_owned(),
            head: vec![Term::var("person")],
            atoms: vec![
                Atom::new("TotalWeight", vec![Term::var("person"), Term::var("total")]),
                Atom::new(
                    "CarryCapacity",
                    vec![Term::var("person"), Term::var("capacity")],
                ),
            ],
            predicates: vec![Predicate::Greater(
                "total".to_owned(),
                "capacity".to_owned(),
            )],
            aggregate: None,
        },
    ]
}

fn seed_ownership_model() -> (WorldModel, EntityRef, EntityRef, EntityRef) {
    let mut model = WorldModel::default();
    register_authoritative_schemas(&mut model);
    let person = model.entities.spawn();
    let item_a = model.entities.spawn();
    let item_b = model.entities.spawn();
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(
                    pending_key("Owns", vec![existing(person), existing(item_a)]),
                    "settlement.a",
                ),
                insert(
                    pending_key("Owns", vec![existing(person), existing(item_b)]),
                    "settlement.a",
                ),
                insert(
                    pending_key("ItemWeight", vec![existing(item_a), int(7)]),
                    "settlement.weights",
                ),
                insert(
                    pending_key("ItemWeight", vec![existing(item_b), int(6)]),
                    "settlement.weights",
                ),
                insert(
                    pending_key("CarryCapacity", vec![existing(person), int(10)]),
                    "settlement.capacity",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    (model, person, item_a, item_b)
}

#[test]
fn entity_foreign_keys_restrict_cascade_and_never_retarget_reused_slots() {
    let mut model = WorldModel::default();
    model
        .relations
        .register(RelationSchema::new(
            "Restricts",
            vec![entity_column("owner"), entity_column("target")],
        ))
        .unwrap();
    model
        .relations
        .register(RelationSchema::new(
            "Cascades",
            vec![entity_column("owner"), entity_column("target").cascade()],
        ))
        .unwrap();
    let owner = model.entities.spawn();
    let target = model.entities.spawn();
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Restricts", vec![existing(owner), existing(target)]),
                "settlement.restrict",
            )],
            ..Transaction::default()
        })
        .unwrap();
    let before = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            despawns: vec![despawn(target, "settlement.restricted_despawn")],
            ..Transaction::default()
        }),
        Err("entity.delete_restricted")
    );
    assert_eq!(model, before);

    model
        .apply_transaction(Transaction {
            operations: vec![
                remove(
                    pending_key("Restricts", vec![existing(owner), existing(target)]),
                    "settlement.remove_restrict",
                ),
                insert(
                    pending_key("Cascades", vec![existing(owner), existing(target)]),
                    "settlement.cascade_source",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    model
        .apply_transaction(Transaction {
            despawns: vec![despawn(target, "settlement.cascade_despawn")],
            ..Transaction::default()
        })
        .unwrap();
    assert!(model
        .relations
        .assertions
        .keys()
        .all(|key| { !key.tuple.contains(&FactValue::Entity(target)) }));
    assert!(model
        .relations
        .last_changes
        .iter()
        .any(|change| change.kind == ChangeKind::Cascade));
    let replacement = model.entities.spawn();
    assert_eq!(replacement.slot, target.slot);
    assert_ne!(replacement.generation, target.generation);

    let handles = model
        .apply_transaction(Transaction {
            spawn_handles: BTreeSet::from([9, 3]),
            operations: vec![
                insert(
                    pending_key("Cascades", vec![existing(owner), candidate(9)]),
                    "settlement.same_candidate_spawn.9",
                ),
                insert(
                    pending_key("Cascades", vec![existing(owner), candidate(3)]),
                    "settlement.same_candidate_spawn.3",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    assert!(model.entities.contains(handles[&9]));
    assert!(model.entities.contains(handles[&3]));
    assert!(handles[&3].slot < handles[&9].slot);
}

#[test]
fn symmetric_storage_has_two_logical_orientations_and_one_self_orientation() {
    let mut model = WorldModel::default();
    let a = model.entities.spawn();
    let b = model.entities.spawn();
    model
        .relations
        .register(
            RelationSchema::new(
                "AlliedWith",
                vec![entity_column("left"), entity_column("right")],
            )
            .symmetric(),
        )
        .unwrap();
    assert_eq!(
        RelationSchema::new(
            "InvalidPartner",
            vec![entity_column("left"), entity_column("right")],
        )
        .symmetric()
        .unique("left", &[0])
        .validate_declaration(),
        Err("relation.symmetric_unique_forbidden")
    );
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(
                    pending_key("AlliedWith", vec![existing(b), existing(a)]),
                    "law.b",
                ),
                insert(
                    pending_key("AlliedWith", vec![existing(a), existing(b)]),
                    "law.a",
                ),
                insert(
                    pending_key("AlliedWith", vec![existing(a), existing(a)]),
                    "law.self",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(model.relations.assertions.len(), 2);
    assert!(model
        .relations
        .contains_ground(FactKey::new(
            "AlliedWith",
            vec![FactValue::Entity(b), FactValue::Entity(a)],
        ))
        .unwrap());
    let rows = model.relations.logical_rows("AlliedWith").unwrap();
    assert_eq!(rows.len(), 3);

    let schemas = derived_schemas();
    let rules = vec![
        RulePlan {
            id: "derive.HasAlly".to_owned(),
            head_relation: "HasAlly".to_owned(),
            head: vec![Term::var("person")],
            atoms: vec![Atom::new(
                "AlliedWith",
                vec![Term::var("person"), Term::var("other")],
            )],
            predicates: Vec::new(),
            aggregate: None,
        },
        RulePlan {
            id: "derive.CountAllies".to_owned(),
            head_relation: "CountAllies".to_owned(),
            head: vec![Term::var("person"), Term::var("count")],
            atoms: vec![Atom::new(
                "AlliedWith",
                vec![Term::var("person"), Term::var("other")],
            )],
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind: AggregateKind::Count,
                input: None,
                output: "count".to_owned(),
                group_by: vec!["person".to_owned()],
            }),
        },
    ];
    let derived = derive_all(&model.relations, &schemas, &rules, limits(64)).unwrap();
    assert!(derived.contains_key(&FactKey::new("HasAlly", vec![FactValue::Entity(a)],)));
    assert!(derived.contains_key(&FactKey::new("HasAlly", vec![FactValue::Entity(b)],)));
    assert!(derived.contains_key(&FactKey::new(
        "CountAllies",
        vec![FactValue::Entity(a), FactValue::Count(2)],
    )));
    assert!(derived.contains_key(&FactKey::new(
        "CountAllies",
        vec![FactValue::Entity(b), FactValue::Count(1)],
    )));
}

#[test]
fn patch_algebra_is_base_relative_named_and_order_independent() {
    let (mut model, owner, item, _) = seed_ownership_model();
    let original = model
        .relations
        .assertions
        .get(&FactKey::new(
            "Owns",
            vec![FactValue::Entity(owner), FactValue::Entity(item)],
        ))
        .unwrap()
        .clone();
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "ignored.duplicate",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(model.relations.assertions[&original.key], original);

    let absent = pending_key("Owns", vec![existing(item), existing(owner)]);
    model
        .apply_transaction(Transaction {
            operations: vec![remove(absent, "ignored.absent")],
            ..Transaction::default()
        })
        .unwrap();

    let conflicting = pending_key("Owns", vec![existing(owner), existing(item)]);
    let before = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![
                insert(conflicting.clone(), "law.insert"),
                remove(conflicting, "law.remove"),
            ],
            ..Transaction::default()
        }),
        Err("relation.operation_conflict")
    );
    assert_eq!(model, before);

    let new_owner = model.entities.spawn();
    model
        .apply_transaction(Transaction {
            operations: vec![
                PendingOperation::ReplaceBy {
                    relation: "Owns".to_owned(),
                    unique_constraint: "item".to_owned(),
                    selected_key: vec![existing(item)],
                    tuple: vec![existing(new_owner), existing(item)],
                    metadata: OperationMetadata::cause("law.transfer.b"),
                },
                PendingOperation::ReplaceBy {
                    relation: "Owns".to_owned(),
                    unique_constraint: "item".to_owned(),
                    selected_key: vec![existing(item)],
                    tuple: vec![existing(new_owner), existing(item)],
                    metadata: OperationMetadata::cause("law.transfer.a"),
                },
            ],
            ..Transaction::default()
        })
        .unwrap();
    let transferred = &model.relations.assertions[&FactKey::new(
        "Owns",
        vec![FactValue::Entity(new_owner), FactValue::Entity(item)],
    )];
    assert_eq!(
        transferred.causes,
        BTreeSet::from(["law.transfer.a".to_owned(), "law.transfer.b".to_owned()])
    );
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![PendingOperation::ReplaceBy {
                relation: "Owns".to_owned(),
                unique_constraint: "missing".to_owned(),
                selected_key: vec![existing(item)],
                tuple: vec![existing(owner), existing(item)],
                metadata: OperationMetadata::cause("bad"),
            }],
            ..Transaction::default()
        }),
        Err("relation.unknown_unique")
    );

    let before_conflicting_replacements = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![
                PendingOperation::ReplaceBy {
                    relation: "Owns".to_owned(),
                    unique_constraint: "item".to_owned(),
                    selected_key: vec![existing(item)],
                    tuple: vec![existing(owner), existing(item)],
                    metadata: OperationMetadata::cause("replace.one"),
                },
                PendingOperation::ReplaceBy {
                    relation: "Owns".to_owned(),
                    unique_constraint: "item".to_owned(),
                    selected_key: vec![existing(item)],
                    tuple: vec![existing(new_owner), existing(item)],
                    metadata: OperationMetadata::cause("replace.two"),
                },
            ],
            ..Transaction::default()
        }),
        Err("relation.replacement_conflict")
    );
    assert_eq!(model, before_conflicting_replacements);

    model
        .relations
        .register(
            RelationSchema::new(
                "Account",
                vec![
                    ColumnSchema::new("user", ValueKind::Text),
                    ColumnSchema::new("email", ValueKind::Text),
                ],
            )
            .unique("user", &[0])
            .unique("email", &[1]),
        )
        .unwrap();
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(
                    pending_key(
                        "Account",
                        vec![
                            PendingValue::Text("alice".to_owned()),
                            PendingValue::Text("a@example".to_owned()),
                        ],
                    ),
                    "account.alice",
                ),
                insert(
                    pending_key(
                        "Account",
                        vec![
                            PendingValue::Text("bob".to_owned()),
                            PendingValue::Text("b@example".to_owned()),
                        ],
                    ),
                    "account.bob",
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let before_other_unique = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![PendingOperation::ReplaceBy {
                relation: "Account".to_owned(),
                unique_constraint: "user".to_owned(),
                selected_key: vec![PendingValue::Text("alice".to_owned())],
                tuple: vec![
                    PendingValue::Text("alice".to_owned()),
                    PendingValue::Text("b@example".to_owned()),
                ],
                metadata: OperationMetadata::cause("account.conflict"),
            }],
            ..Transaction::default()
        }),
        Err("relation.unique_conflict")
    );
    assert_eq!(model, before_other_unique);

    model
        .relations
        .register(
            RelationSchema::new(
                "AlliedPatch",
                vec![entity_column("left"), entity_column("right")],
            )
            .symmetric(),
        )
        .unwrap();
    let symmetric_before = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            operations: vec![
                insert(
                    pending_key("AlliedPatch", vec![existing(owner), existing(new_owner)]),
                    "symmetric.insert",
                ),
                remove(
                    pending_key("AlliedPatch", vec![existing(new_owner), existing(owner)]),
                    "symmetric.remove",
                ),
            ],
            ..Transaction::default()
        }),
        Err("relation.operation_conflict")
    );
    assert_eq!(model, symmetric_before);
}

#[test]
fn all_authoritative_rows_are_schema_validated_and_component_relation_failures_are_atomic() {
    let (mut model, owner, item, _) = seed_ownership_model();
    model.components.insert(
        (owner, "Position".to_owned()),
        FactValue::Text("base".to_owned()),
    );
    let before = model.clone();
    assert_eq!(
        model.apply_transaction(Transaction {
            spawn_handles: BTreeSet::from([1]),
            component_writes: vec![PendingComponentWrite {
                entity: EntityOperand::Existing(owner),
                component: "Position".to_owned(),
                value: FactValue::Text("candidate".to_owned()),
            }],
            operations: vec![insert(
                pending_key("ItemWeight", vec![existing(item), int(99)]),
                "duplicate.weight",
            )],
            ..Transaction::default()
        }),
        Err("relation.unique_conflict")
    );
    assert_eq!(
        model, before,
        "component, entity, and relation writes are one atomic candidate"
    );
}

#[test]
fn insertion_permutations_and_incremental_maintenance_match_full_recomputation() {
    let (seed, person, item_a, item_b) = seed_ownership_model();
    let mut forward = WorldModel {
        entities: seed.entities.clone(),
        components: seed.components.clone(),
        relations: RelationStore {
            schemas: seed.relations.schemas.clone(),
            ..RelationStore::default()
        },
    };
    let operations = vec![
        insert(
            pending_key("Owns", vec![existing(person), existing(item_a)]),
            "a",
        ),
        insert(
            pending_key("Owns", vec![existing(person), existing(item_b)]),
            "b",
        ),
        insert(
            pending_key("ItemWeight", vec![existing(item_a), int(7)]),
            "c",
        ),
        insert(
            pending_key("ItemWeight", vec![existing(item_b), int(6)]),
            "d",
        ),
        insert(
            pending_key("CarryCapacity", vec![existing(person), int(10)]),
            "e",
        ),
    ];
    forward
        .apply_transaction(Transaction {
            operations: operations.clone(),
            ..Transaction::default()
        })
        .unwrap();
    let mut reverse = WorldModel {
        entities: seed.entities.clone(),
        components: seed.components.clone(),
        relations: RelationStore {
            schemas: seed.relations.schemas.clone(),
            ..RelationStore::default()
        },
    };
    reverse
        .apply_transaction(Transaction {
            operations: operations.into_iter().rev().collect(),
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(forward.relations.assertions, reverse.relations.assertions);
    let schemas = derived_schemas();
    let rules = ownership_rules();
    let limits = limits(128);
    assert_eq!(
        derive_all(&forward.relations, &schemas, &rules, limits).unwrap(),
        derive_all(&reverse.relations, &schemas, &rules, limits).unwrap()
    );

    let mut incremental = DependencyMaintainer::new(forward, schemas, rules, limits).unwrap();
    for transaction in [
        Transaction {
            operations: vec![remove(
                pending_key("ItemWeight", vec![existing(item_b), int(6)]),
                "delta.remove_weight",
            )],
            ..Transaction::default()
        },
        Transaction {
            operations: vec![insert(
                pending_key("ItemWeight", vec![existing(item_b), int(1)]),
                "delta.insert_weight",
            )],
            ..Transaction::default()
        },
        Transaction {
            operations: vec![remove(
                pending_key("Owns", vec![existing(person), existing(item_a)]),
                "delta.remove_owns",
            )],
            ..Transaction::default()
        },
    ] {
        incremental.apply(transaction).unwrap();
        assert_eq!(
            incremental.derived,
            derive_all(
                &incremental.model.relations,
                &incremental.derived_schemas,
                &incremental.rules,
                limits,
            )
            .unwrap()
        );
    }
}

#[test]
fn alternative_proofs_are_unioned_and_final_support_removal_retracts_fact() {
    let mut model = WorldModel::default();
    for name in ["MarkerA", "MarkerB"] {
        model
            .relations
            .register(RelationSchema::new(name, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(pending_key("MarkerA", vec![int(7)]), "source.a"),
                insert(pending_key("MarkerB", vec![int(7)]), "source.b"),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let rules = ["MarkerA", "MarkerB"]
        .into_iter()
        .map(|source| RulePlan {
            id: format!("derive.Marked.{source}"),
            head_relation: "Marked".to_owned(),
            head: vec![Term::Constant(FactValue::Int(7))],
            atoms: vec![Atom::new(source, vec![Term::Constant(FactValue::Int(7))])],
            predicates: Vec::new(),
            aggregate: None,
        })
        .collect::<Vec<_>>();
    let schemas = derived_schemas();
    let limits = limits(32);
    let marked = FactKey::new("Marked", vec![FactValue::Int(7)]);
    let first = derive_all(&model.relations, &schemas, &rules, limits).unwrap();
    assert_eq!(first[&marked].len(), 2);
    model
        .apply_transaction(Transaction {
            operations: vec![remove(pending_key("MarkerA", vec![int(7)]), "remove.a")],
            ..Transaction::default()
        })
        .unwrap();
    let second = derive_all(&model.relations, &schemas, &rules, limits).unwrap();
    assert_eq!(second[&marked].len(), 1);
    model
        .apply_transaction(Transaction {
            operations: vec![remove(pending_key("MarkerB", vec![int(7)]), "remove.b")],
            ..Transaction::default()
        })
        .unwrap();
    assert!(!derive_all(&model.relations, &schemas, &rules, limits)
        .unwrap()
        .contains_key(&marked));
}

#[test]
fn aggregate_contract_is_checked_exact_and_bounded() {
    assert_eq!(aggregate_values(AggregateKind::Sum, &[]), Ok(None));
    assert_eq!(
        aggregate_values(
            AggregateKind::Sum,
            &[FactValue::Int(i64::MAX), FactValue::Int(1)],
        ),
        Err("derivation.sum_overflow")
    );
    assert_eq!(
        aggregate_values(AggregateKind::Min, &[FactValue::Int(9), FactValue::Int(-2)],),
        Ok(Some(FactValue::Int(-2)))
    );
    assert_eq!(
        aggregate_values(AggregateKind::Max, &[FactValue::Int(9), FactValue::Int(-2)],),
        Ok(Some(FactValue::Int(9)))
    );
    assert_eq!(checked_count(u64::MAX, 1), Err("derivation.count_overflow"));

    let (model, _, _, _) = seed_ownership_model();
    assert_eq!(
        derive_all(
            &model.relations,
            &derived_schemas(),
            &ownership_rules(),
            limits(1),
        ),
        Err("derivation.binding_limit")
    );
}

#[test]
fn rule_plans_are_range_restricted_and_cycles_fail_closed() {
    let unsafe_rule = RulePlan {
        id: "derive.Unsafe".to_owned(),
        head_relation: "Marked".to_owned(),
        head: vec![Term::var("unbound")],
        atoms: Vec::new(),
        predicates: Vec::new(),
        aggregate: None,
    };
    assert_eq!(
        unsafe_rule.validate(&RelationStore::default(), &derived_schemas()),
        Err("derivation.unbound_variable")
    );
    let cycle = RulePlan {
        id: "derive.Cycle".to_owned(),
        head_relation: "Marked".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![Atom::new("Marked", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    assert_eq!(
        derive_all(
            &RelationStore::default(),
            &derived_schemas(),
            &[cycle],
            limits(8),
        ),
        Err("derivation.cycle")
    );
}

#[test]
fn assertion_lifetimes_preserve_noop_ancestry_and_reinsert_gets_new_ancestry() {
    let (mut model, owner, item, _) = seed_ownership_model();
    let key = FactKey::new(
        "Owns",
        vec![FactValue::Entity(owner), FactValue::Entity(item)],
    );
    let first = model.relations.assertions[&key].clone();
    let semantic_before = fact_key_bytes(&key);
    let operational_before = operational_checkpoint_bytes(&model);
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "ignored.noop",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(model.relations.assertions[&key], first);
    model
        .apply_transaction(Transaction {
            operations: vec![remove(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "settlement.remove",
            )],
            ..Transaction::default()
        })
        .unwrap();
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "settlement.reinsert",
            )],
            ..Transaction::default()
        })
        .unwrap();
    let second = &model.relations.assertions[&key];
    assert_ne!(second.id, first.id);
    assert_eq!(
        second.causes,
        BTreeSet::from(["settlement.reinsert".to_owned()])
    );
    assert_eq!(fact_key_bytes(&key), semantic_before);
    assert_ne!(operational_checkpoint_bytes(&model), operational_before);
}

#[test]
fn why_chain_reaches_exact_assertion_versions_and_settlement_causes() {
    let (model, person, item_a, item_b) = seed_ownership_model();
    let derived = derive_all(
        &model.relations,
        &derived_schemas(),
        &ownership_rules(),
        limits(128),
    )
    .unwrap();
    let encumbered = FactKey::new("Encumbered", vec![FactValue::Entity(person)]);
    let encumbered_proof = derived[&encumbered].iter().next().unwrap();
    let total_support = encumbered_proof
        .supports
        .iter()
        .find(|support| support.key().relation == "TotalWeight")
        .unwrap();
    let total_proof_id = match total_support {
        SupportRef::Derived { proof_id, .. } => proof_id,
        SupportRef::Authoritative { .. } => panic!("TotalWeight must be derived"),
    };
    let total = total_support.key();
    assert_eq!(
        total.tuple,
        vec![FactValue::Entity(person), FactValue::Int(13)]
    );
    assert_eq!(
        total_proof_id,
        &derived[total].iter().next().unwrap().identity()
    );
    let total_proof = derived[total].iter().next().unwrap();
    for expected in [
        FactKey::new(
            "Owns",
            vec![FactValue::Entity(person), FactValue::Entity(item_a)],
        ),
        FactKey::new(
            "Owns",
            vec![FactValue::Entity(person), FactValue::Entity(item_b)],
        ),
        FactKey::new(
            "ItemWeight",
            vec![FactValue::Entity(item_a), FactValue::Int(7)],
        ),
        FactKey::new(
            "ItemWeight",
            vec![FactValue::Entity(item_b), FactValue::Int(6)],
        ),
    ] {
        let support = total_proof
            .supports
            .iter()
            .find(|support| support.key() == &expected)
            .unwrap();
        let SupportRef::Authoritative { assertion_id, .. } = support else {
            panic!("base support must be authoritative");
        };
        let assertion = &model.relations.assertions[&expected];
        assert_eq!(*assertion_id, assertion.id);
        assert!(!assertion.causes.is_empty());
    }
}

#[test]
fn capability_filtering_hides_proof_multiplicity_and_order() {
    let mut model = WorldModel::default();
    for name in ["VisibleSource", "HiddenSource"] {
        model
            .relations
            .register(RelationSchema::new(name, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: vec![
                PendingOperation::Insert(
                    pending_key("VisibleSource", vec![int(1)]),
                    OperationMetadata::cause("visible"),
                ),
                PendingOperation::Insert(
                    pending_key("HiddenSource", vec![int(1)]),
                    OperationMetadata::cause("hidden").with_capability("secret"),
                ),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let rules = ["VisibleSource", "HiddenSource"]
        .into_iter()
        .map(|source| RulePlan {
            id: format!("derive.Marked.{source}"),
            head_relation: "Marked".to_owned(),
            head: vec![Term::var("value")],
            atoms: vec![Atom::new(source, vec![Term::var("value")])],
            predicates: Vec::new(),
            aggregate: None,
        })
        .collect::<Vec<_>>();
    let derived = derive_all(&model.relations, &derived_schemas(), &rules, limits(16)).unwrap();
    let public = render_visible(&derived, &BTreeSet::new());
    let marked = FactKey::new("Marked", vec![FactValue::Int(1)]);
    let mut without_hidden = derived.clone();
    without_hidden
        .get_mut(&marked)
        .unwrap()
        .retain(|proof| proof.required_capabilities.is_empty());
    assert_eq!(public, render_visible(&without_hidden, &BTreeSet::new()));
    let privileged = render_visible(&derived, &BTreeSet::from(["secret".to_owned()]));
    assert_ne!(public, privileged);
}

#[test]
fn semantic_wire_round_trips_while_portable_replay_binds_assertion_identity() {
    let (mut model, owner, item, _) = seed_ownership_model();
    let key = FactKey::new(
        "Owns",
        vec![FactValue::Entity(owner), FactValue::Entity(item)],
    );
    assert_eq!(decode_fact_key(&fact_key_bytes(&key)).unwrap(), key);
    let generic_values = pending_key(
        "GenericValues",
        vec![PendingValue::Count(3), PendingValue::Text("tag".to_owned())],
    )
    .resolve(&BTreeMap::new())
    .unwrap();
    assert_eq!(
        decode_fact_key(&fact_key_bytes(&generic_values)).unwrap(),
        generic_values
    );
    let semantic_before = semantic_relation_bytes(&model.relations);
    assert_eq!(
        decode_semantic_relation_bytes(
            &semantic_before,
            &model.relations.schemas,
            &model.entities,
        )
        .unwrap(),
        model.relations.assertions.keys().cloned().collect()
    );
    let schemas = derived_schemas();
    let rules = ownership_rules();
    let limits = limits(128);
    let derived_before = derive_all(&model.relations, &schemas, &rules, limits).unwrap();
    let attempt = PortableAttempt {
        checkpoint: portable_checkpoint_bytes(&model, &derived_before),
    };
    model
        .apply_transaction(Transaction {
            operations: vec![remove(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "remove",
            )],
            ..Transaction::default()
        })
        .unwrap();
    model
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Owns", vec![existing(owner), existing(item)]),
                "reinsert",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(semantic_relation_bytes(&model.relations), semantic_before);
    let derived_after = derive_all(&model.relations, &schemas, &rules, limits).unwrap();
    assert_ne!(
        derivation_checkpoint_bytes(&derived_before),
        derivation_checkpoint_bytes(&derived_after)
    );
    let mut instructions = 0;
    assert_eq!(
        replay_portable(&model, &derived_after, &attempt, &mut instructions),
        Err("attempt.checkpoint_mismatch")
    );
    assert_eq!(instructions, 0);
}

#[test]
fn simultaneous_despawns_are_classified_as_one_set_before_any_cascade() {
    fn link_model(reverse_columns: bool) -> (WorldModel, EntityRef, EntityRef, EntityRef) {
        let mut model = WorldModel::default();
        let columns = if reverse_columns {
            vec![entity_column("target"), entity_column("source").cascade()]
        } else {
            vec![entity_column("source").cascade(), entity_column("target")]
        };
        model
            .relations
            .register(
                RelationSchema::new("Link", columns)
                    .unique("source", &[usize::from(reverse_columns)]),
            )
            .unwrap();
        let a = model.entities.spawn();
        let b = model.entities.spawn();
        let c = model.entities.spawn();
        let tuple = if reverse_columns {
            vec![existing(b), existing(a)]
        } else {
            vec![existing(a), existing(b)]
        };
        model
            .apply_transaction(Transaction {
                operations: vec![insert(pending_key("Link", tuple), "link.insert")],
                ..Transaction::default()
            })
            .unwrap();
        (model, a, b, c)
    }

    for reverse_columns in [false, true] {
        let (model, a, b, _) = link_model(reverse_columns);
        for despawns in [
            vec![despawn(a, "despawn.a"), despawn(b, "despawn.b")],
            vec![despawn(b, "despawn.b"), despawn(a, "despawn.a")],
        ] {
            let mut candidate = model.clone();
            assert_eq!(
                candidate.apply_transaction(Transaction {
                    despawns,
                    ..Transaction::default()
                }),
                Err("entity.delete_restricted")
            );
            assert_eq!(candidate, model);
        }
    }

    let (mut explicit_remove, a, b, _) = link_model(false);
    explicit_remove
        .apply_transaction(Transaction {
            despawns: vec![despawn(a, "despawn.a"), despawn(b, "despawn.b")],
            operations: vec![remove(
                pending_key("Link", vec![existing(a), existing(b)]),
                "link.explicit_remove",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert!(explicit_remove.relations.assertions.is_empty());

    let (mut replaced, a, b, c) = link_model(false);
    replaced
        .apply_transaction(Transaction {
            despawns: vec![despawn(b, "despawn.old_target")],
            operations: vec![PendingOperation::ReplaceBy {
                relation: "Link".to_owned(),
                unique_constraint: "source".to_owned(),
                selected_key: vec![existing(a)],
                tuple: vec![existing(a), existing(c)],
                metadata: OperationMetadata::cause("link.replace"),
            }],
            ..Transaction::default()
        })
        .unwrap();
    assert!(replaced.relations.assertions.contains_key(&FactKey::new(
        "Link",
        vec![FactValue::Entity(a), FactValue::Entity(c)],
    )));

    let (mut cascade, a, _, _) = link_model(false);
    cascade
        .apply_transaction(Transaction {
            despawns: vec![PendingDespawn {
                entity: a,
                metadata: OperationMetadata::cause("settlement.despawn.a")
                    .with_capability("world.delete"),
            }],
            ..Transaction::default()
        })
        .unwrap();
    let cascade_change = cascade
        .relations
        .last_changes
        .iter()
        .find(|change| change.kind == ChangeKind::Cascade)
        .unwrap();
    assert_eq!(
        cascade_change.causes,
        BTreeSet::from(["settlement.despawn.a".to_owned()])
    );
    assert_eq!(
        cascade_change.required_capabilities,
        BTreeSet::from(["world.delete".to_owned()])
    );

    let (mut same_entity, a, _, _) = link_model(false);
    same_entity
        .apply_transaction(Transaction {
            operations: vec![PendingOperation::ReplaceBy {
                relation: "Link".to_owned(),
                unique_constraint: "source".to_owned(),
                selected_key: vec![existing(a)],
                tuple: vec![existing(a), existing(a)],
                metadata: OperationMetadata::cause("link.self"),
            }],
            ..Transaction::default()
        })
        .unwrap();
    let before = same_entity.clone();
    assert_eq!(
        same_entity.apply_transaction(Transaction {
            despawns: vec![despawn(a, "despawn.self")],
            ..Transaction::default()
        }),
        Err("entity.delete_restricted")
    );
    assert_eq!(same_entity, before);

    let mut inserted_then_despawned = WorldModel::default();
    inserted_then_despawned
        .relations
        .register(RelationSchema::new(
            "CascadeOnly",
            vec![entity_column("source").cascade(), entity_column("target")],
        ))
        .unwrap();
    let source = inserted_then_despawned.entities.spawn();
    let target = inserted_then_despawned.entities.spawn();
    let next_assertion = inserted_then_despawned.relations.next_assertion_id;
    inserted_then_despawned
        .apply_transaction(Transaction {
            despawns: vec![despawn(source, "despawn.inserted_source")],
            operations: vec![insert(
                pending_key("CascadeOnly", vec![existing(source), existing(target)]),
                "insert.before.cascade",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert!(inserted_then_despawned.relations.assertions.is_empty());
    assert!(inserted_then_despawned.relations.last_changes.is_empty());
    assert_eq!(
        inserted_then_despawned.relations.next_assertion_id, next_assertion,
        "a row that never commits has no assertion lifetime"
    );
}

#[test]
fn operational_state_binds_generations_assertion_allocator_and_restoration() {
    let mut generation_a = WorldModel::default();
    let first = generation_a.entities.spawn();
    generation_a.entities.despawn(first).unwrap();
    let mut generation_b = generation_a.clone();
    let reused = generation_b.entities.spawn();
    generation_b.entities.despawn(reused).unwrap();
    assert_eq!(generation_a.entities.live, generation_b.entities.live);
    assert_eq!(
        generation_a.entities.free_slots,
        generation_b.entities.free_slots
    );
    assert_ne!(
        operational_checkpoint_bytes(&generation_a),
        operational_checkpoint_bytes(&generation_b)
    );

    let state = OperationalRelationState::capture(&generation_a);
    let mut restored = state.restore(generation_a.relations.schemas.clone());
    let mut original = generation_a.clone();
    assert_eq!(restored.entities.spawn(), original.entities.spawn());

    let mut assertion_a = WorldModel::default();
    assertion_a
        .relations
        .register(RelationSchema::new("Marker", vec![int_column("value")]))
        .unwrap();
    let mut assertion_b = assertion_a.clone();
    assertion_b
        .apply_transaction(Transaction {
            operations: vec![insert(pending_key("Marker", vec![int(1)]), "insert")],
            ..Transaction::default()
        })
        .unwrap();
    assertion_b
        .apply_transaction(Transaction {
            operations: vec![remove(pending_key("Marker", vec![int(1)]), "remove")],
            ..Transaction::default()
        })
        .unwrap();
    assert!(assertion_a.relations.assertions.is_empty());
    assert!(assertion_b.relations.assertions.is_empty());
    assert_ne!(
        operational_checkpoint_bytes(&assertion_a),
        operational_checkpoint_bytes(&assertion_b)
    );
    let attempt = PortableAttempt {
        checkpoint: portable_checkpoint_bytes(&assertion_a, &DerivationResult::new()),
    };
    let mut instructions = 0;
    assert_eq!(
        replay_portable(
            &assertion_b,
            &DerivationResult::new(),
            &attempt,
            &mut instructions,
        ),
        Err("attempt.checkpoint_mismatch")
    );
    assert_eq!(instructions, 0);

    let state = OperationalRelationState::capture(&assertion_b);
    let mut restored = state.restore(assertion_b.relations.schemas.clone());
    for model in [&mut assertion_b, &mut restored] {
        model
            .apply_transaction(Transaction {
                operations: vec![insert(
                    pending_key("Marker", vec![int(2)]),
                    "next.assertion",
                )],
                ..Transaction::default()
            })
            .unwrap();
    }
    assert_eq!(
        assertion_b.relations.assertions,
        restored.relations.assertions
    );
}

#[test]
fn schemas_and_semantic_wire_reject_ambiguous_or_noncanonical_input() {
    assert_eq!(
        RelationSchema::new("Ambiguous", vec![int_column("value")])
            .unique("same", &[0])
            .unique("same", &[0])
            .validate_declaration(),
        Err("relation.duplicate_unique_name")
    );
    assert_eq!(
        RelationSchema::new(
            "DuplicateColumns",
            vec![int_column("value"), int_column("value")],
        )
        .validate_declaration(),
        Err("relation.duplicate_column_name")
    );
    assert_eq!(
        RelationSchema::new("DuplicateUniqueIndex", vec![int_column("value")])
            .unique("value", &[0, 0])
            .validate_declaration(),
        Err("relation.unique_shape")
    );
    assert_eq!(
        RelationSchema::new("NonEntityDelete", vec![int_column("value").cascade()])
            .validate_declaration(),
        Err("relation.delete_policy_non_entity")
    );
    assert_eq!(
        RelationSchema::new("", vec![int_column("value")]).validate_declaration(),
        Err("relation.empty_name")
    );
    assert_eq!(
        RelationSchema::new("EmptyColumn", vec![int_column("")]).validate_declaration(),
        Err("relation.empty_column_name")
    );
    assert_eq!(
        RelationSchema::new("EmptyUnique", vec![int_column("value")])
            .unique("", &[0])
            .validate_declaration(),
        Err("relation.empty_unique_name")
    );

    let a = FactKey::new("A", vec![FactValue::Int(1)]);
    let b = FactKey::new("B", vec![FactValue::Int(2)]);
    let schemas = BTreeMap::from([
        (
            "A".to_owned(),
            RelationSchema::new("A", vec![int_column("value")]),
        ),
        (
            "B".to_owned(),
            RelationSchema::new("B", vec![int_column("value")]),
        ),
        (
            "S".to_owned(),
            RelationSchema::new("S", vec![entity_column("left"), entity_column("right")])
                .symmetric(),
        ),
    ]);
    let mut duplicate = b"rfc0003.semantic.v1".to_vec();
    write_u64(&mut duplicate, 2);
    encode_fact_key(&mut duplicate, &a);
    encode_fact_key(&mut duplicate, &a);
    assert_eq!(
        decode_semantic_relation_bytes(&duplicate, &schemas, &EntityTable::default()),
        Err("wire.noncanonical_fact_order")
    );
    let mut out_of_order = b"rfc0003.semantic.v1".to_vec();
    write_u64(&mut out_of_order, 2);
    encode_fact_key(&mut out_of_order, &b);
    encode_fact_key(&mut out_of_order, &a);
    assert_eq!(
        decode_semantic_relation_bytes(&out_of_order, &schemas, &EntityTable::default()),
        Err("wire.noncanonical_fact_order")
    );
    let one_fact = |fact: &FactKey| {
        let mut bytes = b"rfc0003.semantic.v1".to_vec();
        write_u64(&mut bytes, 1);
        encode_fact_key(&mut bytes, fact);
        bytes
    };
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new("Unknown", vec![FactValue::Int(1)])),
            &schemas,
            &EntityTable::default(),
        ),
        Err("wire.unknown_relation")
    );
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new("A", Vec::new())),
            &schemas,
            &EntityTable::default(),
        ),
        Err("wire.relation_arity")
    );
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new("A", vec![FactValue::Text("1".to_owned())])),
            &schemas,
            &EntityTable::default(),
        ),
        Err("wire.relation_type")
    );
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new(
                "S",
                vec![
                    FactValue::Entity(EntityRef {
                        slot: 9,
                        generation: 0,
                    }),
                    FactValue::Entity(EntityRef {
                        slot: 4,
                        generation: 0,
                    }),
                ],
            )),
            &schemas,
            &EntityTable::default(),
        ),
        Err("wire.noncanonical_tuple")
    );
    let dead = EntityRef {
        slot: 4,
        generation: 0,
    };
    assert_eq!(
        decode_semantic_relation_bytes(
            &one_fact(&FactKey::new(
                "S",
                vec![FactValue::Entity(dead), FactValue::Entity(dead)],
            )),
            &schemas,
            &EntityTable::default(),
        ),
        Err("wire.entity_not_live")
    );
}

#[test]
fn hidden_proof_branches_do_not_change_transitive_or_aggregate_public_bytes() {
    let mut model = WorldModel::default();
    for source in [
        "VisibleSourceA",
        "VisibleSourceB",
        "HiddenSourceA",
        "HiddenSourceB",
        "JoinSource",
    ] {
        model
            .relations
            .register(RelationSchema::new(source, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: vec![
                PendingOperation::Insert(
                    pending_key("VisibleSourceA", vec![int(1)]),
                    OperationMetadata::cause("visible.a"),
                ),
                PendingOperation::Insert(
                    pending_key("VisibleSourceB", vec![int(1)]),
                    OperationMetadata::cause("visible.b"),
                ),
                PendingOperation::Insert(
                    pending_key("HiddenSourceA", vec![int(1)]),
                    OperationMetadata::cause("hidden.a").with_capability("secret.a"),
                ),
                PendingOperation::Insert(
                    pending_key("HiddenSourceB", vec![int(1)]),
                    OperationMetadata::cause("hidden.b").with_capability("secret.b"),
                ),
                insert(pending_key("JoinSource", vec![int(1)]), "join.visible"),
            ],
            ..Transaction::default()
        })
        .unwrap();

    let mut schemas = derived_schemas();
    for schema in [
        RelationSchema::new("Public", vec![int_column("value")]),
        RelationSchema::new("Joined", vec![int_column("value")]),
        RelationSchema::new("CountMarked", vec![count_column("count")]),
        RelationSchema::new("SumMarked", vec![int_column("sum")]),
        RelationSchema::new("MinMarked", vec![int_column("min")]),
        RelationSchema::new("MaxMarked", vec![int_column("max")]),
    ] {
        schemas.insert(schema.name.clone(), schema);
    }
    let mut rules = [
        "VisibleSourceA",
        "VisibleSourceB",
        "HiddenSourceA",
        "HiddenSourceB",
    ]
    .into_iter()
    .map(|source| RulePlan {
        id: format!("derive.Marked.{source}"),
        head_relation: "Marked".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![Atom::new(source, vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    })
    .collect::<Vec<_>>();
    rules.extend([
        RulePlan {
            id: "derive.Public".to_owned(),
            head_relation: "Public".to_owned(),
            head: vec![Term::var("value")],
            atoms: vec![Atom::new("Marked", vec![Term::var("value")])],
            predicates: Vec::new(),
            aggregate: None,
        },
        RulePlan {
            id: "derive.Joined".to_owned(),
            head_relation: "Joined".to_owned(),
            head: vec![Term::var("value")],
            atoms: vec![
                Atom::new("Marked", vec![Term::var("value")]),
                Atom::new("JoinSource", vec![Term::var("value")]),
            ],
            predicates: Vec::new(),
            aggregate: None,
        },
    ]);
    for (relation, output, kind) in [
        ("CountMarked", "count", AggregateKind::Count),
        ("SumMarked", "sum", AggregateKind::Sum),
        ("MinMarked", "min", AggregateKind::Min),
        ("MaxMarked", "max", AggregateKind::Max),
    ] {
        rules.push(RulePlan {
            id: format!("derive.{relation}"),
            head_relation: relation.to_owned(),
            head: vec![Term::var(output)],
            atoms: vec![Atom::new("Marked", vec![Term::var("value")])],
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind,
                input: (!matches!(kind, AggregateKind::Count)).then(|| "value".to_owned()),
                output: output.to_owned(),
                group_by: Vec::new(),
            }),
        });
    }

    let with_hidden = derive_all(&model.relations, &schemas, &rules, limits(256)).unwrap();
    let mut without_hidden_model = model.clone();
    without_hidden_model
        .apply_transaction(Transaction {
            operations: vec![
                remove(pending_key("HiddenSourceA", vec![int(1)]), "remove.a"),
                remove(pending_key("HiddenSourceB", vec![int(1)]), "remove.b"),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let without_hidden = derive_all(
        &without_hidden_model.relations,
        &schemas,
        &rules,
        limits(256),
    )
    .unwrap();
    assert_eq!(
        render_visible(&with_hidden, &BTreeSet::new()),
        render_visible(&without_hidden, &BTreeSet::new())
    );
    assert_eq!(
        with_hidden[&FactKey::new("CountMarked", vec![FactValue::Count(1)])].len(),
        4,
        "one logical binding contributes once while provenance branches remain separate"
    );
    assert_ne!(
        render_visible(
            &with_hidden,
            &BTreeSet::from(["secret.a".to_owned(), "secret.b".to_owned()]),
        ),
        render_visible(&with_hidden, &BTreeSet::new())
    );
}

#[test]
fn aggregate_checker_rejects_nongrouped_projection_and_invalid_shapes() {
    let (model, _, _, _) = seed_ownership_model();
    let mut schemas = derived_schemas();
    for schema in [
        RelationSchema::new(
            "BadProjection",
            vec![
                entity_column("person"),
                entity_column("item"),
                count_column("count"),
            ],
        ),
        RelationSchema::new(
            "CountByPerson",
            vec![entity_column("person"), count_column("count")],
        ),
        RelationSchema::new(
            "BadBoundOutput",
            vec![entity_column("person"), count_column("item")],
        ),
        RelationSchema::new(
            "SumByPerson",
            vec![entity_column("person"), int_column("sum")],
        ),
        RelationSchema::new(
            "WrongOutput",
            vec![entity_column("person"), count_column("sum")],
        ),
    ] {
        schemas.insert(schema.name.clone(), schema);
    }
    let count_rule = |head_relation: &str, head: Vec<Term>, group_by: Vec<&str>, input| RulePlan {
        id: format!("derive.{head_relation}"),
        head_relation: head_relation.to_owned(),
        head,
        atoms: vec![Atom::new(
            "Owns",
            vec![Term::var("person"), Term::var("item")],
        )],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Count,
            input,
            output: "count".to_owned(),
            group_by: group_by.into_iter().map(str::to_owned).collect(),
        }),
    };
    assert_eq!(
        count_rule(
            "BadProjection",
            vec![Term::var("person"), Term::var("item"), Term::var("count")],
            vec!["person"],
            None,
        )
        .validate(&model.relations, &schemas),
        Err("derivation.aggregate_head_projection")
    );
    let bound_output = RulePlan {
        id: "derive.BadBoundOutput".to_owned(),
        head_relation: "BadBoundOutput".to_owned(),
        head: vec![Term::var("person"), Term::var("item")],
        atoms: vec![Atom::new(
            "Owns",
            vec![Term::var("person"), Term::var("item")],
        )],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Count,
            input: None,
            output: "item".to_owned(),
            group_by: vec!["person".to_owned()],
        }),
    };
    assert_eq!(
        bound_output.validate(&model.relations, &schemas),
        Err("derivation.aggregate_output_not_fresh")
    );
    assert_eq!(
        count_rule(
            "CountByPerson",
            vec![Term::var("person"), Term::var("count")],
            vec!["person", "person"],
            None,
        )
        .validate(&model.relations, &schemas),
        Err("derivation.duplicate_group")
    );
    assert_eq!(
        count_rule(
            "CountByPerson",
            vec![Term::var("person"), Term::var("count")],
            vec!["person"],
            Some("item".to_owned()),
        )
        .validate(&model.relations, &schemas),
        Err("derivation.count_input")
    );

    let invalid_sum = |head_relation: &str, input: &str| RulePlan {
        id: format!("derive.{head_relation}"),
        head_relation: head_relation.to_owned(),
        head: vec![Term::var("person"), Term::var("sum")],
        atoms: vec![Atom::new(
            "Owns",
            vec![Term::var("person"), Term::var("item")],
        )],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Sum,
            input: Some(input.to_owned()),
            output: "sum".to_owned(),
            group_by: vec!["person".to_owned()],
        }),
    };
    assert_eq!(
        invalid_sum("SumByPerson", "item").validate(&model.relations, &schemas),
        Err("derivation.aggregate_type")
    );

    let wrong_output = RulePlan {
        id: "derive.WrongOutput".to_owned(),
        head_relation: "WrongOutput".to_owned(),
        head: vec![Term::var("person"), Term::var("sum")],
        atoms: vec![
            Atom::new("Owns", vec![Term::var("person"), Term::var("item")]),
            Atom::new("ItemWeight", vec![Term::var("item"), Term::var("weight")]),
        ],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Sum,
            input: Some("weight".to_owned()),
            output: "sum".to_owned(),
            group_by: vec!["person".to_owned()],
        }),
    };
    assert_eq!(
        wrong_output.validate(&model.relations, &schemas),
        Err("derivation.head_type")
    );

    let bad_atom_arity = RulePlan {
        id: "derive.BadArity".to_owned(),
        head_relation: "Marked".to_owned(),
        head: vec![Term::Constant(FactValue::Int(1))],
        atoms: vec![Atom::new("Owns", vec![Term::var("person")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    assert_eq!(
        bad_atom_arity.validate(&model.relations, &schemas),
        Err("derivation.atom_arity")
    );
    let bad_atom_type = RulePlan {
        id: "derive.BadType".to_owned(),
        head_relation: "Marked".to_owned(),
        head: vec![Term::Constant(FactValue::Int(1))],
        atoms: vec![Atom::new(
            "Owns",
            vec![Term::Constant(FactValue::Int(1)), Term::var("item")],
        )],
        predicates: Vec::new(),
        aggregate: None,
    };
    assert_eq!(
        bad_atom_type.validate(&model.relations, &schemas),
        Err("derivation.atom_type")
    );

    let incompatible_heads = vec![
        RulePlan {
            id: "derive.Marked.weight".to_owned(),
            head_relation: "Marked".to_owned(),
            head: vec![Term::var("weight")],
            atoms: vec![Atom::new(
                "ItemWeight",
                vec![Term::var("item"), Term::var("weight")],
            )],
            predicates: Vec::new(),
            aggregate: None,
        },
        RulePlan {
            id: "derive.Marked.person".to_owned(),
            head_relation: "Marked".to_owned(),
            head: vec![Term::var("person")],
            atoms: vec![Atom::new(
                "Owns",
                vec![Term::var("person"), Term::var("item")],
            )],
            predicates: Vec::new(),
            aggregate: None,
        },
    ];
    assert_eq!(
        derive_all(&model.relations, &schemas, &incompatible_heads, limits(32)),
        Err("derivation.head_type")
    );
}

#[test]
fn derivation_limits_bound_proofs_depth_supports_capabilities_and_bytes_atomically() {
    let mut model = WorldModel::default();
    for source in ["SourceA", "SourceB", "SourceC", "SourceD"] {
        model
            .relations
            .register(RelationSchema::new(source, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: ["SourceA", "SourceB", "SourceC", "SourceD"]
                .into_iter()
                .map(|source| {
                    PendingOperation::Insert(
                        pending_key(source, vec![int(1)]),
                        OperationMetadata::cause(source).with_capability(&format!("read.{source}")),
                    )
                })
                .collect(),
            ..Transaction::default()
        })
        .unwrap();
    let mut schemas = derived_schemas();
    for schema in [
        RelationSchema::new("D1", vec![int_column("value")]),
        RelationSchema::new("D2", vec![int_column("value")]),
        RelationSchema::new("D3", vec![int_column("value")]),
        RelationSchema::new("Left", vec![int_column("value")]),
        RelationSchema::new("Right", vec![int_column("value")]),
        RelationSchema::new("Combined", vec![int_column("value")]),
        RelationSchema::new("CountBranches", vec![count_column("count")]),
    ] {
        schemas.insert(schema.name.clone(), schema);
    }
    let source_rule = |source: &str, head: &str| RulePlan {
        id: format!("derive.{head}.{source}"),
        head_relation: head.to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![Atom::new(source, vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    let marked_rules = ["SourceA", "SourceB", "SourceC"]
        .into_iter()
        .map(|source| source_rule(source, "Marked"))
        .collect::<Vec<_>>();

    let bounded_result = derive_all(
        &model.relations,
        &schemas,
        &marked_rules[..1],
        DerivationLimits::generous(),
    )
    .unwrap();
    let exact_canonical_bytes = canonical_derivation_bytes(&bounded_result).len();
    let mut exact_byte_limit = DerivationLimits::generous();
    exact_byte_limit.max_canonical_bytes = exact_canonical_bytes;
    assert!(derive_all(
        &model.relations,
        &schemas,
        &marked_rules[..1],
        exact_byte_limit,
    )
    .is_ok());
    exact_byte_limit.max_canonical_bytes -= 1;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &marked_rules[..1],
            exact_byte_limit,
        ),
        Err("derivation.canonical_byte_limit")
    );

    let mut proof_limit = DerivationLimits::generous();
    proof_limit.max_proofs_per_fact = 2;
    assert!(derive_all(&model.relations, &schemas, &marked_rules[..2], proof_limit,).is_ok());
    assert_eq!(
        derive_all(&model.relations, &schemas, &marked_rules, proof_limit),
        Err("derivation.proofs_per_fact_limit")
    );

    let mut capability_limit = DerivationLimits::generous();
    capability_limit.max_capability_alternatives = 2;
    assert_eq!(
        derive_all(&model.relations, &schemas, &marked_rules, capability_limit),
        Err("derivation.capability_alternative_limit")
    );
    let mut support_limit = DerivationLimits::generous();
    support_limit.max_support_nodes = 0;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &marked_rules[..1],
            support_limit
        ),
        Err("derivation.support_limit")
    );
    let mut byte_limit = DerivationLimits::generous();
    byte_limit.max_canonical_bytes = 1;
    assert_eq!(
        derive_all(&model.relations, &schemas, &marked_rules[..1], byte_limit),
        Err("derivation.canonical_byte_limit")
    );
    let mut fact_limit = DerivationLimits::generous();
    fact_limit.max_facts = 0;
    assert_eq!(
        derive_all(&model.relations, &schemas, &marked_rules[..1], fact_limit),
        Err("derivation.fact_limit")
    );
    let mut total_proof_limit = DerivationLimits::generous();
    total_proof_limit.max_total_proofs = 0;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &marked_rules[..1],
            total_proof_limit,
        ),
        Err("derivation.total_proof_limit")
    );

    let depth_rules = vec![
        source_rule("SourceA", "D1"),
        source_rule("D1", "D2"),
        source_rule("D2", "D3"),
    ];
    let mut depth_limit = DerivationLimits::generous();
    depth_limit.max_proof_depth = 2;
    assert_eq!(
        derive_all(&model.relations, &schemas, &depth_rules, depth_limit),
        Err("derivation.depth_limit")
    );

    let branch_rules = vec![
        source_rule("SourceA", "Left"),
        source_rule("SourceB", "Left"),
        source_rule("SourceC", "Right"),
        source_rule("SourceD", "Right"),
        RulePlan {
            id: "derive.Combined".to_owned(),
            head_relation: "Combined".to_owned(),
            head: vec![Term::var("value")],
            atoms: vec![
                Atom::new("Left", vec![Term::var("value")]),
                Atom::new("Right", vec![Term::var("value")]),
            ],
            predicates: Vec::new(),
            aggregate: None,
        },
    ];
    let mut branch_limit = DerivationLimits::generous();
    branch_limit.max_capability_alternatives = 3;
    assert_eq!(
        derive_all(&model.relations, &schemas, &branch_rules, branch_limit),
        Err("derivation.capability_alternative_limit")
    );

    let aggregate_rules = marked_rules
        .iter()
        .cloned()
        .chain([RulePlan {
            id: "derive.CountBranches".to_owned(),
            head_relation: "CountBranches".to_owned(),
            head: vec![Term::var("count")],
            atoms: vec![Atom::new("Marked", vec![Term::var("value")])],
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind: AggregateKind::Count,
                input: None,
                output: "count".to_owned(),
                group_by: Vec::new(),
            }),
        }])
        .collect::<Vec<_>>();
    let mut aggregate_branch_limit = DerivationLimits::generous();
    aggregate_branch_limit.max_proof_combination_attempts = 3;
    assert!(derive_all(
        &model.relations,
        &schemas,
        &aggregate_rules,
        aggregate_branch_limit,
    )
    .is_ok());
    aggregate_branch_limit.max_proof_combination_attempts = 2;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &aggregate_rules,
            aggregate_branch_limit,
        ),
        Err("derivation.proof_combination_limit")
    );
    aggregate_branch_limit.max_proof_combination_attempts = usize::MAX;
    aggregate_branch_limit.max_capability_alternatives = 2;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &aggregate_rules,
            aggregate_branch_limit,
        ),
        Err("derivation.capability_alternative_limit")
    );

    let mut incremental_model = WorldModel::default();
    for source in ["SourceA", "SourceB", "SourceC"] {
        incremental_model
            .relations
            .register(RelationSchema::new(source, vec![int_column("value")]))
            .unwrap();
    }
    incremental_model
        .apply_transaction(Transaction {
            operations: vec![insert(pending_key("SourceA", vec![int(1)]), "a")],
            ..Transaction::default()
        })
        .unwrap();
    let mut incremental = DependencyMaintainer::new(
        incremental_model.clone(),
        schemas.clone(),
        marked_rules.clone(),
        proof_limit,
    )
    .unwrap();
    let overflow = Transaction {
        operations: vec![
            insert(pending_key("SourceB", vec![int(1)]), "b"),
            insert(pending_key("SourceC", vec![int(1)]), "c"),
        ],
        ..Transaction::default()
    };
    let mut full_candidate = incremental_model.clone();
    full_candidate.apply_transaction(overflow.clone()).unwrap();
    let full_error = derive_all(
        &full_candidate.relations,
        &schemas,
        &marked_rules,
        proof_limit,
    );
    assert_eq!(full_error, Err("derivation.proofs_per_fact_limit"));
    assert_eq!(incremental.apply(overflow), full_error.map(|_| ()));
    assert_eq!(incremental.model, incremental_model);
}

#[test]
fn aggregates_require_positive_input_and_empty_scans_produce_no_row() {
    let mut model = WorldModel::default();
    model
        .relations
        .register(RelationSchema::new("Numbers", vec![int_column("value")]))
        .unwrap();
    let schemas = BTreeMap::from([
        (
            "GlobalCount".to_owned(),
            RelationSchema::new("GlobalCount", vec![count_column("count")]),
        ),
        (
            "GlobalInt".to_owned(),
            RelationSchema::new("GlobalInt", vec![int_column("value")]),
        ),
    ]);
    for kind in [
        AggregateKind::Count,
        AggregateKind::Sum,
        AggregateKind::Min,
        AggregateKind::Max,
    ] {
        let (head_relation, output, input) = if matches!(kind, AggregateKind::Count) {
            ("GlobalCount", "count", None)
        } else {
            ("GlobalInt", "value", Some("input".to_owned()))
        };
        let atomless = RulePlan {
            id: format!("derive.atomless.{kind:?}"),
            head_relation: head_relation.to_owned(),
            head: vec![Term::var(output)],
            atoms: Vec::new(),
            predicates: Vec::new(),
            aggregate: Some(AggregateSpec {
                kind,
                input,
                output: output.to_owned(),
                group_by: Vec::new(),
            }),
        };
        assert_eq!(
            atomless.validate(&model.relations, &schemas),
            Err("derivation.aggregate_requires_positive_input")
        );
    }

    let count = RulePlan {
        id: "derive.GlobalCount".to_owned(),
        head_relation: "GlobalCount".to_owned(),
        head: vec![Term::var("count")],
        atoms: vec![Atom::new("Numbers", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Count,
            input: None,
            output: "count".to_owned(),
            group_by: Vec::new(),
        }),
    };
    assert!(derive_all(
        &model.relations,
        &schemas,
        std::slice::from_ref(&count),
        DerivationLimits::generous(),
    )
    .unwrap()
    .is_empty());
    model
        .apply_transaction(Transaction {
            operations: vec![insert(pending_key("Numbers", vec![int(7)]), "number")],
            ..Transaction::default()
        })
        .unwrap();
    let derived = derive_all(
        &model.relations,
        &schemas,
        &[count],
        DerivationLimits::generous(),
    )
    .unwrap();
    assert!(derived.contains_key(&FactKey::new("GlobalCount", vec![FactValue::Count(1)],)));
}

#[test]
fn symmetric_endpoint_metadata_is_logically_symmetric() {
    assert!(RelationSchema::new(
        "CascadeFriend",
        vec![
            entity_column("left").cascade(),
            entity_column("right").cascade()
        ],
    )
    .symmetric()
    .validate_declaration()
    .is_ok());
    assert!(RelationSchema::new(
        "RestrictFriend",
        vec![entity_column("left"), entity_column("right")],
    )
    .symmetric()
    .validate_declaration()
    .is_ok());
    assert_eq!(
        RelationSchema::new(
            "MixedFriend",
            vec![entity_column("left").cascade(), entity_column("right")],
        )
        .symmetric()
        .validate_declaration(),
        Err("relation.symmetric_endpoint_metadata")
    );

    for delete_second in [false, true] {
        let mut model = WorldModel::default();
        model
            .relations
            .register(
                RelationSchema::new(
                    "RestrictedFriend",
                    vec![entity_column("left"), entity_column("right")],
                )
                .symmetric(),
            )
            .unwrap();
        let a = model.entities.spawn();
        let b = model.entities.spawn();
        model
            .apply_transaction(Transaction {
                operations: vec![insert(
                    pending_key("RestrictedFriend", vec![existing(a), existing(b)]),
                    "restricted.friend",
                )],
                ..Transaction::default()
            })
            .unwrap();
        let before = model.clone();
        assert_eq!(
            model.apply_transaction(Transaction {
                despawns: vec![despawn(
                    if delete_second { b } else { a },
                    "despawn.restricted.friend",
                )],
                ..Transaction::default()
            }),
            Err("entity.delete_restricted")
        );
        assert_eq!(model, before);
    }

    for reverse in [false, true] {
        let mut model = WorldModel::default();
        model
            .relations
            .register(
                RelationSchema::new(
                    "Friend",
                    vec![
                        entity_column("left").cascade(),
                        entity_column("right").cascade(),
                    ],
                )
                .symmetric(),
            )
            .unwrap();
        let a = model.entities.spawn();
        let b = model.entities.spawn();
        let tuple = if reverse { (b, a) } else { (a, b) };
        model
            .apply_transaction(Transaction {
                operations: vec![insert(
                    pending_key("Friend", vec![existing(tuple.0), existing(tuple.1)]),
                    "friend",
                )],
                ..Transaction::default()
            })
            .unwrap();
        model
            .apply_transaction(Transaction {
                despawns: vec![despawn(tuple.0, "despawn.friend")],
                ..Transaction::default()
            })
            .unwrap();
        assert!(model.relations.assertions.is_empty());
    }

    let mut self_edge = WorldModel::default();
    self_edge
        .relations
        .register(
            RelationSchema::new(
                "Friend",
                vec![
                    entity_column("left").cascade(),
                    entity_column("right").cascade(),
                ],
            )
            .symmetric(),
        )
        .unwrap();
    let entity = self_edge.entities.spawn();
    self_edge
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Friend", vec![existing(entity), existing(entity)]),
                "self.friend",
            )],
            ..Transaction::default()
        })
        .unwrap();
    self_edge
        .apply_transaction(Transaction {
            despawns: vec![despawn(entity, "despawn.self")],
            ..Transaction::default()
        })
        .unwrap();
    assert!(self_edge.relations.assertions.is_empty());
}

#[test]
fn final_candidate_precedes_uniqueness_and_assertion_allocation() {
    let mut transient = WorldModel::default();
    transient
        .relations
        .register(RelationSchema::new(
            "Temporary",
            vec![entity_column("entity").cascade()],
        ))
        .unwrap();
    let doomed = transient.entities.spawn();
    transient.relations.next_assertion_id = u64::MAX;
    transient
        .apply_transaction(Transaction {
            despawns: vec![despawn(doomed, "despawn.temporary")],
            operations: vec![insert(
                pending_key("Temporary", vec![existing(doomed)]),
                "insert.temporary",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert!(transient.relations.assertions.is_empty());
    assert_eq!(transient.relations.next_assertion_id, u64::MAX);

    let mut surviving = WorldModel::default();
    surviving
        .relations
        .register(RelationSchema::new("Marker", vec![int_column("value")]))
        .unwrap();
    surviving.relations.next_assertion_id = u64::MAX;
    let before = surviving.clone();
    assert_eq!(
        surviving.apply_transaction(Transaction {
            operations: vec![insert(pending_key("Marker", vec![int(1)]), "survives")],
            ..Transaction::default()
        }),
        Err("relation.assertion_id_overflow")
    );
    assert_eq!(surviving, before);

    let owns_model = |reverse_owner_ids: bool| {
        let mut model = WorldModel::default();
        model
            .relations
            .register(
                RelationSchema::new(
                    "Owns",
                    vec![entity_column("owner"), entity_column("item").cascade()],
                )
                .unique("item", &[1]),
            )
            .unwrap();
        let first_owner = model.entities.spawn();
        let second_owner = model.entities.spawn();
        let (alice, bob) = if reverse_owner_ids {
            (second_owner, first_owner)
        } else {
            (first_owner, second_owner)
        };
        let sword = model.entities.spawn();
        model
            .apply_transaction(Transaction {
                operations: vec![insert(
                    pending_key("Owns", vec![existing(alice), existing(sword)]),
                    "owns.alice",
                )],
                ..Transaction::default()
            })
            .unwrap();
        (model, alice, bob, sword)
    };
    for reverse in [false, true] {
        let (mut model, _, bob, sword) = owns_model(reverse);
        model
            .apply_transaction(Transaction {
                despawns: vec![despawn(sword, "despawn.sword")],
                operations: vec![insert(
                    pending_key("Owns", vec![existing(bob), existing(sword)]),
                    "owns.bob",
                )],
                ..Transaction::default()
            })
            .unwrap();
        assert!(model.relations.assertions.is_empty());
    }

    let mut one_cascades = WorldModel::default();
    one_cascades
        .relations
        .register(
            RelationSchema::new(
                "Holds",
                vec![entity_column("owner"), entity_column("item").cascade()],
            )
            .unique("owner", &[0]),
        )
        .unwrap();
    let owner = one_cascades.entities.spawn();
    let retained_item = one_cascades.entities.spawn();
    let doomed_item = one_cascades.entities.spawn();
    one_cascades
        .apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Holds", vec![existing(owner), existing(retained_item)]),
                "base.holds",
            )],
            ..Transaction::default()
        })
        .unwrap();
    let base = one_cascades.clone();
    one_cascades
        .apply_transaction(Transaction {
            despawns: vec![despawn(doomed_item, "despawn.new.item")],
            operations: vec![insert(
                pending_key("Holds", vec![existing(owner), existing(doomed_item)]),
                "candidate.holds",
            )],
            ..Transaction::default()
        })
        .unwrap();
    assert_eq!(one_cascades.relations.assertions, base.relations.assertions);
    let mut conflicting = base.clone();
    assert_eq!(
        conflicting.apply_transaction(Transaction {
            operations: vec![insert(
                pending_key("Holds", vec![existing(owner), existing(doomed_item)]),
                "candidate.holds",
            )],
            ..Transaction::default()
        }),
        Err("relation.unique_conflict")
    );
    assert_eq!(conflicting, base);
}

#[test]
fn intermediate_work_limits_cover_no_match_text_groups_and_proof_products() {
    const WIDTH: i64 = 32;
    const JOIN_ATTEMPTS: usize = 1_056;
    const MATCH_ALL_BINDINGS: usize = 1_024;
    let mut model = WorldModel::default();
    for relation in ["A", "B", "Numbers"] {
        model
            .relations
            .register(RelationSchema::new(relation, vec![int_column("value")]))
            .unwrap();
    }
    for relation in ["TextSource", "TextMirror"] {
        model
            .relations
            .register(RelationSchema::new(
                relation,
                vec![ColumnSchema::new("value", ValueKind::Text)],
            ))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: (0..WIDTH)
                .map(|value| insert(pending_key("A", vec![int(value)]), "a"))
                .chain(
                    (WIDTH..(2 * WIDTH))
                        .map(|value| insert(pending_key("B", vec![int(value)]), "b")),
                )
                .chain(
                    (0..WIDTH)
                        .map(|value| insert(pending_key("Numbers", vec![int(value)]), "number")),
                )
                .chain([
                    PendingOperation::Insert(
                        pending_key("TextSource", vec![PendingValue::Text("x".repeat(8_192))]),
                        OperationMetadata::cause("text"),
                    ),
                    PendingOperation::Insert(
                        pending_key("TextMirror", vec![PendingValue::Text("x".repeat(8_192))]),
                        OperationMetadata::cause("text.mirror"),
                    ),
                ])
                .collect(),
            ..Transaction::default()
        })
        .unwrap();
    let schemas = BTreeMap::from([
        (
            "NoMatch".to_owned(),
            RelationSchema::new("NoMatch", vec![int_column("value")]),
        ),
        (
            "Pairs".to_owned(),
            RelationSchema::new("Pairs", vec![int_column("left"), int_column("right")]),
        ),
        (
            "TextOut".to_owned(),
            RelationSchema::new("TextOut", vec![ColumnSchema::new("value", ValueKind::Text)]),
        ),
        (
            "CountByValue".to_owned(),
            RelationSchema::new(
                "CountByValue",
                vec![int_column("value"), count_column("count")],
            ),
        ),
    ]);
    let no_match = RulePlan {
        id: "derive.NoMatch".to_owned(),
        head_relation: "NoMatch".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![
            Atom::new("A", vec![Term::var("value")]),
            Atom::new("B", vec![Term::var("value")]),
        ],
        predicates: Vec::new(),
        aggregate: None,
    };
    let mut exact = DerivationLimits::generous();
    exact.max_join_attempts = JOIN_ATTEMPTS;
    exact.max_rows_scanned = JOIN_ATTEMPTS;
    exact.max_intermediate_states = WIDTH as usize + 1;
    assert!(derive_all(
        &model.relations,
        &schemas,
        std::slice::from_ref(&no_match),
        exact,
    )
    .unwrap()
    .is_empty());
    for (mut limits, error) in [
        (
            {
                let mut value = exact;
                value.max_join_attempts = JOIN_ATTEMPTS - 1;
                value
            },
            "derivation.join_attempt_limit",
        ),
        (
            {
                let mut value = exact;
                value.max_rows_scanned = JOIN_ATTEMPTS - 1;
                value
            },
            "derivation.rows_scanned_limit",
        ),
        (
            {
                let mut value = exact;
                value.max_intermediate_states = WIDTH as usize;
                value
            },
            "derivation.intermediate_state_limit",
        ),
    ] {
        // Isolate overlapping bounds so the asserted meter wins canonically.
        if error == "derivation.join_attempt_limit" {
            limits.max_rows_scanned = usize::MAX;
        } else if error == "derivation.rows_scanned_limit" {
            limits.max_join_attempts = usize::MAX;
        }
        assert_eq!(
            derive_all(
                &model.relations,
                &schemas,
                std::slice::from_ref(&no_match),
                limits,
            ),
            Err(error)
        );
    }

    let match_all = RulePlan {
        id: "derive.Pairs".to_owned(),
        head_relation: "Pairs".to_owned(),
        head: vec![Term::var("left"), Term::var("right")],
        atoms: vec![
            Atom::new("A", vec![Term::var("left")]),
            Atom::new("B", vec![Term::var("right")]),
        ],
        predicates: Vec::new(),
        aggregate: None,
    };
    let mut match_limit = DerivationLimits::generous();
    match_limit.max_bindings = MATCH_ALL_BINDINGS;
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            std::slice::from_ref(&match_all),
            match_limit,
        )
        .unwrap()
        .len(),
        MATCH_ALL_BINDINGS
    );
    match_limit.max_bindings = MATCH_ALL_BINDINGS - 1;
    assert_eq!(
        derive_all(&model.relations, &schemas, &[match_all], match_limit),
        Err("derivation.binding_limit")
    );

    let text_rule = RulePlan {
        id: "derive.TextOut".to_owned(),
        head_relation: "TextOut".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![
            Atom::new("TextSource", vec![Term::var("value")]),
            Atom::new("TextMirror", vec![Term::var("value")]),
        ],
        predicates: Vec::new(),
        aggregate: None,
    };
    let mut low = 0_usize;
    let mut high = DerivationLimits::generous().max_intermediate_bytes;
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        let mut limits = DerivationLimits::generous();
        limits.max_intermediate_bytes = middle;
        if derive_all(
            &model.relations,
            &schemas,
            std::slice::from_ref(&text_rule),
            limits,
        )
        .is_ok()
        {
            high = middle;
        } else {
            low = middle;
        }
    }
    let mut text_limit = DerivationLimits::generous();
    text_limit.max_intermediate_bytes = high;
    assert!(derive_all(
        &model.relations,
        &schemas,
        std::slice::from_ref(&text_rule),
        text_limit,
    )
    .is_ok());
    text_limit.max_intermediate_bytes = high - 1;
    assert_eq!(
        derive_all(&model.relations, &schemas, &[text_rule], text_limit),
        Err("derivation.intermediate_byte_limit")
    );

    let grouped = RulePlan {
        id: "derive.CountByValue".to_owned(),
        head_relation: "CountByValue".to_owned(),
        head: vec![Term::var("value"), Term::var("count")],
        atoms: vec![Atom::new("Numbers", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: Some(AggregateSpec {
            kind: AggregateKind::Count,
            input: None,
            output: "count".to_owned(),
            group_by: vec!["value".to_owned()],
        }),
    };
    let mut group_limit = DerivationLimits::generous();
    group_limit.max_aggregate_group_entries = WIDTH as usize;
    assert!(derive_all(
        &model.relations,
        &schemas,
        std::slice::from_ref(&grouped),
        group_limit,
    )
    .is_ok());
    group_limit.max_aggregate_group_entries = WIDTH as usize - 1;
    assert_eq!(
        derive_all(&model.relations, &schemas, &[grouped], group_limit),
        Err("derivation.aggregate_group_limit")
    );

    let mut incremental_model = WorldModel::default();
    for relation in ["A", "B"] {
        incremental_model
            .relations
            .register(RelationSchema::new(relation, vec![int_column("value")]))
            .unwrap();
    }
    incremental_model
        .apply_transaction(Transaction {
            operations: (0..WIDTH)
                .map(|value| insert(pending_key("A", vec![int(value)]), "a"))
                .collect(),
            ..Transaction::default()
        })
        .unwrap();
    let mut work_limit = DerivationLimits::generous();
    work_limit.max_join_attempts = 64;
    let mut incremental = DependencyMaintainer::new(
        incremental_model.clone(),
        schemas.clone(),
        vec![no_match.clone()],
        work_limit,
    )
    .unwrap();
    let overflow = Transaction {
        operations: vec![
            insert(pending_key("B", vec![int(10)]), "b10"),
            insert(pending_key("B", vec![int(11)]), "b11"),
        ],
        ..Transaction::default()
    };
    let mut full_candidate = incremental_model.clone();
    full_candidate.apply_transaction(overflow.clone()).unwrap();
    let full_error = derive_all(&full_candidate.relations, &schemas, &[no_match], work_limit);
    assert_eq!(full_error, Err("derivation.join_attempt_limit"));
    assert_eq!(incremental.apply(overflow), full_error.map(|_| ()));
    assert_eq!(incremental.model, incremental_model);
}

#[test]
fn canonical_rule_plans_make_resource_failures_permutation_invariant() {
    let mut model = WorldModel::default();
    for relation in ["One", "TwoLeft", "TwoRight"] {
        model
            .relations
            .register(RelationSchema::new(relation, vec![int_column("value")]))
            .unwrap();
    }
    model
        .apply_transaction(Transaction {
            operations: vec![
                insert(pending_key("One", vec![int(1)]), "one"),
                insert(pending_key("TwoLeft", vec![int(2)]), "left"),
                insert(pending_key("TwoRight", vec![int(2)]), "right"),
            ],
            ..Transaction::default()
        })
        .unwrap();
    let schemas = BTreeMap::from([(
        "Out".to_owned(),
        RelationSchema::new("Out", vec![int_column("value")]),
    )]);
    let one = RulePlan {
        id: "a.one".to_owned(),
        head_relation: "Out".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![Atom::new("One", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    let two = RulePlan {
        id: "b.two".to_owned(),
        head_relation: "Out".to_owned(),
        head: vec![Term::var("value")],
        atoms: vec![
            Atom::new("TwoLeft", vec![Term::var("value")]),
            Atom::new("TwoRight", vec![Term::var("value")]),
        ],
        predicates: Vec::new(),
        aggregate: None,
    };
    let mut limits = DerivationLimits::generous();
    limits.max_facts = 1;
    limits.max_support_nodes = 1;
    let forward = derive_all(
        &model.relations,
        &schemas,
        &[one.clone(), two.clone()],
        limits,
    );
    let reverse = derive_all(
        &model.relations,
        &schemas,
        &[two.clone(), one.clone()],
        limits,
    );
    assert_eq!(forward, Err("derivation.fact_limit"));
    assert_eq!(reverse, forward);

    let mut two_reversed = two.clone();
    two_reversed.atoms.reverse();
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &[one.clone(), two],
            DerivationLimits::generous(),
        ),
        derive_all(
            &model.relations,
            &schemas,
            &[two_reversed, one.clone()],
            DerivationLimits::generous(),
        )
    );
    let mut duplicate = one.clone();
    duplicate.head = vec![Term::Constant(FactValue::Int(9))];
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &[one.clone(), duplicate],
            DerivationLimits::generous(),
        ),
        Err("derivation.duplicate_rule_id")
    );
    let mut empty_id = one;
    empty_id.id.clear();
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &[empty_id],
            DerivationLimits::generous(),
        ),
        Err("derivation.empty_rule_id")
    );
    let mut unqualified = RulePlan {
        id: "a.one".to_owned(),
        head_relation: "Out".to_owned(),
        head: vec![Term::Constant(FactValue::Int(1))],
        atoms: vec![Atom::new("One", vec![Term::var("value")])],
        predicates: Vec::new(),
        aggregate: None,
    };
    unqualified.id = "one".to_owned();
    assert_eq!(
        derive_all(
            &model.relations,
            &schemas,
            &[unqualified],
            DerivationLimits::generous(),
        ),
        Err("derivation.unqualified_rule_id")
    );

    let collision = BTreeMap::from([(
        "One".to_owned(),
        RelationSchema::new("One", vec![int_column("value")]),
    )]);
    assert_eq!(
        derive_all(
            &model.relations,
            &collision,
            &[],
            DerivationLimits::generous(),
        ),
        Err("derivation.relation_namespace_collision")
    );
}
