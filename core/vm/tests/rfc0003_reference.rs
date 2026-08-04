//! Executable semantic oracle for RFC-0003.
//!
//! This deliberately contains no parser, bytecode, VM, GC, or ECS code. It is
//! a generic typed relation model. Full recomputation defines derivation
//! semantics; the dependency maintainer is differential-tested against it.

use std::collections::{BTreeMap, BTreeSet};

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
    on_delete: DeletePolicy,
}

impl ColumnSchema {
    fn new(name: &str, kind: ValueKind) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            on_delete: DeletePolicy::Restrict,
        }
    }

    fn cascade(mut self) -> Self {
        self.on_delete = DeletePolicy::Cascade;
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
        if self.symmetric {
            if self.columns.len() != 2 || self.columns[0].kind != self.columns[1].kind {
                return Err("relation.symmetric_shape");
            }
            if !self.unique.is_empty() {
                return Err("relation.symmetric_unique_forbidden");
            }
        }
        for unique in &self.unique {
            if unique.columns.is_empty()
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
        if self.schemas.insert(schema.name.clone(), schema).is_some() {
            return Err("relation.duplicate_schema");
        }
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

    fn apply_operations(&mut self, operations: Vec<RelationOperation>) -> OracleResult<()> {
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
                    });
                }
            }
        }
        for (key, action) in &actions {
            if let Some(metadata) = &action.insert {
                if !candidate.contains_key(key) {
                    let id = self.next_assertion_id;
                    self.next_assertion_id = self
                        .next_assertion_id
                        .checked_add(1)
                        .ok_or("relation.assertion_id_overflow")?;
                    candidate.insert(
                        key.clone(),
                        FactAssertion {
                            id,
                            key: key.clone(),
                            causes: metadata.causes.clone(),
                            required_capabilities: metadata.required_capabilities.clone(),
                        },
                    );
                    changes.push(FactChange {
                        kind: ChangeKind::Insert,
                        key: key.clone(),
                        causes: metadata.causes.clone(),
                    });
                }
            }
        }

        self.validate_unique(&candidate)?;
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

    fn remove_cascade(&mut self, key: &FactKey, cause: &str) {
        if self.assertions.remove(key).is_some() {
            self.last_changes.push(FactChange {
                kind: ChangeKind::Cascade,
                key: key.clone(),
                causes: BTreeSet::from([cause.to_owned()]),
            });
        }
    }

    fn logical_rows(&self, relation: &str) -> OracleResult<Vec<LogicalRow>> {
        let schema = self.schemas.get(relation).ok_or("relation.unknown")?;
        let mut rows = Vec::new();
        for assertion in self
            .assertions
            .values()
            .filter(|assertion| assertion.key.relation == relation)
        {
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
    despawns: BTreeSet<EntityRef>,
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

impl WorldModel {
    fn apply_transaction(
        &mut self,
        transaction: Transaction,
    ) -> OracleResult<BTreeMap<u32, EntityRef>> {
        let mut candidate = self.clone();
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
        candidate.relations.apply_operations(operations)?;

        for entity in transaction.despawns {
            let mut cascade = BTreeSet::new();
            for fact in candidate.relations.assertions.keys() {
                let schema = &candidate.relations.schemas[&fact.relation];
                let mut referenced = false;
                let mut restricted = false;
                for (value, column) in fact.tuple.iter().zip(&schema.columns) {
                    if *value == FactValue::Entity(entity) {
                        referenced = true;
                        restricted |= column.on_delete == DeletePolicy::Restrict;
                    }
                }
                if referenced {
                    if restricted {
                        return Err("entity.delete_restricted");
                    }
                    cascade.insert(fact.clone());
                }
            }
            for fact in cascade {
                candidate
                    .relations
                    .remove_cascade(&fact, "entity.despawn.cascade");
            }
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
        proof_ids: BTreeSet<String>,
        capability_alternatives: CapabilityFormula,
    },
}

type CapabilityFormula = BTreeSet<BTreeSet<String>>;

fn minimize_capability_formula(formula: CapabilityFormula) -> CapabilityFormula {
    formula
        .iter()
        .filter(|candidate| {
            !formula
                .iter()
                .any(|other| other != *candidate && other.is_subset(candidate))
        })
        .cloned()
        .collect()
}

fn conjoin_support_capabilities(supports: &BTreeSet<SupportRef>) -> CapabilityFormula {
    let mut formula = BTreeSet::from([BTreeSet::new()]);
    for support in supports {
        let alternatives = support.capability_formula();
        let mut next = CapabilityFormula::new();
        for left in &formula {
            for right in &alternatives {
                next.insert(left.union(right).cloned().collect());
            }
        }
        formula = minimize_capability_formula(next);
    }
    formula
}

impl SupportRef {
    fn capability_formula(&self) -> CapabilityFormula {
        match self {
            Self::Authoritative {
                required_capabilities,
                ..
            } => BTreeSet::from([required_capabilities.clone()]),
            Self::Derived {
                capability_alternatives,
                ..
            } => capability_alternatives.clone(),
        }
    }

    fn key(&self) -> &FactKey {
        match self {
            Self::Authoritative { key, .. } | Self::Derived { key, .. } => key,
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
    capability_alternatives: CapabilityFormula,
}

impl ProofAlternative {
    fn identity(&self) -> String {
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
                SupportRef::Derived { proof_ids, .. } => {
                    bytes.push(b'D');
                    write_u64(&mut bytes, proof_ids.len() as u64);
                    for proof in proof_ids {
                        write_text(&mut bytes, proof);
                    }
                }
            }
        }
        hex(&bytes)
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
    fn validate_range_restriction(&self) -> OracleResult<()> {
        let bound = self
            .atoms
            .iter()
            .flat_map(|atom| &atom.terms)
            .filter_map(|term| match term {
                Term::Variable(name) => Some(name.clone()),
                Term::Constant(_) => None,
            })
            .collect::<BTreeSet<_>>();
        let aggregate_output = self.aggregate.as_ref().map(|aggregate| &aggregate.output);
        let mut required = BTreeSet::new();
        for term in &self.head {
            if let Term::Variable(name) = term {
                if Some(name) != aggregate_output {
                    required.insert(name.clone());
                }
            }
        }
        for predicate in &self.predicates {
            match predicate {
                Predicate::Greater(left, right) => {
                    required.insert(left.clone());
                    required.insert(right.clone());
                }
            }
        }
        if let Some(aggregate) = &self.aggregate {
            required.extend(aggregate.group_by.iter().cloned());
            required.extend(aggregate.input.iter().cloned());
        }
        if required.is_subset(&bound) {
            Ok(())
        } else {
            Err("derivation.unbound_variable")
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct BindingState {
    bindings: BTreeMap<String, FactValue>,
    supports: BTreeSet<SupportRef>,
}

#[derive(Clone, Copy, Debug)]
struct EvaluationLimits {
    max_bindings: usize,
}

fn derived_logical_rows(
    relation: &str,
    schemas: &BTreeMap<String, RelationSchema>,
    derived: &DerivationResult,
) -> OracleResult<Vec<LogicalRow>> {
    let schema = schemas.get(relation).ok_or("relation.unknown")?;
    let mut rows = Vec::new();
    for (key, proofs) in derived.iter().filter(|(key, _)| key.relation == relation) {
        let capability_alternatives = proofs
            .iter()
            .flat_map(|proof| proof.capability_alternatives.iter().cloned())
            .collect::<CapabilityFormula>();
        let support = SupportRef::Derived {
            key: key.clone(),
            proof_ids: proofs.iter().map(ProofAlternative::identity).collect(),
            capability_alternatives: minimize_capability_formula(capability_alternatives),
        };
        rows.push(LogicalRow {
            tuple: key.tuple.clone(),
            support: support.clone(),
        });
        if schema.symmetric && key.tuple[0] != key.tuple[1] {
            rows.push(LogicalRow {
                tuple: vec![key.tuple[1].clone(), key.tuple[0].clone()],
                support,
            });
        }
    }
    rows.sort();
    Ok(rows)
}

fn unify(state: &BindingState, terms: &[Term], row: &LogicalRow) -> Option<BindingState> {
    if terms.len() != row.tuple.len() {
        return None;
    }
    let mut next = state.clone();
    for (term, value) in terms.iter().zip(&row.tuple) {
        match term {
            Term::Constant(expected) if expected != value => return None,
            Term::Constant(_) => {}
            Term::Variable(name) => match next.bindings.get(name) {
                Some(existing) if existing != value => return None,
                Some(_) => {}
                None => {
                    next.bindings.insert(name.clone(), value.clone());
                }
            },
        }
    }
    next.supports.insert(row.support.clone());
    Some(next)
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

fn evaluate_rule(
    rule: &RulePlan,
    store: &RelationStore,
    schemas: &BTreeMap<String, RelationSchema>,
    derived: &DerivationResult,
    limits: EvaluationLimits,
) -> OracleResult<DerivationResult> {
    rule.validate_range_restriction()?;
    let mut states = BTreeSet::from([BindingState {
        bindings: BTreeMap::new(),
        supports: BTreeSet::new(),
    }]);
    for atom in &rule.atoms {
        let rows = if store.schemas.contains_key(&atom.relation) {
            store.logical_rows(&atom.relation)?
        } else {
            derived_logical_rows(&atom.relation, schemas, derived)?
        };
        let mut next = BTreeSet::new();
        for state in &states {
            for row in &rows {
                if let Some(joined) = unify(state, &atom.terms, row) {
                    if next.len() >= limits.max_bindings {
                        return Err("derivation.binding_limit");
                    }
                    next.insert(joined);
                }
            }
        }
        states = next;
    }
    states.retain(|state| {
        rule.predicates
            .iter()
            .all(|predicate| predicate_matches(predicate, &state.bindings))
    });

    let mut result = DerivationResult::new();
    if let Some(aggregate) = &rule.aggregate {
        let mut groups = BTreeMap::<Vec<FactValue>, Vec<BindingState>>::new();
        for state in states {
            let group = aggregate
                .group_by
                .iter()
                .map(|name| {
                    state
                        .bindings
                        .get(name)
                        .cloned()
                        .ok_or("derivation.unbound_group")
                })
                .collect::<OracleResult<Vec<_>>>()?;
            groups.entry(group).or_default().push(state);
        }
        for (group, states) in groups {
            let values = match &aggregate.input {
                Some(input) => states
                    .iter()
                    .map(|state| {
                        state
                            .bindings
                            .get(input)
                            .cloned()
                            .ok_or("derivation.unbound_aggregate")
                    })
                    .collect::<OracleResult<Vec<_>>>()?,
                None => vec![FactValue::Count(1); states.len()],
            };
            let Some(value) = aggregate_values(aggregate.kind, &values)? else {
                continue;
            };
            let mut bindings = states[0].bindings.clone();
            bindings.insert(aggregate.output.clone(), value);
            let supports = states
                .iter()
                .flat_map(|state| state.supports.iter().cloned())
                .collect::<BTreeSet<_>>();
            let capability_alternatives = conjoin_support_capabilities(&supports);
            let key = build_head(rule, schemas, &bindings)?;
            result.entry(key).or_default().insert(ProofAlternative {
                rule: rule.id.clone(),
                bindings,
                supports,
                aggregate_group: Some(group),
                capability_alternatives,
            });
        }
    } else {
        for state in states {
            let key = build_head(rule, schemas, &state.bindings)?;
            let capability_alternatives = conjoin_support_capabilities(&state.supports);
            result.entry(key).or_default().insert(ProofAlternative {
                rule: rule.id.clone(),
                bindings: state.bindings,
                supports: state.supports,
                aggregate_group: None,
                capability_alternatives,
            });
        }
    }
    Ok(result)
}

fn derive_all(
    store: &RelationStore,
    schemas: &BTreeMap<String, RelationSchema>,
    rules: &[RulePlan],
    limits: EvaluationLimits,
) -> OracleResult<DerivationResult> {
    let heads = rules
        .iter()
        .map(|rule| rule.head_relation.clone())
        .collect::<BTreeSet<_>>();
    let mut pending = heads.clone();
    let mut completed = BTreeSet::new();
    let mut result = DerivationResult::new();
    while !pending.is_empty() {
        let ready = pending.iter().find(|head| {
            rules
                .iter()
                .filter(|rule| rule.head_relation.as_str() == head.as_str())
                .flat_map(|rule| &rule.atoms)
                .filter(|atom| heads.contains(&atom.relation))
                .all(|atom| completed.contains(&atom.relation))
        });
        let Some(head) = ready.cloned() else {
            return Err("derivation.cycle");
        };
        for rule in rules.iter().filter(|rule| rule.head_relation == head) {
            let produced = evaluate_rule(rule, store, schemas, &result, limits)?;
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
    limits: EvaluationLimits,
}

impl DependencyMaintainer {
    fn new(
        model: WorldModel,
        derived_schemas: BTreeMap<String, RelationSchema>,
        rules: Vec<RulePlan>,
        limits: EvaluationLimits,
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
        let before = self
            .model
            .relations
            .assertions
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        self.model.apply_transaction(transaction)?;
        let after = self
            .model
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
            &self.model.relations,
            &self.derived_schemas,
            &self.rules,
            self.limits,
        )?;
        self.derived
            .retain(|fact, _| !affected.contains(&fact.relation));
        for (fact, proofs) in full
            .iter()
            .filter(|(fact, _)| affected.contains(&fact.relation))
        {
            self.derived.insert(fact.clone(), proofs.clone());
        }
        if self.derived != full {
            return Err("derivation.incremental_mismatch");
        }
        Ok(())
    }
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

fn operational_checkpoint_bytes(model: &WorldModel) -> Vec<u8> {
    let mut out = b"rfc0003.operational.v1".to_vec();
    write_u64(&mut out, model.entities.next_slot as u64);
    write_u64(&mut out, model.entities.live.len() as u64);
    for entity in &model.entities.live {
        out.extend_from_slice(&entity.slot.to_be_bytes());
        out.extend_from_slice(&entity.generation.to_be_bytes());
    }
    write_u64(&mut out, model.entities.free_slots.len() as u64);
    for slot in &model.entities.free_slots {
        out.extend_from_slice(&slot.to_be_bytes());
    }
    write_u64(&mut out, model.components.len() as u64);
    for ((entity, component), value) in &model.components {
        out.extend_from_slice(&entity.slot.to_be_bytes());
        out.extend_from_slice(&entity.generation.to_be_bytes());
        write_text(&mut out, component);
        encode_value(&mut out, value);
    }
    write_u64(&mut out, model.relations.assertions.len() as u64);
    for assertion in model.relations.assertions.values() {
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

fn derivation_checkpoint_bytes(derived: &DerivationResult) -> Vec<u8> {
    let mut out = b"rfc0003.derivation.v1".to_vec();
    write_u64(&mut out, derived.len() as u64);
    for (fact, proofs) in derived {
        encode_fact_key(&mut out, fact);
        write_u64(&mut out, proofs.len() as u64);
        for proof in proofs {
            write_text(&mut out, &proof.identity());
            write_u64(&mut out, proof.capability_alternatives.len() as u64);
            for alternative in &proof.capability_alternatives {
                write_u64(&mut out, alternative.len() as u64);
                for capability in alternative {
                    write_text(&mut out, capability);
                }
            }
        }
    }
    out
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

fn decode_semantic_relation_bytes(mut input: &[u8]) -> OracleResult<BTreeSet<FactKey>> {
    const DOMAIN: &[u8] = b"rfc0003.semantic.v1";
    if input.get(..DOMAIN.len()) != Some(DOMAIN) {
        return Err("wire.domain");
    }
    input = &input[DOMAIN.len()..];
    let count = usize::try_from(read_u64(&mut input)?).map_err(|_| "wire.length")?;
    let facts = (0..count)
        .map(|_| decode_fact_key_from(&mut input))
        .collect::<OracleResult<BTreeSet<_>>>()?;
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
            .filter(|proof| {
                proof
                    .capability_alternatives
                    .iter()
                    .any(|required| required.is_subset(capabilities))
            })
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
            despawns: BTreeSet::from([target]),
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
            despawns: BTreeSet::from([target]),
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
    let derived = derive_all(
        &model.relations,
        &schemas,
        &rules,
        EvaluationLimits { max_bindings: 64 },
    )
    .unwrap();
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
    let limits = EvaluationLimits { max_bindings: 128 };
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
    let limits = EvaluationLimits { max_bindings: 32 };
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
            EvaluationLimits { max_bindings: 1 },
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
        unsafe_rule.validate_range_restriction(),
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
            EvaluationLimits { max_bindings: 8 },
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
        EvaluationLimits { max_bindings: 128 },
    )
    .unwrap();
    let encumbered = FactKey::new("Encumbered", vec![FactValue::Entity(person)]);
    let encumbered_proof = derived[&encumbered].iter().next().unwrap();
    let total_support = encumbered_proof
        .supports
        .iter()
        .find(|support| support.key().relation == "TotalWeight")
        .unwrap();
    let total_proof_ids = match total_support {
        SupportRef::Derived { proof_ids, .. } => proof_ids,
        SupportRef::Authoritative { .. } => panic!("TotalWeight must be derived"),
    };
    let total = total_support.key();
    assert_eq!(
        total.tuple,
        vec![FactValue::Entity(person), FactValue::Int(13)]
    );
    assert_eq!(
        total_proof_ids,
        &derived[total]
            .iter()
            .map(ProofAlternative::identity)
            .collect()
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
    let derived = derive_all(
        &model.relations,
        &derived_schemas(),
        &rules,
        EvaluationLimits { max_bindings: 16 },
    )
    .unwrap();
    let public = render_visible(&derived, &BTreeSet::new());
    let marked = FactKey::new("Marked", vec![FactValue::Int(1)]);
    let mut without_hidden = derived.clone();
    without_hidden
        .get_mut(&marked)
        .unwrap()
        .retain(|proof| proof.capability_alternatives.contains(&BTreeSet::new()));
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
        decode_semantic_relation_bytes(&semantic_before).unwrap(),
        model.relations.assertions.keys().cloned().collect()
    );
    let schemas = derived_schemas();
    let rules = ownership_rules();
    let limits = EvaluationLimits { max_bindings: 128 };
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
