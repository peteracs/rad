

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(name: &str) -> ComponentData {
        ComponentData {
            type_name: name.to_string(),
            layout: std::sync::Arc::new(Vec::new()),
            values: Vec::new(),
        }
    }

    #[test]
    fn spawn_entity_with_name_sets_bidirectional_maps() {
        let mut w = World::new();
        let e = w.spawn_entity(Some("hero")).unwrap();
        assert_eq!(w.name_to_id.get("hero"), Some(&e));
        assert_eq!(w.id_to_name.get(&e), Some(&"hero".to_string()));
    }

    #[test]
    fn duplicate_name_cleans_up_old_entity_mapping() {
        let mut w = World::new();
        let e0 = w.spawn_entity(Some("player")).unwrap();
        let e1 = w.spawn_entity(Some("player")).unwrap();
        assert_ne!(e0, e1);
        assert_eq!(w.name_to_id.get("player"), Some(&e1));
        assert_eq!(w.id_to_name.get(&e1), Some(&"player".to_string()));
        assert!(!w.id_to_name.contains_key(&e0));
    }

    #[test]
    fn destroy_after_name_reuse_cleans_up_correctly() {
        let mut w = World::new();
        let e0 = w.spawn_entity(Some("npc")).unwrap();
        let e1 = w.spawn_entity(Some("npc")).unwrap();
        w.destroy_entity(e1);
        assert!(!w.name_to_id.contains_key("npc"));
        assert!(!w.id_to_name.contains_key(&e1));
        assert!(!w.id_to_name.contains_key(&e0));
    }

    #[test]
    fn destroy_old_entity_after_name_reuse_does_not_remove_new_mapping() {
        let mut w = World::new();
        let e0 = w.spawn_entity(Some("npc")).unwrap();
        let _e1 = w.spawn_entity(Some("npc")).unwrap();
        w.destroy_entity(e0);
        assert_eq!(w.name_to_id.get("npc"), Some(&_e1));
        assert_eq!(w.id_to_name.get(&_e1), Some(&"npc".to_string()));
    }

    #[test]
    fn spawn_unnamed_entity_does_not_pollute_name_maps() {
        let mut w = World::new();
        let e = w.spawn_entity(None).unwrap();
        assert!(w.name_to_id.is_empty());
        assert!(w.id_to_name.is_empty());
        assert!(w.entity_archetype.contains_key(&e));
    }

    #[test]
    fn spawn_empty_name_does_not_pollute_name_maps() {
        let mut w = World::new();
        w.spawn_entity(Some("")).unwrap();
        assert!(w.name_to_id.is_empty());
        assert!(w.id_to_name.is_empty());
    }

    #[test]
    fn destroyed_entity_id_is_reused() {
        let mut w = World::new();
        let e0 = w.spawn_entity(None).unwrap();
        w.destroy_entity(e0);
        let e1 = w.spawn_entity(None).unwrap();
        assert_eq!(e0, e1);
    }

    #[test]
    fn reusable_entities_are_selected_in_canonical_order() {
        let mut world = World::new();
        let ids = (0..3)
            .map(|_| world.spawn_entity(None).unwrap())
            .collect::<Vec<_>>();
        for entity in [ids[2], ids[0], ids[1]] {
            assert!(world.destroy_entity(entity));
        }

        assert_eq!(world.spawn_entity(None), Ok(ids[0]));
        assert_eq!(world.spawn_entity(None), Ok(ids[1]));
        assert_eq!(world.spawn_entity(None), Ok(ids[2]));
    }

    #[test]
    fn generation_exhausted_entity_retires_at_destruction() {
        let mut world = World::new();
        let entity = world.spawn_entity(None).unwrap();
        Arc::make_mut(&mut world.generations).insert(entity, u32::MAX);

        assert!(world.destroy_entity(entity));
        assert!(!world.free_ids.contains(&entity));
        assert_eq!(world.spawn_entity(None), Ok(1));
    }

    #[test]
    fn explicit_entity_gap_limit_rejects_before_allocator_mutation() {
        let mut world = World::new();
        let before = world.snapshot();

        assert!(matches!(
            world.insert_entity_with_id(u32::MAX, None),
            Err(EntityAllocationError::ExplicitIdGapTooLarge { .. })
        ));
        assert_eq!(world.next_id, before.next_id);
        assert_eq!(world.free_ids, before.free_ids);
        assert_eq!(world.generations, before.generations);
        assert!(world.all_entity_ids().is_empty());
        assert_eq!(world.spawn_entity(None), Ok(0));
    }

    #[test]
    fn explicit_entity_claim_rejects_an_unpartitioned_issued_slot() {
        let mut world = World::new();
        let mut corrupted = world.snapshot();
        corrupted.next_id = 1;
        world.restore(corrupted);

        assert_eq!(
            world.insert_entity_with_id(0, None),
            Err(EntityAllocationError::ExplicitIdNotReusable(0))
        );
        assert!(world.all_entity_ids().is_empty());
        assert!(world.free_ids.is_empty());
    }

    #[test]
    fn spawn_rejects_a_corrupted_live_free_overlap_without_duplicating_storage() {
        let mut world = World::new();
        let entity = world.spawn_entity(None).unwrap();
        Arc::make_mut(&mut world.free_ids).insert(entity);
        let before_rows = world.archetypes[0].entities.clone();

        assert_eq!(
            world.spawn_entity(None),
            Err(EntityAllocationError::AllocatorLiveFreeOverlap(entity))
        );
        assert_eq!(world.free_ids.iter().copied().collect::<Vec<_>>(), vec![entity]);
        assert_eq!(world.all_entity_ids(), vec![entity]);
        assert_eq!(world.archetypes[0].entities, before_rows);
    }

    #[test]
    fn reusable_identity_set_cannot_queue_one_slot_twice() {
        let mut world = World::new();
        let entity = world.spawn_entity(None).unwrap();
        assert!(world.destroy_entity(entity));
        assert!(!Arc::make_mut(&mut world.free_ids).insert(entity));

        assert_eq!(world.spawn_entity(None), Ok(entity));
        assert_eq!(world.spawn_entity(None), Ok(1));
        assert_eq!(world.all_entity_ids(), vec![entity, 1]);
        assert_eq!(world.archetypes[0].entities.as_slice(), &[entity, 1]);
        assert_eq!(world.archetypes[0].entity_row.get(&entity), Some(&0));
    }

    #[test]
    fn archetype_rejects_duplicate_physical_entity_rows() {
        let mut archetype = Archetype::new(Vec::new());
        assert_eq!(archetype.push_entity(7, HashMap::new()), Ok(()));
        assert_eq!(
            archetype.push_entity(7, HashMap::new()),
            Err(EntityAllocationError::ArchetypeDuplicate(7))
        );
        assert_eq!(archetype.entities.as_slice(), &[7]);
        assert_eq!(archetype.entity_row.get(&7), Some(&0));
    }

    #[test]
    fn add_and_get_component() {
        let mut w = World::new();
        let e = w.spawn_entity(Some("hero")).unwrap();
        w.add_component(
            e,
            ComponentData {
                type_name: "Health".to_string(),
                layout: std::sync::Arc::new(vec!["hp".to_string()]),
                values: vec![crate::value::Value::int(100)],
            },
        );
        let c = w.get_component(e, "Health").unwrap();
        assert_eq!(c.type_name, "Health");
    }

    #[test]
    fn add_component_on_invalid_entity_is_noop() {
        let mut w = World::new();
        w.add_component(999, comp("Health"));
        assert!(w.get_component(999, "Health").is_none());
    }

    #[test]
    fn remove_component_clears_from_query() {
        let mut w = World::new();
        let e = w.spawn_entity(None).unwrap();
        w.add_component(e, comp("Pos"));
        assert!(w.has_component(e, "Pos"));
        w.remove_component(e, "Pos");
        assert!(!w.has_component(e, "Pos"));
        assert!(w.query(&["Pos".to_string()], &[]).is_empty());
    }

    #[test]
    fn query_returns_entities_with_all_requested_components() {
        let mut w = World::new();
        let e0 = w.spawn_entity(None).unwrap();
        let e1 = w.spawn_entity(None).unwrap();
        w.add_component(e0, comp("Pos"));
        w.add_component(e0, comp("Vel"));
        w.add_component(e1, comp("Pos"));
        let both = w.query(&["Pos".to_string(), "Vel".to_string()], &[]);
        assert_eq!(both, vec![e0]);
        let just_pos = w.query(&["Pos".to_string()], &[]);
        assert!(just_pos.contains(&e0));
        assert!(just_pos.contains(&e1));
    }

    #[test]
    fn destroy_entity_removes_from_all_queries() {
        let mut w = World::new();
        let e = w.spawn_entity(Some("tmp")).unwrap();
        w.add_component(e, comp("Pos"));
        w.destroy_entity(e);
        assert!(w.query(&["Pos".to_string()], &[]).is_empty());
        assert!(w.get_component(e, "Pos").is_none());
        assert!(!w.name_to_id.contains_key("tmp"));
    }

    #[test]
    fn archetype_migration_preserves_existing_components() {
        let mut w = World::new();
        let e = w.spawn_entity(None).unwrap();
        w.add_component(
            e,
            ComponentData {
                type_name: "Pos".to_string(),
                layout: std::sync::Arc::new(vec!["x".to_string()]),
                values: vec![crate::value::Value::int(10)],
            },
        );
        w.add_component(e, comp("Vel"));
        let c = w.get_component(e, "Pos").unwrap();
        assert_eq!(c.values[0].as_int(), Some(10));
        assert!(w.has_component(e, "Vel"));
    }

    #[test]
    fn set_component_overwrites_in_place() {
        let mut w = World::new();
        let e = w.spawn_entity(None).unwrap();
        let layout = std::sync::Arc::new(vec!["hp".to_string()]);
        w.add_component(
            e,
            ComponentData {
                type_name: "Health".to_string(),
                layout: layout.clone(),
                values: vec![crate::value::Value::int(100)],
            },
        );
        w.set_component(
            e,
            ComponentData {
                type_name: "Health".to_string(),
                layout,
                values: vec![crate::value::Value::int(50)],
            },
        );
        let c = w.get_component(e, "Health").unwrap();
        assert_eq!(c.values[0].as_int(), Some(50));
    }

    #[test]
    fn query_with_many_archetypes() {
        let mut w = World::new();
        let e0 = w.spawn_entity(None).unwrap();
        let e1 = w.spawn_entity(None).unwrap();
        let e2 = w.spawn_entity(None).unwrap();
        w.add_component(e0, comp("A"));
        w.add_component(e0, comp("B"));
        w.add_component(e1, comp("A"));
        w.add_component(e1, comp("B"));
        w.add_component(e1, comp("C"));
        w.add_component(e2, comp("A"));
        let q_a = w.query(&["A".to_string()], &[]);
        assert_eq!(q_a.len(), 3);
        let q_ab = w.query(&["A".to_string(), "B".to_string()], &[]);
        assert_eq!(q_ab.len(), 2);
        let q_abc = w.query(&["A".to_string(), "B".to_string(), "C".to_string()], &[]);
        assert_eq!(q_abc, vec![e1]);
    }

    #[test]
    fn snapshot_keeps_string_field_alive_after_overwrite() {
        let mut w = World::new();
        let mut gc = crate::gc::GcHeap::new();
        let e = w.spawn_entity(None).unwrap();
        let layout = std::sync::Arc::new(vec!["name".to_string()]);

        w.add_component(
            e,
            ComponentData {
                type_name: "Name".to_string(),
                layout: layout.clone(),
                values: vec![crate::value::Value::from_string(&mut gc, "old".to_string())],
            },
        );

        let snap = w.snapshot();

        w.set_component(
            e,
            ComponentData {
                type_name: "Name".to_string(),
                layout: layout.clone(),
                values: vec![crate::value::Value::from_string(&mut gc, "new".to_string())],
            },
        );

        let live = w.get_component(e, "Name").unwrap();
        assert_eq!(live.values[0].as_str(), Some("new"));

        let snap_val = snap.get_component(e, "Name").unwrap();
        assert_eq!(snap_val.values[0].as_str(), Some("old"));
    }

    #[test]
    fn indexed_lookup_finds_entity_by_component_field() {
        let mut w = World::new();
        let mut indexed = std::collections::HashMap::new();
        indexed.insert(
            "Tag".to_string(),
            std::collections::HashSet::from(["name".to_string()]),
        );
        w.set_indexed_fields(indexed);
        let e = w.spawn_entity(None).unwrap();
        w.add_component(
            e,
            ComponentData {
                type_name: "Tag".to_string(),
                layout: std::sync::Arc::new(vec!["name".to_string()]),
                values: vec![crate::value::Value::int(7)],
            },
        );
        assert_eq!(
            w.index_lookup("Tag", "name", crate::value::Value::int(7)),
            Some(e)
        );
    }

    #[test]
    fn indexed_lookup_updates_after_component_overwrite() {
        let mut w = World::new();
        let mut indexed = std::collections::HashMap::new();
        indexed.insert(
            "Tag".to_string(),
            std::collections::HashSet::from(["name".to_string()]),
        );
        w.set_indexed_fields(indexed);
        let e = w.spawn_entity(None).unwrap();
        let layout = std::sync::Arc::new(vec!["name".to_string()]);
        w.add_component(
            e,
            ComponentData {
                type_name: "Tag".to_string(),
                layout: layout.clone(),
                values: vec![crate::value::Value::int(1)],
            },
        );
        w.set_component(
            e,
            ComponentData {
                type_name: "Tag".to_string(),
                layout,
                values: vec![crate::value::Value::int(2)],
            },
        );
        assert_eq!(
            w.index_lookup("Tag", "name", crate::value::Value::int(1)),
            None
        );
        assert_eq!(
            w.index_lookup("Tag", "name", crate::value::Value::int(2)),
            Some(e)
        );
    }

    #[test]
    fn indexed_lookup_supports_float_values() {
        let mut w = World::new();
        let mut indexed = std::collections::HashMap::new();
        indexed.insert(
            "Tag".to_string(),
            std::collections::HashSet::from(["score".to_string()]),
        );
        w.set_indexed_fields(indexed);
        let e = w.spawn_entity(None).unwrap();
        w.add_component(
            e,
            ComponentData {
                type_name: "Tag".to_string(),
                layout: std::sync::Arc::new(vec!["score".to_string()]),
                values: vec![crate::value::Value::from_float(3.5)],
            },
        );
        assert_eq!(
            w.index_lookup("Tag", "score", crate::value::Value::from_float(3.5)),
            Some(e)
        );
    }
}
