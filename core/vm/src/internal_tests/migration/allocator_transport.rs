fn allocator_transport_body(
    entities: serde_json::Value,
    free_ids: serde_json::Value,
    next_id: u32,
    exhausted: bool,
    generations: serde_json::Value,
) -> String {
    serde_json::json!({
        "entities": entities,
        "events": [],
        "delayed": [],
        "entity_allocator": [next_id, exhausted, free_ids, generations],
        "relations": serde_json::Value::Null,
        "resources": [],
        "schema": [],
    })
    .to_string()
}

fn allocator_transport_payload(tag: &str, body: &str) -> String {
    crate::radpack::seal(tag, body)
}

fn decode_fork_payload(
    vm: &mut VM,
    payload: String,
) -> Result<crate::world::WorldSnapshot, String> {
    let payload = Value::from_string(vm.gc_mut(), payload);
    let result = vm.call_builtin(Builtin::ForkFromBytes, vec![payload])?;
    let result = result
        .as_sum_type()
        .ok_or_else(|| "fork_from_bytes did not return Result".to_string())?;
    if result.variant == "Err" {
        return Err(format!("{}", result.fields["message"]));
    }
    result.fields["value"]
        .as_world_fork()
        .map(|snapshot| (**snapshot).clone())
        .ok_or_else(|| "fork_from_bytes returned a non-fork value".to_string())
}

fn decode_fork_result(vm: &mut VM, body: &str) -> Result<crate::world::WorldSnapshot, String> {
    decode_fork_payload(vm, allocator_transport_payload("RADFORK2", body))
}

fn load_world_result(vm: &mut VM, body: &str) -> Result<(), String> {
    let payload = allocator_transport_payload("RADWORLD3", body);
    let payload = Value::from_string(vm.gc_mut(), payload);
    vm.call_builtin(Builtin::LoadWorld, vec![payload])?;
    Ok(())
}

fn assert_allocator_transport_rejected(body: &str, expected: &str) {
    let mut world_vm = run_vm("let _ready = 1");
    let before = world_vm.attempt_checkpoint_digest().unwrap();
    let error = load_world_result(&mut world_vm, body).expect_err("world allocator must reject");
    assert!(error.contains(expected), "got: {error}");
    assert_eq!(world_vm.attempt_checkpoint_digest().unwrap(), before);

    let mut fork_vm = run_vm("let _ready = 1");
    let before = fork_vm.attempt_checkpoint_digest().unwrap();
    let error = match decode_fork_result(&mut fork_vm, body) {
        Ok(_) => panic!("fork allocator must reject"),
        Err(error) => error,
    };
    assert!(error.contains(expected), "got: {error}");
    assert_eq!(fork_vm.attempt_checkpoint_digest().unwrap(), before);
}

#[test]
fn allocator_transport_rejects_duplicate_free_ids_before_mutation() {
    let body = allocator_transport_body(
        serde_json::json!([[0, null, []]]),
        serde_json::json!([1, 1]),
        2,
        false,
        serde_json::json!([]),
    );

    assert_allocator_transport_rejected(&body, "duplicate free");

    // A rejected allocator is not partially installed: both identities are
    // still fresh and can be allocated exactly once afterward.
    let mut vm = run_vm("let _ready = 1");
    assert!(load_world_result(&mut vm, &body).is_err());
    assert_eq!(vm.get_world_mut().spawn_entity(None), Ok(0));
    assert_eq!(vm.get_world_mut().spawn_entity(None), Ok(1));
}

#[test]
fn allocator_transport_rejects_every_noncanonical_partition_shape() {
    let cases = [
        (
            "duplicate live",
            allocator_transport_body(
                serde_json::json!([[0, null, []], [0, null, []]]),
                serde_json::json!([]),
                1,
                false,
                serde_json::json!([]),
            ),
        ),
        (
            "live entity IDs are not strictly ascending",
            allocator_transport_body(
                serde_json::json!([[1, null, []], [0, null, []]]),
                serde_json::json!([]),
                2,
                false,
                serde_json::json!([]),
            ),
        ),
        (
            "free entity IDs are not strictly ascending",
            allocator_transport_body(
                serde_json::json!([[0, null, []]]),
                serde_json::json!([2, 1]),
                3,
                false,
                serde_json::json!([]),
            ),
        ),
        (
            "duplicate generation slot",
            allocator_transport_body(
                serde_json::json!([[0, null, []]]),
                serde_json::json!([]),
                1,
                false,
                serde_json::json!([[0, 1], [0, 2]]),
            ),
        ),
        (
            "generation slots are not strictly ascending",
            allocator_transport_body(
                serde_json::json!([[0, null, []], [1, null, []]]),
                serde_json::json!([]),
                2,
                false,
                serde_json::json!([[1, 1], [0, 1]]),
            ),
        ),
        (
            "both live and free",
            allocator_transport_body(
                serde_json::json!([[0, null, []]]),
                serde_json::json!([0]),
                1,
                false,
                serde_json::json!([]),
            ),
        ),
        (
            "canonical partition accounts for 1",
            allocator_transport_body(
                serde_json::json!([[0, null, []]]),
                serde_json::json!([]),
                2,
                false,
                serde_json::json!([]),
            ),
        ),
        (
            "free entity ID 1 is outside",
            allocator_transport_body(
                serde_json::json!([[0, null, []]]),
                serde_json::json!([1]),
                1,
                false,
                serde_json::json!([]),
            ),
        ),
        (
            "stores noncanonical zero",
            allocator_transport_body(
                serde_json::json!([[0, null, []]]),
                serde_json::json!([]),
                1,
                false,
                serde_json::json!([[0, 0]]),
            ),
        ),
        (
            "exhausted fresh identity space requires",
            allocator_transport_body(
                serde_json::json!([[0, null, []]]),
                serde_json::json!([]),
                1,
                true,
                serde_json::json!([]),
            ),
        ),
    ];

    for (expected, body) in cases {
        assert_allocator_transport_rejected(&body, expected);
    }
}

#[test]
fn allocator_transport_accepts_a_canonical_retired_slot() {
    let body = allocator_transport_body(
        serde_json::json!([[1, null, []]]),
        serde_json::json!([]),
        2,
        false,
        serde_json::json!([[0, u32::MAX]]),
    );

    let mut world_vm = run_vm("let _ready = 1");
    load_world_result(&mut world_vm, &body)
        .expect("retired slot is valid allocator state");
    assert_eq!(world_vm.get_world_mut().spawn_entity(None), Ok(2));

    let mut fork_vm = run_vm("let _ready = 1");
    let snapshot = decode_fork_result(&mut fork_vm, &body).expect("retired fork slot is valid");
    let mut fork_world = crate::world::World::new();
    fork_world.restore(snapshot);
    assert_eq!(fork_world.spawn_entity(None), Ok(2));
}

#[test]
fn mixed_live_free_and_retired_partition_round_trips_exactly() {
    let generations = vec![(0, u32::MAX), (1, 7), (2, u32::MAX), (3, 9), (4, 4)];
    let body = allocator_transport_body(
        serde_json::json!([[1, "one", []], [4, "four", []]]),
        serde_json::json!([3]),
        5,
        false,
        serde_json::json!(generations),
    );

    let mut world_vm = run_vm("let _ready = 1");
    load_world_result(&mut world_vm, &body).expect("mixed allocator partition loads");
    assert_eq!(world_vm.get_world().allocator_state(), (5, false, vec![3]));
    assert_eq!(world_vm.get_world().generation_entries(), generations);

    let saved = world_vm
        .call_builtin(Builtin::SaveWorld, Vec::new())
        .expect("mixed allocator partition saves")
        .as_str()
        .unwrap()
        .to_string();
    let mut restored_vm = run_vm("let _ready = 1");
    let saved = Value::from_string(restored_vm.gc_mut(), saved);
    restored_vm
        .call_builtin(Builtin::LoadWorld, vec![saved])
        .expect("saved allocator partition reloads");
    assert_eq!(restored_vm.get_world().allocator_state(), (5, false, vec![3]));
    assert_eq!(restored_vm.get_world().generation_entries(), generations);
    assert_eq!(restored_vm.get_world_mut().spawn_entity(None), Ok(3));
    assert_eq!(restored_vm.get_world().entity_ref(3).unwrap().generation, 10);
    assert_eq!(restored_vm.get_world_mut().spawn_entity(None), Ok(5));

    let mut fork_vm = run_vm("let _ready = 1");
    let snapshot = decode_fork_result(&mut fork_vm, &body).expect("mixed fork partition loads");
    let fork = Value::world_fork(fork_vm.gc_mut(), std::sync::Arc::new(snapshot));
    let encoded = fork_vm
        .call_builtin(Builtin::ForkToBytes, vec![fork])
        .expect("mixed fork partition re-encodes")
        .as_str()
        .unwrap()
        .to_string();
    let snapshot = decode_fork_payload(&mut fork_vm, encoded).expect("re-encoded fork loads");
    let mut fork_world = crate::world::World::new();
    fork_world.restore(snapshot);
    assert_eq!(fork_world.allocator_state(), (5, false, vec![3]));
    assert_eq!(fork_world.generation_entries(), generations);
    assert_eq!(fork_world.spawn_entity(None), Ok(3));
    assert_eq!(fork_world.entity_ref(3).unwrap().generation, 10);
    assert_eq!(fork_world.spawn_entity(None), Ok(5));
}

#[test]
fn maximum_generation_free_slot_is_rejected_as_noncanonical() {
    let body = allocator_transport_body(
        serde_json::json!([[1, null, []]]),
        serde_json::json!([0]),
        2,
        false,
        serde_json::json!([[0, u32::MAX]]),
    );

    assert_allocator_transport_rejected(&body, "must be retired, not free");
}
