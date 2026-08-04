// Full-recompute derivation, deterministic work metering, and the explicitly
// non-independent affected-relation projection harness.

// Runtime-only join state belongs to evaluation, not to the sealed rule plan.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct BindingState {
    bindings: BTreeMap<String, FactValue>,
    supports: BTreeSet<SupportRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DerivationLimits {
    raw_rule_input: RawRuleInputLimits,
    rule_plans: RulePlanLimits,
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
            raw_rule_input: RawRuleInputLimits::generous(),
            rule_plans: RulePlanLimits::generous(),
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
    if let Some(diagnostic) =
        select_rule_diagnostic(rules, limits.raw_rule_input, limits.rule_plans)
    {
        return Err(diagnostic.code.error());
    }
    let mut canonical_rules = rules
        .iter()
        .map(|rule| SealedRulePlan::new(rule, schemas))
        .collect::<Vec<_>>();
    canonical_rules.sort_by(|left, right| {
        (&left.plan.head_relation, &left.plan.id, left.digest).cmp(&(
            &right.plan.head_relation,
            &right.plan.id,
            right.digest,
        ))
    });
    for rule in &canonical_rules {
        rule.validate(store, schemas)?;
    }
    let mut meter = DerivationMeter::new(limits);
    let heads = canonical_rules
        .iter()
        .map(|rule| rule.plan.head_relation.clone())
        .collect::<BTreeSet<_>>();
    let mut pending = heads.clone();
    let mut completed = BTreeSet::new();
    let mut result = DerivationResult::new();
    while !pending.is_empty() {
        let ready = pending.iter().find(|head| {
            canonical_rules
                .iter()
                .filter(|rule| rule.plan.head_relation.as_str() == head.as_str())
                .flat_map(|rule| &rule.dependency_set)
                .filter(|dependency| heads.contains(*dependency))
                .all(|dependency| completed.contains(dependency))
        });
        let Some(head) = ready.cloned() else {
            return Err("derivation.cycle");
        };
        for rule in canonical_rules
            .iter()
            .filter(|rule| rule.plan.head_relation == head)
        {
            let produced = evaluate_rule(rule.plan, store, schemas, &result, &mut meter)?;
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
struct AffectedRelationProjectionHarness {
    model: WorldModel,
    derived_schemas: BTreeMap<String, RelationSchema>,
    rules: Vec<RulePlan>,
    derived: DerivationResult,
    limits: DerivationLimits,
}

impl AffectedRelationProjectionHarness {
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
        let mut projected = self.derived.clone();
        projected.retain(|fact, _| !affected.contains(&fact.relation));
        for (fact, proofs) in full
            .iter()
            .filter(|(fact, _)| affected.contains(&fact.relation))
        {
            projected.insert(fact.clone(), proofs.clone());
        }
        if projected != full {
            return Err("derivation.affected_projection_mismatch");
        }
        self.model = candidate_model;
        self.derived = projected;
        Ok(())
    }
}
