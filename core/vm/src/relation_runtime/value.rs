use std::collections::BTreeSet;
use std::fmt;

pub type RelationRuntimeResult<T> = Result<T, RelationRuntimeError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationRuntimeError {
    pub code: &'static str,
    pub detail: String,
}

impl RelationRuntimeError {
    pub(crate) fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for RelationRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for RelationRuntimeError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntityRef {
    pub slot: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EntityOperand {
    Existing(EntityRef),
    Candidate(u32),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FactValue {
    Entity(EntityRef),
    Int(i64),
    Count(u64),
    Text(String),
}

impl FactValue {
    pub(crate) fn relation_type(&self) -> crate::relation_frontend::RelationType {
        use crate::relation_frontend::RelationType;
        match self {
            Self::Entity(_) => RelationType::Entity,
            Self::Int(_) => RelationType::Int,
            Self::Count(_) => RelationType::Count,
            Self::Text(_) => RelationType::Text,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FactKey {
    pub relation: String,
    pub tuple: Vec<FactValue>,
}

impl FactKey {
    pub fn new(relation: impl Into<String>, tuple: Vec<FactValue>) -> Self {
        Self {
            relation: relation.into(),
            tuple,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OperationMetadata {
    pub causes: BTreeSet<String>,
    pub required_capabilities: BTreeSet<String>,
}

impl OperationMetadata {
    pub fn cause(cause: impl Into<String>) -> Self {
        Self {
            causes: BTreeSet::from([cause.into()]),
            required_capabilities: BTreeSet::new(),
        }
    }

    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.required_capabilities.insert(capability.into());
        self
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        self.causes.extend(other.causes.iter().cloned());
        self.required_capabilities
            .extend(other.required_capabilities.iter().cloned());
    }
}
