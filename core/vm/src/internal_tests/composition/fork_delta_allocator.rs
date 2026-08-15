// Fork-delta allocator regressions. These histories are intentionally kept
// separate from the broader causality composition suite: they exercise the
// exact final live/free/retired partition carried by RADDELTA, including
// identity history that leaves no final entity row.

#[test]
fn delta_roundtrips_spawn_then_despawn_without_a_final_upsert() {
    let vm = run(
        r#"
        component Mark { value: 0 }
        let base = fork()
        let transient = spawn("transient", Mark { value: 1 })
        despawn(transient)
        let expected = fork()
        let delta = fork_delta(base, expected)
        match fork_apply(base, delta) {
            Ok(restored) => {
                print(fork_to_bytes(expected))
                print(fork_to_bytes(restored))
            }
            Err(message) => { print("apply failed: " + message) }
        }
    "#,
    );

    assert_eq!(vm.print_buffer.len(), 2, "got: {:?}", vm.print_buffer);
    assert_eq!(
        wire_state_part(&vm.print_buffer[0]),
        wire_state_part(&vm.print_buffer[1]),
        "transient allocation history must survive without a final upsert"
    );
}

#[test]
fn delta_roundtrips_multiple_transient_spawns() {
    let vm = run(
        r#"
        component Mark { value: 0 }
        let base = fork()
        let a = spawn("a", Mark { value: 1 })
        let b = spawn("b", Mark { value: 2 })
        let c = spawn("c", Mark { value: 3 })
        despawn(a)
        despawn(b)
        despawn(c)
        let expected = fork()
        let restored = fork_apply(base, fork_delta(base, expected)) |> unwrap
        print(fork_to_bytes(expected))
        print(fork_to_bytes(restored))
    "#,
    );

    assert_eq!(
        wire_state_part(&vm.print_buffer[0]),
        wire_state_part(&vm.print_buffer[1]),
        "all transient identities must be represented by the final allocator partition"
    );
}

#[test]
fn delta_roundtrips_reuse_generation_advance_then_despawn() {
    let vm = run(
        r#"
        component Mark { value: 0 }
        let first = spawn("first", Mark { value: 1 })
        despawn(first)
        let base = fork()

        let reused = spawn("reused", Mark { value: 2 })
        despawn(reused)
        let expected = fork()
        let restored = fork_apply(base, fork_delta(base, expected)) |> unwrap
        print(fork_to_bytes(expected))
        print(fork_to_bytes(restored))
    "#,
    );

    assert_eq!(
        wire_state_part(&vm.print_buffer[0]),
        wire_state_part(&vm.print_buffer[1]),
        "generation advancement without a final row must survive the delta"
    );
}

#[test]
fn malformed_delta_allocator_rejects_without_mutating_the_vm() {
    let producer = run(
        r#"
        component Mark { value: 0 }
        let base = fork()
        let transient = spawn("transient", Mark { value: 1 })
        despawn(transient)
        print(fork_delta(base, fork()))
    "#,
    );
    let plain = crate::radpack::open(&producer.print_buffer[0]).expect("valid generated delta");
    let rest = plain
        .strip_prefix("RADDELTA1 ")
        .expect("generated delta tag");
    let (_, body_text) = rest.split_once(' ').expect("generated delta header");
    let mut body: serde_json::Value =
        serde_json::from_str(body_text).expect("generated delta body");
    body["entity_allocator"][2] = serde_json::json!([]);
    let mutant = crate::radpack::seal(
        "RADDELTA1",
        &serde_json::to_string(&body).expect("mutated delta body"),
    );

    let consumer = run(&format!(
        r#"
        component Mark {{ value: 0 }}
        let base = fork()
        let before = world_digest()
        match fork_apply(base, {mutant:?}) {{
            Ok(_) => {{ print("accepted malformed allocator") }}
            Err(message) => {{ print(message) }}
        }}
        print(str(before == world_digest()))
        print(spawn("still-usable", Mark {{ value: 9 }}))
    "#
    ));

    assert!(
        consumer.print_buffer[0].contains("canonical partition"),
        "got: {}",
        consumer.print_buffer[0]
    );
    assert_eq!(consumer.print_buffer[1], "true");
    assert_eq!(consumer.print_buffer[2], "0");
}
