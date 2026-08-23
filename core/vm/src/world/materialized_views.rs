// Materialized views: installation, wire transport, incremental refresh,
// membership queries, and the negative explanations behind them.

impl World {
    #[inline]
    pub(crate) fn has_materialized_views(&self) -> bool {
        !self.materialized_views.is_empty()
    }

    pub(crate) fn install_materialized_view(
        &mut self,
        name: String,
        dependencies: Vec<String>,
        key: Option<(String, String)>,
        predicate: crate::materialized_view::MaterializedViewPredicate,
    ) {
        let state = MaterializedViewState {
            dependencies: dependencies.clone(),
            key,
            predicate,
            members: BTreeSet::new(),
            first_member: None,
            next_members: HashMap::new(),
            key_members: BTreeMap::new(),
            entity_keys: HashMap::new(),
            revision: 0,
            changes: Vec::new(),
            reasons: HashMap::new(),
            #[cfg(test)]
            evaluation_count: 0,
        };
        Arc::make_mut(&mut self.materialized_views).insert(name.clone(), state);
        let dependents = Arc::make_mut(&mut self.view_dependents);
        for dependency in dependencies {
            let names = dependents.entry(dependency).or_default();
            if !names.contains(&name) {
                names.push(name.clone());
                names.sort_unstable();
            }
        }
        let mut fields = Vec::new();
        if let Some((component, field)) = self
            .materialized_views
            .get(&name)
            .and_then(|view| view.key.clone())
        {
            fields.push((component, field));
        }
        if let Some(view) = self.materialized_views.get(&name) {
            fields.extend(
                view.predicate
                    .clauses
                    .iter()
                    .map(|clause| (clause.component.clone(), clause.field.clone())),
            );
        }
        fields.sort_unstable();
        fields.dedup();
        let field_dependents = Arc::make_mut(&mut self.view_field_dependents);
        for (component, field) in fields {
            let names = field_dependents
                .entry(component)
                .or_default()
                .entry(field)
                .or_default();
            if !names.contains(&name) {
                names.push(name.clone());
                names.sort_unstable();
            }
        }
        for entity in self.all_entity_ids() {
            self.refresh_materialized_view(&name, entity);
        }
    }

    /// Install only the receiving program's compiled view definitions, then
    /// derive their current membership from this world's authoritative rows.
    /// No definition or predicate is accepted from a wire payload.
    pub(crate) fn install_materialized_view_definitions_from(&mut self, other: &World) {
        let mut definitions = other
            .materialized_views
            .iter()
            .map(|(name, state)| {
                (
                    name.clone(),
                    state.dependencies.clone(),
                    state.key.clone(),
                    state.predicate.clone(),
                )
            })
            .collect::<Vec<_>>();
        definitions.sort_unstable_by(|left, right| left.0.cmp(&right.0));

        self.materialized_views = Arc::new(HashMap::new());
        self.view_dependents = Arc::new(HashMap::new());
        self.view_field_dependents = Arc::new(HashMap::new());
        for (name, dependencies, key, predicate) in definitions {
            self.install_materialized_view(name, dependencies, key, predicate);
        }
    }

    fn materialized_view_transport_states(&self) -> BTreeMap<String, MaterializedViewTransport> {
        self.materialized_views
            .iter()
            .map(|(name, state)| {
                (
                    name.clone(),
                    MaterializedViewTransport {
                        revision: state.revision,
                        members: state.members.clone(),
                        entity_keys: state
                            .entity_keys
                            .iter()
                            .map(|(entity, key)| (*entity, key.clone()))
                            .collect(),
                        changes: state.changes.clone(),
                        reasons: state
                            .reasons
                            .iter()
                            .map(|(entity, reason)| (*entity, reason.clone()))
                            .collect(),
                    },
                )
            })
            .collect()
    }

    /// Canonical JSON for derived view state. BTree-backed maps and sets keep
    /// the fork payload byte-identical across machines.
    pub(crate) fn materialized_view_transport_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.materialized_view_transport_states())
            .map_err(|error| format!("materialized view transport encoding failed: {error}"))
    }

    /// Restore exact revision/change history after proving that the payload's
    /// present-tense membership and keys are exactly those derived by this
    /// program from the decoded authoritative rows.
    pub(crate) fn restore_materialized_view_transport(
        &mut self,
        encoded: &serde_json::Value,
        operation: &str,
    ) -> Result<(), String> {
        let transported: BTreeMap<String, MaterializedViewTransport> =
            serde_json::from_value(encoded.clone()).map_err(|error| {
                format!("{operation}: malformed materialized view state: {error}")
            })?;
        let expected = self.materialized_view_transport_states();

        let transported_names = transported.keys().cloned().collect::<BTreeSet<_>>();
        let expected_names = expected.keys().cloned().collect::<BTreeSet<_>>();
        if transported_names != expected_names {
            let missing = expected_names
                .difference(&transported_names)
                .cloned()
                .collect::<Vec<_>>();
            let unknown = transported_names
                .difference(&expected_names)
                .cloned()
                .collect::<Vec<_>>();
            return Err(format!(
                "{operation}: materialized view definitions do not match the receiving program \
                 (missing [{}], unknown [{}])",
                missing.join(", "),
                unknown.join(", ")
            ));
        }

        for (name, state) in &transported {
            let derived = expected
                .get(name)
                .expect("equal view-name sets have the same entries");
            if state.members != derived.members {
                return Err(format!(
                    "{operation}: materialized view '{name}' membership disagrees with its \
                     compiled predicate over the decoded authoritative state"
                ));
            }
            if state.entity_keys != derived.entity_keys {
                return Err(format!(
                    "{operation}: materialized view '{name}' key index disagrees with its \
                     compiled key over the decoded authoritative state"
                ));
            }
            if state.revision != state.changes.len() as u64 {
                return Err(format!(
                    "{operation}: materialized view '{name}' revision {} does not match its {} \
                     recorded membership changes",
                    state.revision,
                    state.changes.len()
                ));
            }

            let mut replayed = BTreeSet::new();
            for (index, change) in state.changes.iter().enumerate() {
                let expected_revision = index as u64 + 1;
                if change.revision != expected_revision || change.reason.is_empty() {
                    return Err(format!(
                        "{operation}: materialized view '{name}' has an invalid change at \
                         position {expected_revision}"
                    ));
                }
                if change.entered {
                    replayed.insert(change.entity);
                } else {
                    replayed.remove(&change.entity);
                }
            }
            if replayed != state.members {
                return Err(format!(
                    "{operation}: materialized view '{name}' change history does not replay to \
                     its current membership"
                ));
            }
            if state
                .entity_keys
                .keys()
                .any(|entity| !state.members.contains(entity))
            {
                return Err(format!(
                    "{operation}: materialized view '{name}' contains a key for a non-member"
                ));
            }
            if state.reasons.values().any(String::is_empty) {
                return Err(format!(
                    "{operation}: materialized view '{name}' contains an empty explanation"
                ));
            }
        }

        let views = Arc::make_mut(&mut self.materialized_views);
        for (name, transported) in transported {
            let view = views
                .get_mut(&name)
                .expect("validated view name must remain installed");
            view.revision = transported.revision;
            view.changes = transported.changes;
            view.reasons = transported.reasons.into_iter().collect();
        }
        Ok(())
    }

    pub(crate) fn refresh_materialized_views_for(&mut self, component: &str, entity: u32) {
        self.refresh_materialized_views_for_components(std::iter::once(component), entity);
    }

    /// Re-evaluate each affected view once after one atomic component bundle.
    ///
    /// A spawn or transaction can publish several dependencies of the same
    /// view together. Refreshing after every component both exposed impossible
    /// intermediate membership and multiplied predicate work by the bundle
    /// width. Resolve the reverse dependency union first, then evaluate the
    /// final entity shape exactly once per view.
    pub(crate) fn refresh_materialized_views_for_components<'a>(
        &mut self,
        components: impl IntoIterator<Item = &'a str>,
        entity: u32,
    ) {
        let mut names = Vec::new();
        for component in components {
            if let Some(dependents) = self.view_dependents.get(component) {
                names.extend(dependents.iter().cloned());
            }
        }
        names.sort_unstable();
        names.dedup();
        for name in names {
            self.refresh_materialized_view(&name, entity);
        }
    }

    /// Refresh only views whose membership or key can change when one field
    /// changes. A field write cannot alter component presence, so dependencies
    /// that mention only the component's existence require no work here.
    pub(crate) fn refresh_materialized_views_for_field(
        &mut self,
        component: &str,
        field: &str,
        entity: u32,
    ) {
        let names = self
            .view_field_dependents
            .get(component)
            .and_then(|fields| fields.get(field))
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        for name in names {
            self.refresh_materialized_view(&name, entity);
        }
    }

    fn refresh_materialized_view(&mut self, name: &str, entity: u32) {
        #[cfg(test)]
        if let Some(view) = Arc::make_mut(&mut self.materialized_views).get_mut(name) {
            view.evaluation_count = view.evaluation_count.saturating_add(1);
        }
        let Some(definition) = self.materialized_views.get(name) else {
            return;
        };
        let dependencies = definition.dependencies.clone();
        let key_spec = definition.key.clone();
        let predicate = definition.predicate.clone();
        let missing = dependencies
            .iter()
            .filter(|dependency| self.get_component(entity, dependency).is_none())
            .cloned()
            .collect::<Vec<_>>();
        let key = if missing.is_empty() {
            key_spec.as_ref().and_then(|(component, field)| {
                self.get_component(entity, component)
                    .and_then(|data| Self::field_value(&data, field))
            })
        } else {
            None
        };
        let predicate_failure = if missing.is_empty() {
            predicate
                .clauses
                .iter()
                .find_map(|clause| self.materialized_view_clause_failure(entity, clause))
        } else {
            None
        };
        let eligible = missing.is_empty()
            && predicate_failure.is_none()
            && (key_spec.is_none() || key.is_some());
        let reason = if !missing.is_empty() {
            format!("missing {}", missing.join(", "))
        } else if let Some(reason) = predicate_failure {
            reason
        } else if let (Some((component, field)), None) = (key_spec.as_ref(), key.as_ref()) {
            format!("key {component}.{field} is absent or not indexable")
        } else {
            "all dependencies satisfied".to_string()
        };

        let views = Arc::make_mut(&mut self.materialized_views);
        let view = views.get_mut(name).expect("view definition disappeared");
        let was_member = view.members.contains(&entity);
        let old_key = view.entity_keys.get(&entity).cloned();
        if eligible {
            if let Some(key_value) = key.clone() {
                if let Some(displaced) = view.key_members.get(&key_value).copied() {
                    if displaced != entity {
                        view.remove_member(displaced);
                        view.entity_keys.remove(&displaced);
                        view.revision = view.revision.saturating_add(1);
                        let displaced_reason = format!(
                            "same-key replacement by entity {entity} at revision {}",
                            view.revision
                        );
                        view.reasons.insert(displaced, displaced_reason.clone());
                        view.changes.push(MaterializedViewChange {
                            revision: view.revision,
                            entity: displaced,
                            entered: false,
                            reason: displaced_reason,
                        });
                    }
                }
                if let Some(previous) = old_key.as_ref() {
                    if previous != &key_value {
                        view.key_members.remove(previous);
                    }
                }
                view.key_members.insert(key_value.clone(), entity);
                view.entity_keys.insert(entity, key_value);
            }
            view.insert_member(entity);
            view.reasons.insert(entity, reason.clone());
            if !was_member || old_key != key {
                view.revision = view.revision.saturating_add(1);
                view.changes.push(MaterializedViewChange {
                    revision: view.revision,
                    entity,
                    entered: true,
                    reason,
                });
            }
        } else {
            if let Some(previous) = old_key {
                view.key_members.remove(&previous);
                view.entity_keys.remove(&entity);
            }
            view.reasons.insert(entity, reason.clone());
            if view.remove_member(entity) {
                view.revision = view.revision.saturating_add(1);
                view.changes.push(MaterializedViewChange {
                    revision: view.revision,
                    entity,
                    entered: false,
                    reason,
                });
            }
        }
    }

    fn materialized_view_clause_failure(
        &self,
        entity: u32,
        clause: &crate::materialized_view::ViewPredicateClause,
    ) -> Option<String> {
        use crate::materialized_view::{ViewComparison, ViewPredicateValue};
        use std::cmp::Ordering;

        let actual = self
            .get_component(entity, &clause.component)
            .and_then(|component| {
                let index = component
                    .layout
                    .iter()
                    .position(|field| field == &clause.field)?;
                component.values.get(index).copied()
            });
        let Some(actual) = actual else {
            return Some(format!(
                "predicate {clause} failed: {}.{} is absent",
                clause.component, clause.field
            ));
        };

        let ordering = match &clause.expected {
            ViewPredicateValue::Int(expected) => actual.as_int().map(|value| value.cmp(expected)),
            ViewPredicateValue::Float(bits) => actual
                .as_float()
                .and_then(|value| value.partial_cmp(&f64::from_bits(*bits))),
            ViewPredicateValue::Str(expected) => actual.as_str().map(|value| value.cmp(expected)),
            ViewPredicateValue::Bool(expected) => actual.as_bool().map(|value| value.cmp(expected)),
        };
        let matches = ordering.is_some_and(|ordering| match clause.comparison {
            ViewComparison::Eq => ordering == Ordering::Equal,
            ViewComparison::Ne => ordering != Ordering::Equal,
            ViewComparison::Lt => ordering == Ordering::Less,
            ViewComparison::Le => ordering != Ordering::Greater,
            ViewComparison::Gt => ordering == Ordering::Greater,
            ViewComparison::Ge => ordering != Ordering::Less,
        });
        if matches {
            return None;
        }
        Some(format!("predicate {clause} failed (current {})", actual))
    }

    pub(crate) fn materialized_view_entities(&self, name: &str) -> Option<Vec<u32>> {
        Some(
            self.materialized_views
                .get(name)?
                .members
                .iter()
                .copied()
                .collect(),
        )
    }

    pub(crate) fn materialized_view_first_entity(&self, name: &str) -> Option<Option<u32>> {
        Some(self.materialized_views.get(name)?.first_member)
    }

    pub(crate) fn materialized_view_next_entity(
        &self,
        name: &str,
        entity: u32,
    ) -> Option<Option<u32>> {
        Some(
            self.materialized_views
                .get(name)?
                .next_members
                .get(&entity)
                .copied(),
        )
    }

    pub(crate) fn materialized_view_dependencies(&self, name: &str) -> Option<&[String]> {
        self.materialized_views
            .get(name)
            .map(|view| view.dependencies.as_slice())
    }

    pub(crate) fn materialized_view_lookup(&self, name: &str, key: &Value) -> Option<Option<u32>> {
        let view = self.materialized_views.get(name)?;
        let key = IndexValue::from_value(key)?;
        Some(view.key_members.get(&key).copied())
    }

    pub(crate) fn materialized_view_revision(&self, name: &str) -> Option<u64> {
        self.materialized_views.get(name).map(|view| view.revision)
    }

    #[cfg(test)]
    pub(crate) fn materialized_view_evaluation_count(&self, name: &str) -> Option<u64> {
        self.materialized_views
            .get(name)
            .map(|view| view.evaluation_count)
    }

    pub(crate) fn materialized_view_changes_since(
        &self,
        name: &str,
        revision: u64,
    ) -> Option<Vec<MaterializedViewChange>> {
        Some(
            self.materialized_views
                .get(name)?
                .changes
                .iter()
                .filter(|change| change.revision > revision)
                .cloned()
                .collect(),
        )
    }

    /// Operational membership explanation for embedders and CLI tooling.
    /// The explanation is maintained transactionally with the view itself.
    pub fn materialized_view_reason(&self, name: &str, entity: u32) -> Option<(bool, String)> {
        let view = self.materialized_views.get(name)?;
        Some((
            view.members.contains(&entity),
            view.reasons
                .get(&entity)
                .cloned()
                .unwrap_or_else(|| "entity has not touched a declared dependency".to_string()),
        ))
    }

    pub(crate) fn field_has_view_dependents(&self, component: &str, field: &str) -> bool {
        self.view_field_dependents
            .get(component)
            .and_then(|fields| fields.get(field))
            .is_some_and(|views| !views.is_empty())
    }
}
