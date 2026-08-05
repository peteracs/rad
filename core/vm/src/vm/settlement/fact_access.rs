// RFC-0003 fact access exposed to RFC-0001/0002 causal code.
//
// The relation front end and the ordinary RAD compiler remain separate
// bounded inputs. This bridge therefore accepts a sealed relation identity
// plus an exact tuple, then validates the tuple against the installed runtime
// manifest before observing state or staging an authoritative resolver patch.

fn runtime_fact_value(
    snapshot: &WorldSnapshot,
    relation: &str,
    column: &crate::relation_frontend::RelationColumn,
    value: &Value,
) -> Result<crate::relation_runtime::FactValue, String> {
    use crate::relation_frontend::RelationType;
    use crate::relation_runtime::FactValue;

    match column.value_type {
        RelationType::Entity => value
            .as_entity_id()
            .and_then(|entity| snapshot.entity_ref(entity))
            .map(FactValue::Entity)
            .ok_or_else(|| {
                format!(
                    "relation value '{}.{}' expects a live entity",
                    relation, column.name
                )
            }),
        RelationType::Int => value.as_int().map(FactValue::Int).ok_or_else(|| {
            format!("relation value '{}.{}' expects int", relation, column.name)
        }),
        RelationType::Count => value
            .as_int()
            .and_then(|value| u64::try_from(value).ok())
            .map(FactValue::Count)
            .ok_or_else(|| {
                format!(
                    "relation value '{}.{}' expects a nonnegative count",
                    relation, column.name
                )
            }),
        RelationType::Text => value
            .as_str()
            .map(|value| FactValue::Text(value.to_string()))
            .ok_or_else(|| {
                format!(
                    "relation value '{}.{}' expects text",
                    relation, column.name
                )
            }),
    }
}

fn runtime_fact_values(
    snapshot: &WorldSnapshot,
    relation: &str,
    columns: &[crate::relation_frontend::RelationColumn],
    values: &crate::value::RadList,
) -> Result<Vec<crate::relation_runtime::FactValue>, String> {
    if values.len() != columns.len() {
        return Err(format!(
            "relation '{}' expects {} tuple values, got {}",
            relation,
            columns.len(),
            values.len()
        ));
    }
    values
        .iter()
        .zip(columns)
        .map(|(value, column)| runtime_fact_value(snapshot, relation, column, value))
        .collect()
}

fn runtime_fact_key(
    snapshot: &WorldSnapshot,
    relation: &str,
    values: &crate::value::RadList,
    authoritative_only: bool,
) -> Result<crate::relation_runtime::FactKey, String> {
    let manifest = snapshot
        .relation_state()
        .manifest()
        .ok_or_else(|| "relation manifest is not installed".to_string())?;
    let runtime_schema = if authoritative_only {
        manifest.authoritative_schema(relation).ok_or_else(|| {
            if manifest.schema(relation).is_some() {
                format!("relation operation targets derived relation '{relation}'")
            } else {
                format!("relation operation references unknown relation '{relation}'")
            }
        })?
    } else {
        manifest
            .schema(relation)
            .ok_or_else(|| format!("constraint fact references unknown relation '{relation}'"))?
    };
    let schema = runtime_schema.schema();
    let tuple = runtime_fact_values(snapshot, relation, &schema.columns, values)?;
    crate::relation_runtime::canonical_fact_key(
        schema,
        crate::relation_runtime::FactKey::new(relation, tuple),
    )
    .map_err(|error| error.to_string())
}

fn pending_fact_values(
    values: Vec<crate::relation_runtime::FactValue>,
) -> Vec<crate::relation_runtime::PendingRelationValue> {
    use crate::relation_runtime::{EntityOperand, FactValue, PendingRelationValue};

    values
        .into_iter()
        .map(|value| match value {
            FactValue::Entity(entity) => {
                PendingRelationValue::Entity(EntityOperand::Existing(entity))
            }
            FactValue::Int(value) => PendingRelationValue::Int(value),
            FactValue::Count(value) => PendingRelationValue::Count(value),
            FactValue::Text(value) => PendingRelationValue::Text(value),
        })
        .collect()
}

impl VM {
    pub(crate) fn bi_constraint_fact(
        &mut self,
        args: Vec<Value>,
        candidate: bool,
    ) -> Result<Value, String> {
        if args.len() != 2 {
            let name = if candidate {
                "candidate_fact"
            } else {
                "base_fact"
            };
            return Err(format!(
                "{name}() requires exactly (relation_identity, tuple)"
            ));
        }
        if self.sandbox_caps.is_some() {
            return Err(
                "sandbox: relation fact reads require a capability-aware relation grant".into(),
            );
        }
        let relation = args[0]
            .as_str()
            .ok_or_else(|| "constraint fact relation identity must be a string".to_string())?;
        let values = args[1]
            .as_list()
            .ok_or_else(|| "constraint fact tuple must be a list".to_string())?;
        let context = self
            .settlement
            .as_ref()
            .ok_or_else(|| "constraint fact read requires an active settlement".to_string())?;
        if context.active_constraint.is_none() {
            return Err("constraint fact read is only valid while a constraint runs".into());
        }
        let snapshot = if candidate {
            context
                .candidate
                .as_ref()
                .ok_or_else(|| "constraint candidate is not installed".to_string())?
        } else {
            &context.base
        };
        let key = runtime_fact_key(snapshot, relation, values, false)?;
        let schema = snapshot
            .relation_state()
            .manifest()
            .and_then(|manifest| manifest.schema(relation))
            .expect("fact key validation resolved schema")
            .schema();
        let exists = match schema.kind {
            crate::relation_frontend::RelationKind::Authoritative => snapshot
                .relation_state()
                .assertions()
                .contains_key(&key),
            crate::relation_frontend::RelationKind::Derived => {
                snapshot.derived_relation_state().facts().contains_key(&key)
            }
        };
        Ok(Value::from_bool(exists))
    }

    pub(crate) fn bi_resolver_fact_write(
        &mut self,
        args: Vec<Value>,
        builtin: Builtin,
    ) -> Result<Value, String> {
        use crate::relation_runtime::{
            OperationMetadata, PendingFactKey, PendingRelationOperation,
        };

        let (relation_index, tuple_index, expected) = match builtin {
            Builtin::InsertFact | Builtin::RemoveFact => (0, 1, 2),
            Builtin::ReplaceFactBy => (0, 3, 4),
            _ => return Err("internal error: non-write fact builtin dispatched here".into()),
        };
        if args.len() != expected {
            return Err(format!(
                "{}() requires {} arguments",
                builtin.name(),
                expected
            ));
        }
        let relation = args[relation_index]
            .as_str()
            .ok_or_else(|| format!("{}() relation identity must be a string", builtin.name()))?
            .to_string();
        let tuple = args[tuple_index]
            .as_list()
            .ok_or_else(|| format!("{}() tuple must be a list", builtin.name()))?;
        self.sandbox_check_relation_write(&relation)?;

        let operation = {
            let context = self
                .settlement
                .as_ref()
                .ok_or_else(|| "relation patches require an active settlement".to_string())?;
            if context.active_constraint.is_some() {
                return Err("constraints cannot stage authoritative relation writes".into());
            }
            if context.active.is_none() {
                return Err("relation patches are only valid inside a resolver".into());
            }
            let snapshot = &context.base;
            let key = runtime_fact_key(snapshot, &relation, tuple, true)?;
            let pending_tuple = pending_fact_values(key.tuple);
            match builtin {
                Builtin::InsertFact => PendingRelationOperation::Insert {
                    fact: PendingFactKey::new(&relation, pending_tuple),
                    metadata: OperationMetadata::default(),
                },
                Builtin::RemoveFact => PendingRelationOperation::Remove {
                    fact: PendingFactKey::new(&relation, pending_tuple),
                    metadata: OperationMetadata::default(),
                },
                Builtin::ReplaceFactBy => {
                    let constraint = args[1].as_str().ok_or_else(|| {
                        "replace_fact_by() unique constraint must be a string".to_string()
                    })?;
                    let selected = args[2].as_list().ok_or_else(|| {
                        "replace_fact_by() selected key must be a list".to_string()
                    })?;
                    let schema = snapshot
                        .relation_state()
                        .manifest()
                        .and_then(|manifest| manifest.authoritative_schema(&relation))
                        .expect("authoritative fact key validation resolved schema")
                        .schema();
                    let unique = schema
                        .unique
                        .iter()
                        .find(|unique| unique.name == constraint)
                        .ok_or_else(|| {
                            format!(
                                "replace_fact_by() references unknown unique constraint '{constraint}'"
                            )
                        })?;
                    let columns = unique
                        .columns
                        .iter()
                        .map(|name| {
                            schema
                                .columns
                                .iter()
                                .find(|column| column.name == *name)
                                .expect("sealed unique column exists")
                                .clone()
                        })
                        .collect::<Vec<_>>();
                    let selected = runtime_fact_values(snapshot, &relation, &columns, selected)?;
                    PendingRelationOperation::ReplaceBy {
                        relation: relation.clone(),
                        unique_constraint: constraint.to_string(),
                        selected_key: pending_fact_values(selected),
                        tuple: pending_tuple,
                        metadata: OperationMetadata::default(),
                    }
                }
                _ => unreachable!("write builtin checked above"),
            }
        };
        self.stage_relation_operation(operation)?;
        Ok(Value::NIL)
    }
}
