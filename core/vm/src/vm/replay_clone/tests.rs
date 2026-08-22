// Fingerprint bound and clone-fidelity tests.
#[cfg(test)]
mod tests {
    use super::{
        fingerprint_roots, fingerprint_roots_with_limits, fingerprint_world_snapshot,
        FingerprintError, FingerprintLimits,
    };
    use crate::causality::WireProvenance;
    use crate::gc::GcHeap;
    use crate::value::Value;
    use crate::world::World;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    #[test]
    fn operational_world_identity_includes_hidden_allocator_and_type_state() {
        let baseline = World::new().snapshot();
        let visible = baseline.snapshot_json_like();
        let digest = fingerprint_world_snapshot(&baseline);

        let mut next_id = baseline.clone();
        next_id.next_id = 41;
        assert_eq!(visible, next_id.snapshot_json_like());
        assert_ne!(digest, fingerprint_world_snapshot(&next_id));

        let mut free_ids = baseline.clone();
        free_ids.free_ids = Arc::new(BTreeSet::from([3, 7]));
        assert_eq!(visible, free_ids.snapshot_json_like());
        let free_digest = fingerprint_world_snapshot(&free_ids);
        assert_ne!(digest, free_digest);
        free_ids.free_ids = Arc::new([7, 3].into_iter().collect());
        assert_eq!(free_digest, fingerprint_world_snapshot(&free_ids));

        let mut types = baseline.clone();
        types.type_registry = Arc::new(crate::world::FastMap::from_iter([(
            "HiddenType".to_string(),
            9u32,
        )]));
        types.next_type_id = 10;
        assert_eq!(visible, types.snapshot_json_like());
        assert_ne!(digest, fingerprint_world_snapshot(&types));
    }

    #[test]
    fn world_fork_identity_includes_events_timers_provenance_and_seed() {
        fn root_fingerprint(snapshot: crate::world::WorldSnapshot) -> String {
            let mut gc = GcHeap::new();
            let root = Value::world_fork(&mut gc, Arc::new(snapshot));
            fingerprint_roots(&[root]).expect("test fingerprint should fit")
        }

        let baseline = World::new().snapshot();
        let visible = baseline.snapshot_json_like();
        let digest = root_fingerprint(baseline.clone());

        let mut event = baseline.clone();
        event.events = Arc::new(vec![("Ping".to_string(), Value::int(1), 11)]);
        assert_eq!(visible, event.snapshot_json_like());
        let event_digest = root_fingerprint(event.clone());
        assert_ne!(digest, event_digest);
        event.events = Arc::new(vec![("Ping".to_string(), Value::int(2), 11)]);
        assert_ne!(event_digest, root_fingerprint(event));

        let mut delayed = baseline.clone();
        delayed.delayed = Arc::new(vec![(3, "Later".to_string(), Value::int(2), 12)]);
        assert_eq!(visible, delayed.snapshot_json_like());
        let delayed_digest = root_fingerprint(delayed.clone());
        assert_ne!(digest, delayed_digest);
        delayed.delayed = Arc::new(vec![(4, "Later".to_string(), Value::int(2), 12)]);
        assert_ne!(delayed_digest, root_fingerprint(delayed));

        let mut provenance = baseline.clone();
        provenance.provenance = Some(Arc::new(WireProvenance {
            origin: "remote-a".to_string(),
            ..WireProvenance::default()
        }));
        assert_eq!(visible, provenance.snapshot_json_like());
        assert_ne!(digest, root_fingerprint(provenance));

        let mut seeded = baseline;
        seeded.rollout_seed = Some(99);
        assert_eq!(visible, seeded.snapshot_json_like());
        assert_ne!(digest, root_fingerprint(seeded));
    }

    #[test]
    fn world_fork_fingerprint_preserves_snapshot_sharing_topology() {
        let snapshot = Arc::new(World::new().snapshot());

        let mut shared_gc = GcHeap::new();
        let shared_left = Value::world_fork(&mut shared_gc, Arc::clone(&snapshot));
        let shared_right = Value::world_fork(&mut shared_gc, Arc::clone(&snapshot));
        assert_eq!(shared_left, shared_right);
        let shared = fingerprint_roots(&[shared_left, shared_right]).expect("shared graph fits");

        let mut distinct_gc = GcHeap::new();
        let distinct_left = Value::world_fork(&mut distinct_gc, Arc::new((*snapshot).clone()));
        let distinct_right = Value::world_fork(&mut distinct_gc, Arc::new((*snapshot).clone()));
        assert_ne!(distinct_left, distinct_right);
        let distinct =
            fingerprint_roots(&[distinct_left, distinct_right]).expect("distinct graph fits");

        assert_ne!(shared, distinct);
    }

    #[test]
    fn deep_copy_keeps_world_snapshot_topology() {
        let mut source_gc = GcHeap::new();
        let snapshot = Arc::new(World::new().snapshot());
        let original = Value::world_fork(&mut source_gc, Arc::clone(&snapshot));
        let copy = original.deep_copy(&mut source_gc);
        assert_eq!(original, copy);

        let same_snapshot = fingerprint_roots(&[original, copy]).expect("shared graph fits");
        let distinct = Value::world_fork(&mut source_gc, Arc::new((*snapshot).clone()));
        let distinct_snapshot =
            fingerprint_roots(&[original, distinct]).expect("distinct graph fits");
        assert_ne!(same_snapshot, distinct_snapshot);
    }

    #[test]
    fn shared_world_snapshot_dag_is_fingerprinted_by_distinct_nodes() {
        let mut gc = GcHeap::new();
        let mut snapshot = Arc::new(World::new().snapshot());
        for depth in 0..18 {
            let left = Value::world_fork(&mut gc, Arc::clone(&snapshot));
            let right = Value::world_fork(&mut gc, Arc::clone(&snapshot));
            let mut next = World::new().snapshot();
            next.events = Arc::new(vec![
                (format!("left-{depth}"), left, depth * 2),
                (format!("right-{depth}"), right, depth * 2 + 1),
            ]);
            snapshot = Arc::new(next);
        }
        let root = Value::world_fork(&mut gc, snapshot);
        let limits = FingerprintLimits {
            max_nodes: 128,
            max_worlds: 32,
            max_edges: 256,
            max_pending: 128,
            max_encoded_bytes: 1024 * 1024,
        };
        fingerprint_roots_with_limits(&[root], limits)
            .expect("shared DAG work should be linear in distinct snapshots");
    }

    #[test]
    fn deep_world_snapshot_chain_returns_a_typed_limit_error() {
        let mut gc = GcHeap::new();
        let mut snapshot = Arc::new(World::new().snapshot());
        for depth in 0..10_000 {
            let parent = Value::world_fork(&mut gc, snapshot);
            let mut next = World::new().snapshot();
            next.events = Arc::new(vec![(format!("depth-{depth}"), parent, depth)]);
            snapshot = Arc::new(next);
        }
        let root = Value::world_fork(&mut gc, snapshot);
        let limits = FingerprintLimits {
            max_nodes: 1_000,
            max_worlds: 32,
            max_edges: 256,
            max_pending: 1_000,
            max_encoded_bytes: 1024 * 1024,
        };
        assert_eq!(
            fingerprint_roots_with_limits(&[root], limits),
            Err(FingerprintError::LimitExceeded {
                resource: "world-snapshot",
                limit: 32,
            })
        );
    }
}
