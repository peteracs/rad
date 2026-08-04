use crate::relation_frontend::{OnDelete, RelationKind, RelationSchema};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{
    CandidateEntityState, EntityRef, FactKey, FactValue, OperationMetadata, PendingDespawn,
    PendingRelationOperation, RelationCandidate, RelationRuntimeError, RelationRuntimeManifest,
    RelationRuntimeResult, RelationTransaction,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactAssertion {
    pub assertion_id: u64,
    pub fact_key: FactKey,
    pub causes: BTreeSet<String>,
    pub required_capabilities: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactChangeKind {
    Insert,
    Remove,
    Cascade,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactChange {
    pub kind: FactChangeKind,
    pub fact_key: FactKey,
    pub causes: BTreeSet<String>,
    pub required_capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalFactRow {
    pub tuple: Vec<FactValue>,
    pub assertion_id: u64,
    pub causes: BTreeSet<String>,
    pub required_capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UniqueIndexKey {
    pub relation: String,
    pub constraint: String,
    pub values: Vec<FactValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeRelationState {
    manifest: Option<Arc<RelationRuntimeManifest>>,
    assertions: Arc<BTreeMap<FactKey, FactAssertion>>,
    unique_indexes: Arc<BTreeMap<UniqueIndexKey, FactKey>>,
    next_assertion_id: u64,
}

impl Default for AuthoritativeRelationState {
    fn default() -> Self {
        Self {
            manifest: None,
            assertions: Arc::new(BTreeMap::new()),
            unique_indexes: Arc::new(BTreeMap::new()),
            next_assertion_id: 0,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct NormalizedAction {
    insert: Option<OperationMetadata>,
    remove: Option<OperationMetadata>,
}

impl NormalizedAction {
    fn merge(slot: &mut Option<OperationMetadata>, metadata: &OperationMetadata) {
        match slot {
            Some(existing) => existing.merge(metadata),
            None => *slot = Some(metadata.clone()),
        }
    }
}

impl AuthoritativeRelationState {
    pub fn install_manifest(
        &mut self,
        manifest: Arc<RelationRuntimeManifest>,
        expected_frontend_digest: crate::relation_frontend::FrontendManifestDigest,
    ) -> RelationRuntimeResult<()> {
        if manifest.frontend_digest() != expected_frontend_digest {
            return Err(RelationRuntimeError::new(
                "relation.manifest_digest_mismatch",
                "runtime manifest was not derived from the installed front-end artifact",
            ));
        }
        if let Some(installed) = &self.manifest {
            if installed.digest() == manifest.digest() {
                return Ok(());
            }
            return Err(RelationRuntimeError::new(
                "relation.manifest_already_installed",
                "a different immutable relation manifest is already installed",
            ));
        }
        self.manifest = Some(manifest);
        Ok(())
    }

    pub fn manifest(&self) -> Option<&Arc<RelationRuntimeManifest>> {
        self.manifest.as_ref()
    }

    pub fn manifest_digest(&self) -> Option<[u8; 32]> {
        self.manifest.as_ref().map(|manifest| manifest.digest())
    }

    pub fn assertions(&self) -> &BTreeMap<FactKey, FactAssertion> {
        &self.assertions
    }

    pub fn unique_indexes(&self) -> &BTreeMap<UniqueIndexKey, FactKey> {
        &self.unique_indexes
    }

    pub fn next_assertion_id(&self) -> u64 {
        self.next_assertion_id
    }

    pub fn contains(&self, fact: FactKey) -> RelationRuntimeResult<bool> {
        let canonical = self.canonical_key(fact)?;
        Ok(self.assertions.contains_key(&canonical))
    }

    /// Logical scan view. Symmetric relations keep one physical assertion
    /// while exposing both orientations (one for a self-edge), and both rows
    /// retain the same assertion lifetime and provenance.
    pub fn logical_rows(&self, relation: &str) -> RelationRuntimeResult<Vec<LogicalFactRow>> {
        let manifest = self.manifest.as_ref().ok_or_else(|| {
            RelationRuntimeError::new("relation.manifest_not_installed", "no relation manifest")
        })?;
        let schema = manifest.authoritative_schema(relation).ok_or_else(|| {
            RelationRuntimeError::new(
                if manifest.schema(relation).is_some() {
                    "relation.operation_targets_derived"
                } else {
                    "relation.unknown"
                },
                relation,
            )
        })?;
        let mut rows = Vec::new();
        for assertion in self
            .assertions
            .values()
            .filter(|assertion| assertion.fact_key.relation == relation)
        {
            let row = LogicalFactRow {
                tuple: assertion.fact_key.tuple.clone(),
                assertion_id: assertion.assertion_id,
                causes: assertion.causes.clone(),
                required_capabilities: assertion.required_capabilities.clone(),
            };
            rows.push(row.clone());
            if schema.schema().symmetric && row.tuple[0] != row.tuple[1] {
                let mut reverse = row;
                reverse.tuple.swap(0, 1);
                rows.push(reverse);
            }
        }
        rows.sort_by(|left, right| {
            (&left.tuple, left.assertion_id).cmp(&(&right.tuple, right.assertion_id))
        });
        Ok(rows)
    }

    pub fn prepare_candidate(
        &self,
        transaction: &RelationTransaction,
        entities: &CandidateEntityState,
    ) -> RelationRuntimeResult<RelationCandidate> {
        let manifest = self.manifest.as_ref().ok_or_else(|| {
            RelationRuntimeError::new(
                "relation.manifest_not_installed",
                "install sealed front-end artifacts before applying relation patches",
            )
        })?;
        let base_keys = self.assertions.keys().cloned().collect::<BTreeSet<_>>();
        let mut assertions = self.assertions.as_ref().clone();
        let mut changes = Vec::new();
        self.apply_explicit_operations(
            manifest,
            &mut assertions,
            &mut changes,
            &transaction.operations,
            &entities.candidate_handles,
        )?;

        let despawns = normalize_despawns(&transaction.despawns);
        if despawns
            .keys()
            .any(|entity| entities.live_after.contains(entity))
        {
            return Err(RelationRuntimeError::new(
                "entity.despawn_still_live",
                "candidate entity inventory still contains a scheduled despawn",
            ));
        }
        let mut cascades = BTreeMap::<FactKey, OperationMetadata>::new();
        for fact in assertions.keys() {
            let schema = manifest
                .authoritative_schema(&fact.relation)
                .ok_or_else(|| {
                    RelationRuntimeError::new("relation.unknown", fact.relation.clone())
                })?;
            let mut referenced = false;
            let mut restricted = false;
            let mut metadata = OperationMetadata::default();
            for (value, column) in fact.tuple.iter().zip(&schema.schema().columns) {
                let FactValue::Entity(entity) = value else {
                    continue;
                };
                let Some(cause) = despawns.get(entity) else {
                    continue;
                };
                referenced = true;
                restricted |= column.on_delete.unwrap_or(OnDelete::Restrict) == OnDelete::Restrict;
                metadata.merge(cause);
            }
            if restricted {
                return Err(RelationRuntimeError::new(
                    "entity.delete_restricted",
                    format!("{} still references a despawned entity", fact.relation),
                ));
            }
            if referenced {
                cascades.insert(fact.clone(), metadata);
            }
        }
        for (fact, metadata) in cascades {
            if assertions.remove(&fact).is_some() {
                changes.push(change(FactChangeKind::Cascade, fact, &metadata));
            }
        }

        validate_live_entities(&assertions, &entities.live_after)?;
        let indexes = build_unique_indexes(manifest, &assertions)?;

        let new_keys = assertions
            .keys()
            .filter(|key| !base_keys.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        for (offset, key) in new_keys.iter().enumerate() {
            let offset = u64::try_from(offset).map_err(|_| {
                RelationRuntimeError::new("relation.assertion_id_overflow", "too many assertions")
            })?;
            assertions
                .get_mut(key)
                .expect("candidate key exists")
                .assertion_id = self.next_assertion_id.checked_add(offset).ok_or_else(|| {
                RelationRuntimeError::new(
                    "relation.assertion_id_overflow",
                    "assertion identity space exhausted",
                )
            })?;
        }
        let next_assertion_id = self
            .next_assertion_id
            .checked_add(u64::try_from(new_keys.len()).map_err(|_| {
                RelationRuntimeError::new("relation.assertion_id_overflow", "too many assertions")
            })?)
            .ok_or_else(|| {
                RelationRuntimeError::new(
                    "relation.assertion_id_overflow",
                    "assertion identity space exhausted",
                )
            })?;
        let final_keys = assertions.keys().cloned().collect::<BTreeSet<_>>();
        changes.retain(|entry| {
            base_keys.contains(&entry.fact_key) || final_keys.contains(&entry.fact_key)
        });
        Ok(RelationCandidate {
            state: Self {
                manifest: self.manifest.clone(),
                assertions: Arc::new(assertions),
                unique_indexes: Arc::new(indexes),
                next_assertion_id,
            },
            changes,
        })
    }

    pub fn adopt(&mut self, candidate: RelationCandidate) -> Vec<FactChange> {
        let RelationCandidate { state, changes } = candidate;
        *self = state;
        changes
    }

    fn canonical_key(&self, key: FactKey) -> RelationRuntimeResult<FactKey> {
        let manifest = self.manifest.as_ref().ok_or_else(|| {
            RelationRuntimeError::new("relation.manifest_not_installed", "no relation manifest")
        })?;
        let schema = manifest
            .authoritative_schema(&key.relation)
            .ok_or_else(|| {
                let code = if manifest.schema(&key.relation).is_some() {
                    "relation.operation_targets_derived"
                } else {
                    "relation.unknown"
                };
                RelationRuntimeError::new(code, key.relation.clone())
            })?;
        canonical_key(schema.schema(), key)
    }

    fn apply_explicit_operations(
        &self,
        manifest: &RelationRuntimeManifest,
        assertions: &mut BTreeMap<FactKey, FactAssertion>,
        changes: &mut Vec<FactChange>,
        operations: &[PendingRelationOperation],
        handles: &BTreeMap<u32, EntityRef>,
    ) -> RelationRuntimeResult<()> {
        let mut actions = BTreeMap::<FactKey, NormalizedAction>::new();
        let mut replacements =
            BTreeMap::<(String, String, Vec<FactValue>), (FactKey, OperationMetadata)>::new();
        for operation in operations {
            match operation {
                PendingRelationOperation::Insert { fact, metadata } => {
                    let key = self.canonical_key(fact.resolve(handles)?)?;
                    NormalizedAction::merge(&mut actions.entry(key).or_default().insert, metadata);
                }
                PendingRelationOperation::Remove { fact, metadata } => {
                    let key = self.canonical_key(fact.resolve(handles)?)?;
                    NormalizedAction::merge(&mut actions.entry(key).or_default().remove, metadata);
                }
                PendingRelationOperation::ReplaceBy {
                    relation,
                    unique_constraint,
                    selected_key,
                    tuple,
                    metadata,
                } => {
                    let schema = manifest.authoritative_schema(relation).ok_or_else(|| {
                        RelationRuntimeError::new(
                            if manifest.schema(relation).is_some() {
                                "relation.operation_targets_derived"
                            } else {
                                "relation.unknown"
                            },
                            relation.clone(),
                        )
                    })?;
                    let unique = unique_columns(schema.schema(), unique_constraint)?;
                    let selected = selected_key
                        .iter()
                        .map(|value| value.resolve(handles))
                        .collect::<RelationRuntimeResult<Vec<_>>>()?;
                    let target = canonical_key(
                        schema.schema(),
                        FactKey::new(
                            relation,
                            tuple
                                .iter()
                                .map(|value| value.resolve(handles))
                                .collect::<RelationRuntimeResult<Vec<_>>>()?,
                        ),
                    )?;
                    let target_unique = project(&target, &unique);
                    if selected != target_unique {
                        return Err(RelationRuntimeError::new(
                            "relation.replace_key_mismatch",
                            unique_constraint.clone(),
                        ));
                    }
                    let identity = (relation.clone(), unique_constraint.clone(), selected);
                    match replacements.get_mut(&identity) {
                        Some((previous, previous_metadata)) if previous == &target => {
                            previous_metadata.merge(metadata)
                        }
                        Some(_) => {
                            return Err(RelationRuntimeError::new(
                                "relation.replacement_conflict",
                                relation.clone(),
                            ));
                        }
                        None => {
                            replacements.insert(identity, (target, metadata.clone()));
                        }
                    }
                }
            }
        }
        for ((relation, constraint, selected), (target, metadata)) in replacements {
            let schema = manifest
                .authoritative_schema(&relation)
                .expect("replacement schema checked");
            let columns = unique_columns(schema.schema(), &constraint)?;
            let existing = assertions
                .keys()
                .find(|fact| fact.relation == relation && project(fact, &columns) == selected)
                .cloned();
            if existing.as_ref() == Some(&target) {
                continue;
            }
            if let Some(existing) = existing {
                NormalizedAction::merge(
                    &mut actions.entry(existing).or_default().remove,
                    &metadata,
                );
            }
            NormalizedAction::merge(&mut actions.entry(target).or_default().insert, &metadata);
        }
        if actions
            .values()
            .any(|action| action.insert.is_some() && action.remove.is_some())
        {
            return Err(RelationRuntimeError::new(
                "relation.operation_conflict",
                "one candidate both inserts and removes the same fact",
            ));
        }
        for (key, action) in &actions {
            if let Some(metadata) = &action.remove {
                if assertions.remove(key).is_some() {
                    changes.push(change(FactChangeKind::Remove, key.clone(), metadata));
                }
            }
        }
        for (key, action) in actions {
            if let Some(metadata) = action.insert {
                if !assertions.contains_key(&key) {
                    assertions.insert(
                        key.clone(),
                        FactAssertion {
                            assertion_id: 0,
                            fact_key: key.clone(),
                            causes: metadata.causes.clone(),
                            required_capabilities: metadata.required_capabilities.clone(),
                        },
                    );
                    changes.push(change(FactChangeKind::Insert, key, &metadata));
                }
            }
        }
        Ok(())
    }
}

fn normalize_despawns(values: &[PendingDespawn]) -> BTreeMap<EntityRef, OperationMetadata> {
    let mut out = BTreeMap::<EntityRef, OperationMetadata>::new();
    for despawn in values {
        out.entry(despawn.entity)
            .or_default()
            .merge(&despawn.metadata);
    }
    out
}

fn validate_live_entities(
    assertions: &BTreeMap<FactKey, FactAssertion>,
    live: &BTreeSet<EntityRef>,
) -> RelationRuntimeResult<()> {
    for fact in assertions.keys() {
        for value in &fact.tuple {
            if let FactValue::Entity(entity) = value {
                if !live.contains(entity) {
                    return Err(RelationRuntimeError::new(
                        "relation.dangling_entity",
                        format!(
                            "{} references {}:{}",
                            fact.relation, entity.slot, entity.generation
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn build_unique_indexes(
    manifest: &RelationRuntimeManifest,
    assertions: &BTreeMap<FactKey, FactAssertion>,
) -> RelationRuntimeResult<BTreeMap<UniqueIndexKey, FactKey>> {
    let mut indexes = BTreeMap::new();
    for fact in assertions.keys() {
        let schema = manifest
            .authoritative_schema(&fact.relation)
            .expect("stored facts have authoritative schemas");
        for unique in &schema.schema().unique {
            let columns = unique_columns(schema.schema(), &unique.name)?;
            let index = UniqueIndexKey {
                relation: fact.relation.clone(),
                constraint: unique.name.clone(),
                values: project(fact, &columns),
            };
            if indexes.insert(index.clone(), fact.clone()).is_some() {
                return Err(RelationRuntimeError::new(
                    "relation.unique_conflict",
                    format!("{}::{}", index.relation, index.constraint),
                ));
            }
        }
    }
    Ok(indexes)
}

fn canonical_key(schema: &RelationSchema, mut key: FactKey) -> RelationRuntimeResult<FactKey> {
    if schema.kind != RelationKind::Authoritative {
        return Err(RelationRuntimeError::new(
            "relation.operation_targets_derived",
            schema.identity.clone(),
        ));
    }
    if key.tuple.len() != schema.columns.len() {
        return Err(RelationRuntimeError::new(
            "relation.arity",
            schema.identity.clone(),
        ));
    }
    for (value, column) in key.tuple.iter().zip(&schema.columns) {
        if value.relation_type() != column.value_type {
            return Err(RelationRuntimeError::new(
                "relation.type_mismatch",
                format!("{}::{}", schema.identity, column.name),
            ));
        }
    }
    if schema.symmetric && key.tuple[1] < key.tuple[0] {
        key.tuple.swap(0, 1);
    }
    Ok(key)
}

fn unique_columns(schema: &RelationSchema, name: &str) -> RelationRuntimeResult<Vec<usize>> {
    let unique = schema
        .unique
        .iter()
        .find(|unique| unique.name == name)
        .ok_or_else(|| RelationRuntimeError::new("relation.unknown_unique", name.to_string()))?;
    unique
        .columns
        .iter()
        .map(|column| {
            schema
                .columns
                .iter()
                .position(|candidate| candidate.name == *column)
                .ok_or_else(|| {
                    RelationRuntimeError::new("relation.unknown_unique_column", column.clone())
                })
        })
        .collect()
}

fn project(fact: &FactKey, columns: &[usize]) -> Vec<FactValue> {
    columns
        .iter()
        .map(|column| fact.tuple[*column].clone())
        .collect()
}

fn change(kind: FactChangeKind, fact_key: FactKey, metadata: &OperationMetadata) -> FactChange {
    FactChange {
        kind,
        fact_key,
        causes: metadata.causes.clone(),
        required_capabilities: metadata.required_capabilities.clone(),
    }
}
