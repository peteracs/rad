// Relation state owned by the world: manifests, candidate staging,
// derivation, transactional application, and checkpoint restore.

impl World {

    pub fn relation_state(&self) -> &crate::relation::runtime::AuthoritativeRelationState {
        &self.authoritative_relations
    }

    pub fn derived_relation_state(&self) -> &crate::relation::derivation::DerivedRelationState {
        &self.derived_relations
    }

    pub(crate) fn restore_relation_transport(
        &mut self,
        encoded: &str,
        manifest: std::sync::Arc<crate::relation::runtime::RelationRuntimeManifest>,
    ) -> crate::relation::runtime::RelationRuntimeResult<()> {
        let state = crate::relation::runtime::AuthoritativeRelationState::from_transport_hex(
            encoded, manifest,
        )?;
        state.validate_live_entity_set(&self.live_relation_entities())?;
        let derived = Self::derive_relations(&state)?;
        self.authoritative_relations = state;
        self.derived_relations = derived;
        Ok(())
    }

    pub fn install_relation_manifest(
        &mut self,
        manifest: std::sync::Arc<crate::relation::runtime::RelationRuntimeManifest>,
        expected: crate::relation::frontend::FrontendManifestDigest,
    ) -> crate::relation::runtime::RelationRuntimeResult<()> {
        let mut authoritative = self.authoritative_relations.clone();
        authoritative.install_manifest(manifest, expected)?;
        let derived = Self::derive_relations(&authoritative)?;
        self.authoritative_relations = authoritative;
        self.derived_relations = derived;
        Ok(())
    }

    pub(crate) fn live_relation_entities(
        &self,
    ) -> std::collections::BTreeSet<crate::relation::runtime::EntityRef> {
        self.entity_archetype
            .keys()
            .filter_map(|id| self.entity_ref(*id))
            .collect()
    }

    pub(crate) fn prepare_relation_candidate(
        &self,
        transaction: &crate::relation::runtime::RelationTransaction,
        live_after: std::collections::BTreeSet<crate::relation::runtime::EntityRef>,
        handles: std::collections::BTreeMap<u32, crate::relation::runtime::EntityRef>,
    ) -> crate::relation::runtime::RelationRuntimeResult<crate::relation::runtime::RelationCandidate>
    {
        self.authoritative_relations.prepare_candidate(
            transaction,
            &crate::relation::runtime::CandidateEntityState {
                live_after,
                candidate_handles: handles,
            },
        )
    }

    pub(crate) fn adopt_relation_candidate(
        &mut self,
        candidate: crate::relation::runtime::RelationCandidate,
    ) -> crate::relation::runtime::RelationRuntimeResult<Vec<crate::relation::runtime::FactChange>>
    {
        let mut authoritative = self.authoritative_relations.clone();
        let changes = authoritative.adopt(candidate);
        let derived = Self::maintain_relations(&self.derived_relations, &authoritative, &changes)?;
        self.authoritative_relations = authoritative;
        self.derived_relations = derived;
        Ok(changes)
    }

    fn maintain_relations(
        previous: &crate::relation::derivation::DerivedRelationState,
        authoritative: &crate::relation::runtime::AuthoritativeRelationState,
        changes: &[crate::relation::runtime::FactChange],
    ) -> crate::relation::runtime::RelationRuntimeResult<
        crate::relation::derivation::DerivedRelationState,
    > {
        let Some(manifest) = authoritative.manifest() else {
            return Ok(crate::relation::derivation::DerivedRelationState::default());
        };
        crate::relation::derivation::maintain_indexed(
            previous,
            authoritative,
            manifest,
            changes,
            crate::relation::derivation::DerivationLimits::default(),
        )
        .map_err(|error| crate::relation::runtime::RelationRuntimeError {
            code: error.code,
            detail: error.detail,
        })
    }

    fn derive_relations(
        authoritative: &crate::relation::runtime::AuthoritativeRelationState,
    ) -> crate::relation::runtime::RelationRuntimeResult<
        crate::relation::derivation::DerivedRelationState,
    > {
        let Some(manifest) = authoritative.manifest() else {
            return Ok(crate::relation::derivation::DerivedRelationState::default());
        };
        crate::relation::derivation::derive_all(
            authoritative,
            manifest,
            crate::relation::derivation::DerivationLimits::default(),
        )
        .map_err(|error| crate::relation::runtime::RelationRuntimeError {
            code: error.code,
            detail: error.detail,
        })
    }

    /// Apply authoritative relation operations and entity deletion as one
    /// copy-on-write world candidate. A relation failure leaves ECS rows,
    /// assertion identities, indexes, and provenance untouched.
    pub fn apply_relation_transaction(
        &mut self,
        transaction: &crate::relation::runtime::RelationTransaction,
    ) -> crate::relation::runtime::RelationRuntimeResult<Vec<crate::relation::runtime::FactChange>>
    {
        // Construct the complete ECS + relation candidate in an isolated CoW
        // world. No allocator, component, entity, assertion, or index state is
        // adopted unless every phase succeeds.
        let mut candidate_world = World::new();
        candidate_world.restore(self.snapshot());
        let mut handles = std::collections::BTreeMap::new();
        let mut spawns = transaction.spawns.clone();
        spawns.sort_unstable_by_key(|spawn| spawn.handle);
        for pair in spawns.windows(2) {
            if pair[0].handle == pair[1].handle {
                return Err(crate::relation::runtime::RelationRuntimeError {
                    code: "entity.duplicate_candidate_handle",
                    detail: pair[0].handle.to_string(),
                });
            }
        }
        for spawn in spawns {
            let slot = candidate_world
                .spawn_entity(spawn.name.as_deref())
                .map_err(|error| crate::relation::runtime::RelationRuntimeError {
                    code: error.code(),
                    detail: "candidate entity allocation failed".into(),
                })?;
            handles.insert(
                spawn.handle,
                candidate_world
                    .entity_ref(slot)
                    .expect("new entity is live"),
            );
        }

        let mut component_writes = std::collections::BTreeMap::<
            (crate::relation::runtime::EntityRef, String),
            crate::value::ComponentData,
        >::new();
        for write in &transaction.component_writes {
            let entity = match write.entity {
                crate::relation::runtime::EntityOperand::Existing(entity) => entity,
                crate::relation::runtime::EntityOperand::Candidate(handle) => *handles
                    .get(&handle)
                    .ok_or_else(|| crate::relation::runtime::RelationRuntimeError {
                        code: "entity.unknown_candidate_handle",
                        detail: handle.to_string(),
                    })?,
            };
            if candidate_world.entity_ref(entity.slot) != Some(entity) {
                return Err(crate::relation::runtime::RelationRuntimeError {
                    code: "component.entity_not_live",
                    detail: format!("{}:{}", entity.slot, entity.generation),
                });
            }
            let key = (entity, write.component.type_name.clone());
            match component_writes.get(&key) {
                Some(existing) if existing != &write.component => {
                    return Err(crate::relation::runtime::RelationRuntimeError {
                        code: "component.write_conflict",
                        detail: format!("{}:{}::{}", entity.slot, entity.generation, key.1),
                    });
                }
                Some(_) => {}
                None => {
                    component_writes.insert(key, write.component.clone());
                }
            }
        }
        for ((entity, _), component) in component_writes {
            if !candidate_world.add_component(entity.slot, component) {
                return Err(crate::relation::runtime::RelationRuntimeError {
                    code: "component.write_failed",
                    detail: format!("{}:{}", entity.slot, entity.generation),
                });
            }
        }

        let mut live_after = candidate_world.live_relation_entities();
        let despawn_entities = transaction
            .despawns
            .iter()
            .map(|despawn| despawn.entity)
            .collect::<std::collections::BTreeSet<_>>();
        for entity in &despawn_entities {
            if !live_after.remove(entity) {
                return Err(crate::relation::runtime::RelationRuntimeError {
                    code: "entity.not_live",
                    detail: format!(
                        "{}:{} is not a live entity lifetime",
                        entity.slot, entity.generation
                    ),
                });
            }
        }
        let candidate =
            candidate_world.prepare_relation_candidate(transaction, live_after, handles)?;
        for entity in despawn_entities {
            // Exact lifetime membership was checked above; the raw slot is
            // now safe to remove only after the complete relation candidate
            // has passed restrict/cascade, foreign-key, and unique checks.
            let removed = candidate_world.destroy_entity_storage(entity.slot);
            debug_assert!(removed);
        }
        let changes = candidate_world.adopt_relation_candidate(candidate)?;
        self.restore(candidate_world.snapshot());
        Ok(changes)
    }

    /// Apply a host-admitted transaction. The complete envelope was checked
    /// before this method can clone or mutate the candidate world.
    pub fn apply_bounded_relation_transaction(
        &mut self,
        transaction: &crate::relation::runtime::BoundedRelationTransaction,
    ) -> crate::relation::runtime::RelationRuntimeResult<Vec<crate::relation::runtime::FactChange>>
    {
        self.apply_relation_transaction(transaction.transaction())
    }

    pub(crate) fn transaction_relation_checkpoint(&self) -> TransactionRelationCheckpoint {
        TransactionRelationCheckpoint {
            authoritative: self.authoritative_relations.clone(),
            derived: self.derived_relations.clone(),
        }
    }

    pub(crate) fn restore_transaction_relations(
        &mut self,
        checkpoint: TransactionRelationCheckpoint,
    ) {
        self.authoritative_relations = checkpoint.authoritative;
        self.derived_relations = checkpoint.derived;
    }
}
