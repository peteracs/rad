//! Read-only semantic query surface for host adapters.
//!
//! The checker owns mutable analysis state. Editors need a stable projection,
//! not access to those implementation tables. This index deliberately clones
//! the bounded checked metadata after analysis so adapters cannot mutate or
//! retain borrows into a later checker pass.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::types::{
    ComponentType, EventType, StateMachineType, StructType, SumTypeDef, SystemType, Ty,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticIntent {
    pub fields: Vec<(String, Ty)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticLaw {
    pub params: Vec<Ty>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticConstraint {
    pub attached_component: String,
    pub watches: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct CheckerSemanticIndex {
    components: BTreeMap<String, ComponentType>,
    structs: BTreeMap<String, StructType>,
    intents: BTreeMap<String, SemanticIntent>,
    laws: BTreeMap<String, SemanticLaw>,
    constraints: BTreeMap<String, SemanticConstraint>,
    state_machines: BTreeMap<String, StateMachineType>,
    systems: BTreeMap<String, SystemType>,
    events: BTreeMap<String, EventType>,
    sum_types: BTreeMap<String, SumTypeDef>,
    functions: BTreeSet<String>,
    module_aliases: HashMap<String, HashMap<String, String>>,
    type_redirects: HashMap<String, String>,
}

macro_rules! lookup_and_iter {
    ($one:ident, $many:ident, $field:ident, $value:ty) => {
        pub fn $one(&self, name: &str) -> Option<&$value> {
            self.$field.get(name)
        }

        pub fn $many(&self) -> impl Iterator<Item = (&str, &$value)> {
            self.$field
                .iter()
                .map(|(name, value)| (name.as_str(), value))
        }
    };
}

impl CheckerSemanticIndex {
    lookup_and_iter!(component, components, components, ComponentType);
    lookup_and_iter!(structure, structures, structs, StructType);
    lookup_and_iter!(intent, intents, intents, SemanticIntent);
    lookup_and_iter!(law, laws, laws, SemanticLaw);
    lookup_and_iter!(constraint, constraints, constraints, SemanticConstraint);
    lookup_and_iter!(
        state_machine,
        state_machines,
        state_machines,
        StateMachineType
    );
    lookup_and_iter!(system, systems, systems, SystemType);
    lookup_and_iter!(event, events, events, EventType);
    lookup_and_iter!(sum_type, sum_types, sum_types, SumTypeDef);

    pub fn has_function(&self, name: &str) -> bool {
        self.functions.contains(name)
    }

    pub fn functions(&self) -> impl Iterator<Item = &str> {
        self.functions.iter().map(String::as_str)
    }

    pub fn canonical_name(&self, name: &str) -> String {
        crate::ast::resolve_canonical_name(
            name,
            Some(&self.module_aliases),
            &[],
            &self.type_redirects,
        )
    }
}

impl super::Checker {
    pub fn semantic_index(&self) -> CheckerSemanticIndex {
        CheckerSemanticIndex {
            components: self.components.clone().into_iter().collect(),
            structs: self.structs.clone().into_iter().collect(),
            intents: self
                .intents
                .iter()
                .map(|(name, intent)| {
                    (
                        name.clone(),
                        SemanticIntent {
                            fields: intent.fields.clone(),
                        },
                    )
                })
                .collect(),
            laws: self
                .laws
                .iter()
                .map(|(name, law)| {
                    (
                        name.clone(),
                        SemanticLaw {
                            params: law.params.clone(),
                        },
                    )
                })
                .collect(),
            constraints: self
                .constraints
                .iter()
                .map(|(name, constraint)| {
                    (
                        name.clone(),
                        SemanticConstraint {
                            attached_component: constraint.attached_component.clone(),
                            watches: constraint.watches.iter().cloned().collect(),
                        },
                    )
                })
                .collect(),
            state_machines: self.state_machines.clone().into_iter().collect(),
            systems: self.systems.clone().into_iter().collect(),
            events: self.events.clone().into_iter().collect(),
            sum_types: self.sum_types.clone().into_iter().collect(),
            functions: self.functions.keys().cloned().collect(),
            module_aliases: self.module_aliases.clone(),
            type_redirects: self.type_redirects.clone(),
        }
    }
}
