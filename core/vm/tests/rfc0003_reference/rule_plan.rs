// Proof identities, typed rule plans, and the immutable sealed-plan
// representation consumed by evaluation. Raw admission and canonical invalid
// diagnostics live in raw_validation.rs.

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

    fn canonical_len(&self) -> OracleResult<usize> {
        let mut length = b"rfc0003.rule-plan.v1".len();
        length = checked_len_add(length, encoded_text_len(&self.id)?)?;
        length = checked_len_add(length, encoded_text_len(&self.head_relation)?)?;
        length = checked_len_add(length, encoded_terms_len(&self.head)?)?;
        length = checked_len_add(length, 8)?;
        for atom in &self.atoms {
            length = checked_len_add(length, 8)?;
            length = checked_len_add(length, encoded_atom_len(atom)?)?;
        }
        length = checked_len_add(length, 8)?;
        for predicate in &self.predicates {
            length = checked_len_add(length, 8)?;
            length = checked_len_add(length, encoded_predicate_len(predicate)?)?;
        }
        length = checked_len_add(length, 1)?;
        if let Some(aggregate) = &self.aggregate {
            length = checked_len_add(length, 2)?;
            if let Some(input) = &aggregate.input {
                length = checked_len_add(length, encoded_text_len(input)?)?;
            }
            length = checked_len_add(length, encoded_text_len(&aggregate.output)?)?;
            length = checked_len_add(length, 8)?;
            for group in &aggregate.group_by {
                length = checked_len_add(length, encoded_text_len(group)?)?;
            }
        }
        Ok(length)
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

fn encoded_term_len(term: &Term) -> OracleResult<usize> {
    checked_len_add(
        1,
        match term {
            Term::Variable(name) => encoded_text_len(name)?,
            Term::Constant(value) => encoded_value_len(value)?,
        },
    )
}

fn encoded_terms_len(terms: &[Term]) -> OracleResult<usize> {
    terms.iter().try_fold(8, |length, term| {
        checked_len_add(length, encoded_term_len(term)?)
    })
}

fn encoded_atom_len(atom: &Atom) -> OracleResult<usize> {
    checked_len_add(
        encoded_text_len(&atom.relation)?,
        encoded_terms_len(&atom.terms)?,
    )
}

fn encoded_predicate_len(predicate: &Predicate) -> OracleResult<usize> {
    match predicate {
        Predicate::Greater(left, right) => checked_len_add(
            checked_len_add(1, encoded_text_len(left)?)?,
            encoded_text_len(right)?,
        ),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuleStaticQuote {
    atoms: usize,
    predicates: usize,
    terms: usize,
    canonical_bytes: usize,
    dependency_edges: usize,
}

struct SealedRulePlan<'a> {
    plan: &'a RulePlan,
    canonical_bytes: Arc<[u8]>,
    digest: [u8; 32],
    dependency_set: BTreeSet<String>,
    inferred_head_schema: Option<RelationSchema>,
    static_quote: RuleStaticQuote,
}

impl<'a> SealedRulePlan<'a> {
    fn new(plan: &'a RulePlan, schemas: &BTreeMap<String, RelationSchema>) -> Self {
        let canonical_bytes = Arc::<[u8]>::from(plan.canonical_bytes());
        let digest = Sha256::digest(canonical_bytes.as_ref()).into();
        let dependency_set = plan
            .atoms
            .iter()
            .map(|atom| atom.relation.clone())
            .collect::<BTreeSet<_>>();
        let static_quote = RuleStaticQuote {
            atoms: plan.atoms.len(),
            predicates: plan.predicates.len(),
            terms: plan.head.len()
                + plan
                    .atoms
                    .iter()
                    .map(|atom| atom.terms.len())
                    .sum::<usize>(),
            canonical_bytes: canonical_bytes.len(),
            dependency_edges: dependency_set.len(),
        };
        Self {
            plan,
            canonical_bytes,
            digest,
            dependency_set,
            inferred_head_schema: schemas.get(&plan.head_relation).cloned(),
            static_quote,
        }
    }

    fn validate(
        &self,
        store: &RelationStore,
        schemas: &BTreeMap<String, RelationSchema>,
    ) -> OracleResult<()> {
        let expected_quote = RuleStaticQuote {
            atoms: self.plan.atoms.len(),
            predicates: self.plan.predicates.len(),
            terms: self.plan.head.len()
                + self
                    .plan
                    .atoms
                    .iter()
                    .map(|atom| atom.terms.len())
                    .sum::<usize>(),
            canonical_bytes: self.canonical_bytes.len(),
            dependency_edges: self.dependency_set.len(),
        };
        if self.static_quote != expected_quote
            || self.inferred_head_schema.as_ref() != schemas.get(&self.plan.head_relation)
        {
            return Err("derivation.invalid_sealed_rule");
        }
        self.plan.validate(store, schemas)
    }
}
