use std::collections::{BTreeMap, BTreeSet};

use super::{
    AuthoritativeRelationState, EntityOperand, EntityRef, FactChange, FactKey, FactValue,
    OperationMetadata, RelationRuntimeResult,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingSpawn {
    pub handle: u32,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingComponentWrite {
    pub entity: EntityOperand,
    pub component: crate::value::ComponentData,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingRelationValue {
    Entity(EntityOperand),
    Int(i64),
    Count(u64),
    Text(String),
}

impl PendingRelationValue {
    pub(crate) fn resolve(
        &self,
        handles: &BTreeMap<u32, EntityRef>,
    ) -> RelationRuntimeResult<FactValue> {
        Ok(match self {
            Self::Entity(EntityOperand::Existing(entity)) => FactValue::Entity(*entity),
            Self::Entity(EntityOperand::Candidate(handle)) => {
                FactValue::Entity(*handles.get(handle).ok_or_else(|| {
                    super::RelationRuntimeError::new(
                        "entity.unknown_candidate_handle",
                        handle.to_string(),
                    )
                })?)
            }
            Self::Int(value) => FactValue::Int(*value),
            Self::Count(value) => FactValue::Count(*value),
            Self::Text(value) => FactValue::Text(value.clone()),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingFactKey {
    pub relation: String,
    pub tuple: Vec<PendingRelationValue>,
}

impl PendingFactKey {
    pub fn new(relation: impl Into<String>, tuple: Vec<PendingRelationValue>) -> Self {
        Self {
            relation: relation.into(),
            tuple,
        }
    }

    pub(crate) fn resolve(
        &self,
        handles: &BTreeMap<u32, EntityRef>,
    ) -> RelationRuntimeResult<FactKey> {
        Ok(FactKey::new(
            &self.relation,
            self.tuple
                .iter()
                .map(|value| value.resolve(handles))
                .collect::<RelationRuntimeResult<Vec<_>>>()?,
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingRelationOperation {
    Insert {
        fact: PendingFactKey,
        metadata: OperationMetadata,
    },
    Remove {
        fact: PendingFactKey,
        metadata: OperationMetadata,
    },
    ReplaceBy {
        relation: String,
        unique_constraint: String,
        selected_key: Vec<PendingRelationValue>,
        tuple: Vec<PendingRelationValue>,
        metadata: OperationMetadata,
    },
}

impl PendingRelationOperation {
    #[cfg(test)]
    pub(crate) fn metadata(&self) -> &OperationMetadata {
        match self {
            Self::Insert { metadata, .. }
            | Self::Remove { metadata, .. }
            | Self::ReplaceBy { metadata, .. } => metadata,
        }
    }

    pub(crate) fn metadata_mut(&mut self) -> &mut OperationMetadata {
        match self {
            Self::Insert { metadata, .. }
            | Self::Remove { metadata, .. }
            | Self::ReplaceBy { metadata, .. } => metadata,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingDespawn {
    pub entity: EntityRef,
    pub metadata: OperationMetadata,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RelationTransaction {
    pub spawns: Vec<PendingSpawn>,
    pub component_writes: Vec<PendingComponentWrite>,
    pub operations: Vec<PendingRelationOperation>,
    pub despawns: Vec<PendingDespawn>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CandidateEntityState {
    pub live_after: BTreeSet<EntityRef>,
    pub candidate_handles: BTreeMap<u32, EntityRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationCandidate {
    pub(crate) state: AuthoritativeRelationState,
    pub(crate) changes: Vec<FactChange>,
}

impl RelationCandidate {
    pub fn state(&self) -> &AuthoritativeRelationState {
        &self.state
    }

    pub fn changes(&self) -> &[FactChange] {
        &self.changes
    }

    pub fn into_state(self) -> AuthoritativeRelationState {
        self.state
    }
}

impl RelationTransaction {
    /// Lower the already type-checked, ground front-end operations into one
    /// runtime patch. Symbolic entity names are resolved by the embedding
    /// world; no variable binding or derived evaluation occurs here.
    pub fn from_frontend(
        artifacts: &crate::relation_frontend::FrontendArtifacts,
        mut resolve_entity: impl FnMut(&str) -> Option<EntityRef>,
    ) -> RelationRuntimeResult<Self> {
        use crate::relation_frontend::{Literal, RawOperationValue, RelationOperationKind};

        fn value(
            value: &RawOperationValue,
            resolve: &mut impl FnMut(&str) -> Option<EntityRef>,
        ) -> RelationRuntimeResult<PendingRelationValue> {
            Ok(match value {
                RawOperationValue::EntitySymbol(symbol) => PendingRelationValue::Entity(
                    EntityOperand::Existing(resolve(symbol).ok_or_else(|| {
                        super::RelationRuntimeError::new("entity.unknown_symbol", symbol.clone())
                    })?),
                ),
                RawOperationValue::Literal(Literal::Int(value)) => {
                    PendingRelationValue::Int(*value)
                }
                RawOperationValue::Literal(Literal::Count(value)) => {
                    PendingRelationValue::Count(*value)
                }
                RawOperationValue::Literal(Literal::Text(value)) => {
                    PendingRelationValue::Text(value.clone())
                }
            })
        }

        let operations = artifacts
            .operations
            .iter()
            .map(|operation| {
                let tuple = operation
                    .tuple
                    .iter()
                    .map(|item| value(item, &mut resolve_entity))
                    .collect::<RelationRuntimeResult<Vec<_>>>()?;
                let metadata =
                    OperationMetadata::cause(format!("frontend.operation:{}", operation.owner));
                Ok(match &operation.kind {
                    RelationOperationKind::Insert => PendingRelationOperation::Insert {
                        fact: PendingFactKey::new(&operation.relation, tuple),
                        metadata,
                    },
                    RelationOperationKind::Remove => PendingRelationOperation::Remove {
                        fact: PendingFactKey::new(&operation.relation, tuple),
                        metadata,
                    },
                    RelationOperationKind::ReplaceBy { constraint, key } => {
                        PendingRelationOperation::ReplaceBy {
                            relation: operation.relation.clone(),
                            unique_constraint: constraint.clone(),
                            selected_key: key
                                .iter()
                                .map(|item| value(item, &mut resolve_entity))
                                .collect::<RelationRuntimeResult<Vec<_>>>()?,
                            tuple,
                            metadata,
                        }
                    }
                })
            })
            .collect::<RelationRuntimeResult<Vec<_>>>()?;
        Ok(Self {
            spawns: Vec::new(),
            component_writes: Vec::new(),
            operations,
            despawns: Vec::new(),
        })
    }
}
