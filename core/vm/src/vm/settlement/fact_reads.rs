// Immutable RFC-0003 fact reads exposed to RFC-0002 constraints.
//
// The relation front end and the ordinary RAD compiler remain separate
// bounded inputs. This bridge therefore accepts a sealed relation identity
// plus an exact tuple, then validates the tuple against the installed runtime
// manifest before observing authoritative or derived state.

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
        let manifest = snapshot
            .relation_state()
            .manifest()
            .ok_or_else(|| "relation manifest is not installed".to_string())?;
        let runtime_schema = manifest
            .schema(relation)
            .ok_or_else(|| format!("constraint fact references unknown relation '{relation}'"))?;
        let schema = runtime_schema.schema();
        if values.len() != schema.columns.len() {
            return Err(format!(
                "constraint fact '{}' expects {} tuple values, got {}",
                relation,
                schema.columns.len(),
                values.len()
            ));
        }
        let tuple = values
            .iter()
            .zip(&schema.columns)
            .map(|(value, column)| {
                use crate::relation_frontend::RelationType;
                use crate::relation_runtime::FactValue;
                match column.value_type {
                    RelationType::Entity => value
                        .as_entity_id()
                        .and_then(|entity| snapshot.entity_ref(entity))
                        .map(FactValue::Entity)
                        .ok_or_else(|| {
                            format!(
                                "constraint fact '{}.{}' expects a live entity",
                                relation, column.name
                            )
                        }),
                    RelationType::Int => value.as_int().map(FactValue::Int).ok_or_else(|| {
                        format!(
                            "constraint fact '{}.{}' expects int",
                            relation, column.name
                        )
                    }),
                    RelationType::Count => value
                        .as_int()
                        .and_then(|value| u64::try_from(value).ok())
                        .map(FactValue::Count)
                        .ok_or_else(|| {
                            format!(
                                "constraint fact '{}.{}' expects a nonnegative count",
                                relation, column.name
                            )
                        }),
                    RelationType::Text => value
                        .as_str()
                        .map(|value| FactValue::Text(value.to_string()))
                        .ok_or_else(|| {
                            format!(
                                "constraint fact '{}.{}' expects text",
                                relation, column.name
                            )
                        }),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let key = crate::relation_runtime::canonical_fact_key(
            schema,
            crate::relation_runtime::FactKey::new(relation, tuple),
        )
        .map_err(|error| error.to_string())?;
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
}
