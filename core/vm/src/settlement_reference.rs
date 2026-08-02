//! Small, deliberately unoptimized oracle for RFC-0001 settlements.
//!
//! This model has no VM, bytecode, GC, modules, networking, or provenance
//! ledger. It exists so generated tests can compare the production runtime
//! against a second implementation of grouping, canonicalization, isolated
//! candidate writes, conflict rejection, and atomic adoption.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReferenceValue {
    Bool(bool),
    Entity(u32),
    Int(i64),
    Text(String),
}

pub type ReferenceComponent = BTreeMap<String, ReferenceValue>;
pub type ReferenceSubject = (u32, String);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferenceWorld {
    pub components: BTreeMap<ReferenceSubject, ReferenceComponent>,
}

impl ReferenceWorld {
    pub fn component(&self, entity: u32, component: &str) -> Option<&ReferenceComponent> {
        self.components.get(&(entity, component.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceProposal {
    pub intent: String,
    pub key: u32,
    pub payload: ReferenceComponent,
    /// Stable typed-payload encoding used by the language runtime. Keeping
    /// it explicit makes this oracle independent from the VM value codec.
    pub canonical: String,
    pub producer: String,
    pub source_line: u32,
}

impl ReferenceProposal {
    fn canonical_key(&self) -> (&str, &str, u32) {
        (&self.canonical, &self.producer, self.source_line)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceWrite {
    pub entity: u32,
    pub component: String,
    pub value: ReferenceComponent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceResolution {
    pub intent: String,
    pub key: u32,
    pub resolver: String,
    pub proposals: Vec<ReferenceProposal>,
    pub writes: Vec<ReferenceWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceSettlement {
    pub world: ReferenceWorld,
    pub patch: BTreeMap<ReferenceSubject, ReferenceComponent>,
    pub resolutions: Vec<ReferenceResolution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReferenceError {
    MissingResolver {
        intent: String,
    },
    ResolverFailed {
        resolver: String,
        message: String,
    },
    CrossEntityWrite {
        resolver: String,
        key: u32,
        entity: u32,
    },
    DuplicateWrite {
        resolver: String,
        subject: ReferenceSubject,
    },
    ConflictingWrite {
        subject: ReferenceSubject,
        first_resolver: String,
        second_resolver: String,
    },
    MissingComponent {
        subject: ReferenceSubject,
    },
}

pub type ReferenceResolverFn = fn(
    key: u32,
    proposals: &[ReferenceProposal],
    base: &ReferenceWorld,
) -> Result<Vec<ReferenceWrite>, String>;

#[derive(Clone, Copy)]
pub struct ReferenceResolver {
    pub name: &'static str,
    pub resolve: ReferenceResolverFn,
}

/// Resolve one proposal multiset against `base` without mutating either
/// input. A successful result contains a newly adopted reference world.
pub fn settle_reference(
    base: &ReferenceWorld,
    proposals: Vec<ReferenceProposal>,
    resolvers: &BTreeMap<String, ReferenceResolver>,
) -> Result<ReferenceSettlement, ReferenceError> {
    let mut groups: BTreeMap<(String, u32), Vec<ReferenceProposal>> = BTreeMap::new();
    for proposal in proposals {
        groups
            .entry((proposal.intent.clone(), proposal.key))
            .or_default()
            .push(proposal);
    }
    for proposals in groups.values_mut() {
        proposals.sort_by(|left, right| left.canonical_key().cmp(&right.canonical_key()));
    }

    let mut resolutions = Vec::with_capacity(groups.len());
    let mut patch = BTreeMap::new();
    let mut owners: BTreeMap<ReferenceSubject, String> = BTreeMap::new();

    for ((intent, key), proposals) in groups {
        let resolver = resolvers
            .get(&intent)
            .ok_or_else(|| ReferenceError::MissingResolver {
                intent: intent.clone(),
            })?;
        let writes = (resolver.resolve)(key, &proposals, base).map_err(|message| {
            ReferenceError::ResolverFailed {
                resolver: resolver.name.to_string(),
                message,
            }
        })?;
        let mut local_subjects = BTreeMap::new();
        for write in &writes {
            if write.entity != key {
                return Err(ReferenceError::CrossEntityWrite {
                    resolver: resolver.name.to_string(),
                    key,
                    entity: write.entity,
                });
            }
            let subject = (write.entity, write.component.clone());
            if local_subjects.insert(subject.clone(), ()).is_some() {
                return Err(ReferenceError::DuplicateWrite {
                    resolver: resolver.name.to_string(),
                    subject,
                });
            }
            if !base.components.contains_key(&subject) {
                return Err(ReferenceError::MissingComponent { subject });
            }
            if let Some(first_resolver) = owners.insert(subject.clone(), resolver.name.to_string())
            {
                return Err(ReferenceError::ConflictingWrite {
                    subject,
                    first_resolver,
                    second_resolver: resolver.name.to_string(),
                });
            }
            patch.insert(subject, write.value.clone());
        }
        resolutions.push(ReferenceResolution {
            intent,
            key,
            resolver: resolver.name.to_string(),
            proposals,
            writes,
        });
    }

    let mut world = base.clone();
    for (subject, value) in &patch {
        world.components.insert(subject.clone(), value.clone());
    }
    Ok(ReferenceSettlement {
        world,
        patch,
        resolutions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int_field(component: &ReferenceComponent, field: &str) -> Result<i64, String> {
        match component.get(field) {
            Some(ReferenceValue::Int(value)) => Ok(*value),
            _ => Err(format!("missing integer field {field}")),
        }
    }

    fn damage(
        key: u32,
        proposals: &[ReferenceProposal],
        base: &ReferenceWorld,
    ) -> Result<Vec<ReferenceWrite>, String> {
        let health = base
            .component(key, "Health")
            .ok_or_else(|| "missing Health".to_string())?;
        let total = proposals
            .iter()
            .map(|proposal| int_field(&proposal.payload, "amount"))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .sum::<i64>();
        Ok(vec![ReferenceWrite {
            entity: key,
            component: "Health".to_string(),
            value: BTreeMap::from([(
                "hp".to_string(),
                ReferenceValue::Int(int_field(health, "hp")? - total),
            )]),
        }])
    }

    #[test]
    fn proposal_permutations_produce_the_same_world_and_provenance_shape() {
        let base = ReferenceWorld {
            components: BTreeMap::from([(
                (7, "Health".to_string()),
                BTreeMap::from([("hp".to_string(), ReferenceValue::Int(100))]),
            )]),
        };
        let proposal = |amount, line| ReferenceProposal {
            intent: "Damage".to_string(),
            key: 7,
            payload: BTreeMap::from([("amount".to_string(), ReferenceValue::Int(amount))]),
            canonical: format!("{amount:020}"),
            producer: "Hit".to_string(),
            source_line: line,
        };
        let resolvers = BTreeMap::from([(
            "Damage".to_string(),
            ReferenceResolver {
                name: "ResolveDamage",
                resolve: damage,
            },
        )]);
        let first = settle_reference(
            &base,
            vec![proposal(20, 1), proposal(5, 2), proposal(20, 3)],
            &resolvers,
        )
        .unwrap();
        let reversed = settle_reference(
            &base,
            vec![proposal(20, 3), proposal(5, 2), proposal(20, 1)],
            &resolvers,
        )
        .unwrap();
        assert_eq!(first.world, reversed.world);
        assert_eq!(first.resolutions, reversed.resolutions);
        assert_eq!(first.resolutions[0].proposals.len(), 3);
    }

    #[test]
    fn conflicts_return_an_error_without_mutating_the_base() {
        let base = ReferenceWorld {
            components: BTreeMap::from([(
                (7, "Health".to_string()),
                BTreeMap::from([("hp".to_string(), ReferenceValue::Int(100))]),
            )]),
        };
        let before = base.clone();
        let proposal = |intent: &str| ReferenceProposal {
            intent: intent.to_string(),
            key: 7,
            payload: BTreeMap::from([("amount".to_string(), ReferenceValue::Int(10))]),
            canonical: "10".to_string(),
            producer: intent.to_string(),
            source_line: 1,
        };
        let resolvers = BTreeMap::from([
            (
                "Damage".to_string(),
                ReferenceResolver {
                    name: "ResolveDamage",
                    resolve: damage,
                },
            ),
            (
                "Healing".to_string(),
                ReferenceResolver {
                    name: "ResolveHealing",
                    resolve: damage,
                },
            ),
        ]);
        let error = settle_reference(
            &base,
            vec![proposal("Damage"), proposal("Healing")],
            &resolvers,
        )
        .expect_err("two resolvers writing Health must conflict");
        assert!(matches!(error, ReferenceError::ConflictingWrite { .. }));
        assert_eq!(base, before);
    }
}
