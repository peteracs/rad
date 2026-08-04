//! Executable semantic oracle for RFC-0003.
//!
//! This deliberately contains no parser, bytecode, VM, GC, or ECS code. Full
//! recomputation defines the result; the tiny affected-group maintainer is
//! differential-tested against it before production lowering exists.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Fact {
    relation: &'static str,
    tuple: Vec<i64>,
}

impl Fact {
    fn new(relation: &'static str, tuple: impl Into<Vec<i64>>) -> Self {
        Self {
            relation,
            tuple: tuple.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Proof {
    rule: &'static str,
    supports: BTreeSet<Fact>,
}

type BaseFacts = BTreeSet<Fact>;
type DerivedFacts = BTreeMap<Fact, BTreeSet<Proof>>;

fn relation<'a>(facts: &'a BaseFacts, name: &'static str) -> impl Iterator<Item = &'a Fact> {
    facts.iter().filter(move |fact| fact.relation == name)
}

fn derive_person(base: &BaseFacts, person: i64) -> DerivedFacts {
    let weights = relation(base, "ItemWeight")
        .map(|fact| (fact.tuple[0], (fact.tuple[1], fact.clone())))
        .collect::<BTreeMap<_, _>>();
    let capacity = relation(base, "CarryCapacity")
        .find(|fact| fact.tuple[0] == person)
        .cloned();

    let mut total = 0_i64;
    let mut supports = BTreeSet::new();
    let mut has_item = false;
    for ownership in relation(base, "Owns").filter(|fact| fact.tuple[0] == person) {
        let item = ownership.tuple[1];
        if let Some((weight, weight_fact)) = weights.get(&item) {
            total = total
                .checked_add(*weight)
                .expect("reference fixture sum fits");
            has_item = true;
            supports.insert(ownership.clone());
            supports.insert(weight_fact.clone());
        }
    }

    let mut derived = DerivedFacts::new();
    if !has_item {
        return derived;
    }

    let total_fact = Fact::new("TotalWeight", vec![person, total]);
    derived
        .entry(total_fact.clone())
        .or_default()
        .insert(Proof {
            rule: "derive.TotalWeight",
            supports,
        });
    if let Some(capacity_fact) = capacity {
        if total > capacity_fact.tuple[1] {
            derived
                .entry(Fact::new("Encumbered", vec![person]))
                .or_default()
                .insert(Proof {
                    rule: "derive.Encumbered",
                    supports: BTreeSet::from([total_fact, capacity_fact]),
                });
        }
    }
    derived
}

fn derive_all(base: &BaseFacts) -> DerivedFacts {
    let people = relation(base, "Owns")
        .map(|fact| fact.tuple[0])
        .chain(relation(base, "CarryCapacity").map(|fact| fact.tuple[0]))
        .collect::<BTreeSet<_>>();
    let mut derived = DerivedFacts::new();
    for person in people {
        derived.extend(derive_person(base, person));
    }
    derived
}

#[derive(Clone, Debug)]
enum Delta {
    Insert(Fact),
    Remove(Fact),
}

#[derive(Clone, Debug)]
struct IncrementalReference {
    base: BaseFacts,
    derived: DerivedFacts,
}

impl IncrementalReference {
    fn new(base: BaseFacts) -> Self {
        let derived = derive_all(&base);
        Self { base, derived }
    }

    fn affected_people(&self, fact: &Fact) -> BTreeSet<i64> {
        match fact.relation {
            "Owns" | "CarryCapacity" => BTreeSet::from([fact.tuple[0]]),
            "ItemWeight" => relation(&self.base, "Owns")
                .filter(|ownership| ownership.tuple[1] == fact.tuple[0])
                .map(|ownership| ownership.tuple[0])
                .collect(),
            _ => BTreeSet::new(),
        }
    }

    fn apply(&mut self, delta: Delta) {
        let fact = match &delta {
            Delta::Insert(fact) | Delta::Remove(fact) => fact.clone(),
        };
        let mut affected = self.affected_people(&fact);
        match delta {
            Delta::Insert(fact) => {
                self.base.insert(fact);
            }
            Delta::Remove(fact) => {
                self.base.remove(&fact);
            }
        }
        affected.extend(self.affected_people(&fact));

        self.derived
            .retain(|fact, _| !affected.contains(&fact.tuple[0]));
        for person in affected {
            self.derived.extend(derive_person(&self.base, person));
        }
    }
}

#[derive(Clone)]
struct RelationSchema {
    arity: usize,
    unique_columns: BTreeSet<usize>,
    symmetric: bool,
}

impl RelationSchema {
    fn canonicalize(&self, mut tuple: Vec<i64>) -> Result<Vec<i64>, &'static str> {
        if tuple.len() != self.arity {
            return Err("relation.arity");
        }
        if self.symmetric {
            if self.arity != 2 {
                return Err("relation.symmetric_arity");
            }
            if tuple[1] < tuple[0] {
                tuple.swap(0, 1);
            }
        }
        Ok(tuple)
    }
}

#[derive(Clone, Default)]
struct RelationStore {
    schemas: BTreeMap<&'static str, RelationSchema>,
    rows: BTreeMap<&'static str, BTreeSet<Vec<i64>>>,
}

impl RelationStore {
    fn apply_patch(&mut self, patch: impl IntoIterator<Item = Fact>) -> Result<(), &'static str> {
        let mut candidate = self.rows.clone();
        for fact in patch {
            let schema = self.schemas.get(fact.relation).ok_or("relation.unknown")?;
            candidate
                .entry(fact.relation)
                .or_default()
                .insert(schema.canonicalize(fact.tuple)?);
        }
        for (name, rows) in &candidate {
            let schema = &self.schemas[name];
            for column in &schema.unique_columns {
                let mut keys = BTreeSet::new();
                for row in rows {
                    if !keys.insert(row[*column]) {
                        return Err("relation.unique_conflict");
                    }
                }
            }
        }
        self.rows = candidate;
        Ok(())
    }
}

fn fixture_base() -> Vec<Fact> {
    vec![
        Fact::new("Owns", vec![1, 10]),
        Fact::new("Owns", vec![1, 11]),
        Fact::new("Owns", vec![2, 12]),
        Fact::new("ItemWeight", vec![10, 7]),
        Fact::new("ItemWeight", vec![11, 6]),
        Fact::new("ItemWeight", vec![12, 2]),
        Fact::new("CarryCapacity", vec![1, 10]),
        Fact::new("CarryCapacity", vec![2, 10]),
    ]
}

#[test]
fn canonical_relation_rows_are_order_independent_and_schema_atomic() {
    let mut store = RelationStore::default();
    store.schemas.insert(
        "AlliedWith",
        RelationSchema {
            arity: 2,
            unique_columns: BTreeSet::new(),
            symmetric: true,
        },
    );
    store.schemas.insert(
        "Owns",
        RelationSchema {
            arity: 2,
            unique_columns: BTreeSet::from([1]),
            symmetric: false,
        },
    );

    store
        .apply_patch([
            Fact::new("AlliedWith", vec![9, 4]),
            Fact::new("AlliedWith", vec![4, 9]),
        ])
        .unwrap();
    assert_eq!(store.rows["AlliedWith"], BTreeSet::from([vec![4, 9]]));

    let before = store.rows.clone();
    assert_eq!(
        store.apply_patch([
            Fact::new("Owns", vec![1, 20]),
            Fact::new("Owns", vec![2, 20]),
        ]),
        Err("relation.unique_conflict")
    );
    assert_eq!(store.rows, before, "failed schema patch must be atomic");
}

#[test]
fn insertion_permutations_have_identical_facts_and_proofs() {
    let forward = fixture_base().into_iter().collect::<BaseFacts>();
    let reverse = fixture_base().into_iter().rev().collect::<BaseFacts>();
    assert_eq!(forward, reverse);
    assert_eq!(derive_all(&forward), derive_all(&reverse));
    assert!(derive_all(&forward).contains_key(&Fact::new("Encumbered", vec![1])));
}

#[test]
fn indexed_affected_group_maintenance_matches_full_recomputation() {
    let mut incremental = IncrementalReference::new(fixture_base().into_iter().collect());
    let changes = [
        Delta::Remove(Fact::new("ItemWeight", vec![11, 6])),
        Delta::Insert(Fact::new("ItemWeight", vec![11, 1])),
        Delta::Remove(Fact::new("Owns", vec![1, 10])),
        Delta::Insert(Fact::new("Owns", vec![2, 10])),
        Delta::Remove(Fact::new("CarryCapacity", vec![2, 10])),
        Delta::Insert(Fact::new("CarryCapacity", vec![2, 5])),
    ];
    for change in changes {
        incremental.apply(change);
        assert_eq!(incremental.derived, derive_all(&incremental.base));
    }
}

#[test]
fn why_chain_reaches_exact_authoritative_supports() {
    let base = fixture_base().into_iter().collect::<BaseFacts>();
    let derived = derive_all(&base);
    let encumbered = Fact::new("Encumbered", vec![1]);
    let encumbered_proof = derived[&encumbered].iter().next().unwrap();
    let total = encumbered_proof
        .supports
        .iter()
        .find(|fact| fact.relation == "TotalWeight")
        .unwrap();
    let total_proof = derived[total].iter().next().unwrap();

    assert_eq!(encumbered_proof.rule, "derive.Encumbered");
    assert!(encumbered_proof
        .supports
        .contains(&Fact::new("CarryCapacity", vec![1, 10])));
    assert_eq!(total_proof.rule, "derive.TotalWeight");
    assert_eq!(
        total_proof.supports,
        BTreeSet::from([
            Fact::new("Owns", vec![1, 10]),
            Fact::new("Owns", vec![1, 11]),
            Fact::new("ItemWeight", vec![10, 7]),
            Fact::new("ItemWeight", vec![11, 6]),
        ])
    );
}
