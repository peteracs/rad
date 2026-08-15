// Declared schema versions and persistence identity across migrations.

#[test]
fn migrate_receives_the_saves_declared_version() {
    // gen1 declares `component Incident v1`; gen2's two-param migrate block
    // binds that 1 from the SAVE — a fact, where shape-sniffing was a guess.
    let save = save_of(
        r#"
        component Incident v1 { sev: 3 }
        let e = spawn("e", Incident { sev: 3 })
        "#,
    );
    assert!(
        save.contains("[\"Incident\",[\"sev\"],1]"),
        "schema entry should carry the declared version, save: {}",
        save
    );
    let vm = load_into(
        r#"
        component Incident v2 { severity: 0, migrated_from: 0 }
        migrate Incident(old, from_version) {
            return Incident { severity: old["sev"], migrated_from: from_version }
        }
        "#,
        &save,
    )
    .expect("load");
    let eid = vm.get_world().get_entity_by_name("e").expect("e");
    let inc = vm
        .get_world()
        .get_component(eid, "Incident")
        .expect("Incident");
    assert_eq!(field_of(&inc, "severity").as_int(), Some(3));
    assert_eq!(
        field_of(&inc, "migrated_from").as_int(),
        Some(1),
        "from_version must be the save's declared version"
    );
}

#[test]
fn versionless_save_migrates_with_from_version_zero() {
    // A save whose program declared no version hands 0 to the block — the
    // documented "no declared version" value, distinguishable from any real
    // generation (versions start at 1).
    let save = save_of(
        r#"
        component Incident { sev: 5 }
        let e = spawn("e", Incident { sev: 5 })
        "#,
    );
    assert!(
        save.contains("[\"Incident\",[\"sev\"]]"),
        "versionless schema entry must stay two-element (byte-stable saves), save: {}",
        save
    );
    let vm = load_into(
        r#"
        component Incident v2 { severity: 0, migrated_from: 9 }
        migrate Incident(old, from_version) {
            return Incident { severity: old["sev"], migrated_from: from_version }
        }
        "#,
        &save,
    )
    .expect("load");
    let eid = vm.get_world().get_entity_by_name("e").expect("e");
    let inc = vm
        .get_world()
        .get_component(eid, "Incident")
        .expect("Incident");
    assert_eq!(field_of(&inc, "severity").as_int(), Some(5));
    assert_eq!(field_of(&inc, "migrated_from").as_int(), Some(0));
}

#[test]
fn one_param_migrate_still_works_with_versioned_save() {
    // The second parameter is OPTIONAL: an existing `migrate X(old)` block
    // keeps working even when the save carries a version.
    let save = save_of(
        r#"
        component Incident v1 { sev: 7 }
        let e = spawn("e", Incident { sev: 7 })
        "#,
    );
    let vm = load_into(
        r#"
        component Incident v2 { severity: 0 }
        migrate Incident(old) {
            return Incident { severity: old["sev"] }
        }
        "#,
        &save,
    )
    .expect("load");
    let eid = vm.get_world().get_entity_by_name("e").expect("e");
    let inc = vm
        .get_world()
        .get_component(eid, "Incident")
        .expect("Incident");
    assert_eq!(field_of(&inc, "severity").as_int(), Some(7));
}

#[test]
fn version_tag_does_not_change_world_digest() {
    // The version tag is load metadata, not state: two programs identical
    // except for the tag digest identically, so a rolling upgrade can still
    // certify state convergence across peers of different schema vintages.
    let mut a = run_vm("component P { x: 1 }\nlet e = spawn(\"e\", P { x: 1 })");
    let mut b = run_vm("component P v3 { x: 1 }\nlet e = spawn(\"e\", P { x: 1 })");
    let da = a
        .call_builtin(Builtin::WorldDigest, vec![])
        .expect("digest a");
    let db = b
        .call_builtin(Builtin::WorldDigest, vec![])
        .expect("digest b");
    assert_eq!(
        da.as_str().unwrap(),
        db.as_str().unwrap(),
        "a version re-tag must not move world_digest()"
    );
    // Resources take the tag too.
    let save =
        save_of("resource Cfg v4 { rate: 2 }\ncomponent P { x: 1 }\nlet e = spawn(P { x: 1 })");
    assert!(
        save.contains("[\"Cfg\",[\"rate\"],4]"),
        "resource schema entry should carry the version, save: {}",
        save
    );
}

// A v1 world and its v2-migrated twin digest differently by construction.
// `schema_digest()` identifies that version skew, while `world_digest(fork)`
// certifies the receiver's migrated view without committing it.
#[test]
fn cross_version_convergence_certified_via_migrated_view() {
    // v1: produces its wire bytes and its own digests.
    let v1 = run_vm(
        r#"
        component Acct { owner: "", bal: 0 }
        let _a = spawn("a1", Acct { owner: "alice", bal: 10 })
        let _b = spawn("a2", Acct { owner: "bob", bal: 20 })
        print(fork_to_bytes(fork()))
        print(world_digest())
        print(schema_digest())
    "#,
    );
    let v1_bytes = v1.print_buffer[0].clone();
    let v1_world_digest = v1.print_buffer[1].clone();
    let v1_schema_digest = v1.print_buffer[2].clone();

    // v2: `owner` renamed to `holder`, `tier` derived. It ingests v1's
    // bytes (migrating them) and certifies against a NATIVELY-built world
    // holding the same logical data.
    let v2_src = format!(
        r#"
        component Acct {{ holder: "", bal: 0, tier: "basic" }}
        migrate Acct(old) {{
            return Acct {{ holder: old["owner"], bal: old["bal"], tier: "basic" }}
        }}
        // native v2 twin of v1's data, built independently
        let _a = spawn("a1", Acct {{ holder: "alice", bal: 10 }})
        let _b = spawn("a2", Acct {{ holder: "bob", bal: 20 }})

        let theirs = fork_from_bytes({v1_bytes:?}) |> unwrap
        print(world_digest(theirs))   // the migrated view of v1's world
        print(world_digest())          // our native world
        print(schema_digest())
    "#
    );
    let v2 = run_vm(&v2_src);
    let migrated_view = &v2.print_buffer[0];
    let native_v2 = &v2.print_buffer[1];
    let v2_schema_digest = &v2.print_buffer[2];

    // The certification: same logical data converges THROUGH the migration.
    assert_eq!(
        migrated_view, native_v2,
        "migrated view of v1 must digest equal to the native v2 twin"
    );
    // The honesty: raw digests differ across versions, and schema_digest
    // tells the two sides why.
    assert_ne!(
        &v1_world_digest, native_v2,
        "raw digests differ by construction"
    );
    assert_ne!(
        &v1_schema_digest, v2_schema_digest,
        "schema fingerprints must detect the version skew"
    );
}

/// `world_digest(fork)` agrees with `world_digest()` for a same-schema
/// fork of the live world, ignores in-flight events (state-only), and
/// inspecting a fork never mutates the live world.
#[test]
fn world_digest_of_fork_semantics() {
    let vm = run_vm(
        r#"
        component Gold { amount: 0 }
        event Ping { n: int }
        let hero = spawn("hero", Gold { amount: 7 })
        let here = fork()
        print(world_digest(here) == world_digest())   // same state, same digest

        emit Ping { n: 1 }                            // in-flight event:
        print(world_digest(fork()) == world_digest()) // state-only, still equal

        set(hero, Gold { amount: 8 })
        let later = fork()
        print(world_digest(later) == world_digest(here))  // diverged forks differ
        // peeking at an old fork's digest must not rewind the live world
        let _ = world_digest(here)
        let g = get(hero, Gold) |> unwrap
        print(g.amount)
    "#,
    );
    assert_eq!(vm.print_buffer, vec!["true", "true", "false", "8"]);
}

/// schema_digest is a fingerprint of the DECLARATIONS: stable across
/// world contents, changed by any layout change.
#[test]
fn schema_digest_tracks_declarations_not_data() {
    let a = run_vm(
        r#"
        component Acct { owner: "", bal: 0 }
        print(schema_digest())
        let _x = spawn("x", Acct { owner: "zoe", bal: 999 })
        print(schema_digest())
    "#,
    );
    // data doesn't move it
    assert_eq!(a.print_buffer[0], a.print_buffer[1]);

    // an added field moves it
    let b = run_vm(
        r#"
        component Acct { owner: "", bal: 0, tier: "" }
        print(schema_digest())
    "#,
    );
    assert_ne!(a.print_buffer[0], b.print_buffer[0]);

    // a renamed field moves it
    let c = run_vm(
        r#"
        component Acct { holder: "", bal: 0 }
        print(schema_digest())
    "#,
    );
    assert_ne!(a.print_buffer[0], c.print_buffer[0]);
}

/// The deeper migration honesty check: a DERIVED field whose migrate
/// output depends on per-row data still certifies, because certification
/// digests the post-migration view (not a lossy projection).
#[test]
fn certification_covers_derived_fields() {
    let v1 = run_vm(
        r#"
        component Tk { pri: 0 }
        let _a = spawn("t1", Tk { pri: 1 })
        let _b = spawn("t2", Tk { pri: 4 })
        print(fork_to_bytes(fork()))
    "#,
    );
    let bytes = v1.print_buffer[0].clone();

    let v2_src = format!(
        r#"
        component Tk {{ pri: 0, est: 0 }}
        migrate Tk(old) {{
            return Tk {{ pri: old["pri"], est: old["pri"] * 10 }}
        }}
        let _a = spawn("t1", Tk {{ pri: 1, est: 10 }})
        let _b = spawn("t2", Tk {{ pri: 4, est: 40 }})
        let theirs = fork_from_bytes({bytes:?}) |> unwrap
        print(world_digest(theirs) == world_digest())
    "#
    );
    let v2 = run_vm(&v2_src);
    assert_eq!(v2.print_buffer, vec!["true"]);

    // and a native twin with DIFFERENT derived values does NOT certify
    let v2_wrong = format!(
        r#"
        component Tk {{ pri: 0, est: 0 }}
        migrate Tk(old) {{
            return Tk {{ pri: old["pri"], est: old["pri"] * 10 }}
        }}
        let _a = spawn("t1", Tk {{ pri: 1, est: 10 }})
        let _b = spawn("t2", Tk {{ pri: 4, est: 41 }})
        let theirs = fork_from_bytes({bytes:?}) |> unwrap
        print(world_digest(theirs) == world_digest())
    "#
    );
    let vw = run_vm(&v2_wrong);
    assert_eq!(vw.print_buffer, vec!["false"]);
}

// === Persistence integrity envelope + fallible load (dogfood feature seq 69) ===

#[test]
fn save_world_carries_integrity_digest_and_roundtrips() {
    // IDEA 01: save_world now emits `RADWORLD3 <blake3-of-body> <body>` — the
    // integrity envelope fork_to_bytes already had. A small save stays plain
    // text so the digest is inspectable.
    let blob = save_of(
        r#"
        component Pos { x: 0, y: 0 }
        let _e = spawn("e", Pos { x: 3, y: 4 })
        "#,
    );
    let rest = blob
        .strip_prefix("RADWORLD3 ")
        .expect("save must carry the RADWORLD3 envelope");
    let (digest, body) = rest.split_once(' ').expect("digest then body");
    assert_eq!(
        digest,
        blake3::hash(body.as_bytes()).to_hex().as_str(),
        "the envelope digest must be blake3 of the body"
    );
    let vm = load_into("component Pos { x: 0, y: 0 }", &blob).expect("load ok");
    assert_eq!(vm.get_world().all_entity_ids().len(), 1);
}

#[test]
fn tampered_save_body_is_rejected() {
    // The point of the envelope: a mutated body with a stale digest no longer
    // loads — the silent-corruption class A4 measured is closed.
    let blob = save_of(
        r#"
        component Pos { x: 0, y: 0 }
        let _e = spawn("e", Pos { x: 3, y: 4 })
        "#,
    );
    let rest = blob.strip_prefix("RADWORLD3 ").unwrap();
    let (digest, body) = rest.split_once(' ').unwrap();
    let mutated = body.replacen("Pos", "Qos", 1);
    assert_ne!(mutated, body, "test setup: body must contain 'Pos'");
    let tampered = format!("RADWORLD3 {} {}", digest, mutated);
    let err = load_err("component Pos { x: 0, y: 0 }", &tampered);
    assert!(
        err.contains("integrity digest mismatch"),
        "tampered save must be refused, got: {}",
        err
    );
}

#[test]
fn unsupported_pre_release_save_formats_are_rejected() {
    let obsolete = r#"RADWORLD2 {"entities":[],"resources":[],"schema":[]}"#;
    let error = match load_into("component Pos { x: 0, y: 0 }", obsolete) {
        Ok(_) => panic!("unsupported pre-release formats must not become compatibility debt"),
        Err(error) => error,
    };
    assert!(error.contains("expected RADWORLD3"), "got: {error}");
}

#[test]
fn try_load_world_returns_ok_on_valid_save() {
    // IDEA 02: the fallible sibling returns a Result instead of aborting.
    let blob = save_of("component Pos { x: 0 }\nlet _e = spawn(\"e\", Pos { x: 5 })");
    let mut vm = run_vm("component Pos { x: 0 }");
    let jv = Value::from_string(vm.gc_mut(), blob);
    let r = vm
        .call_builtin(Builtin::TryLoadWorld, vec![jv])
        .expect("try_load_world must not abort");
    let st = r.as_sum_type().expect("try_load_world returns Result");
    assert_eq!(st.variant, "Ok");
    assert_eq!(vm.get_world().all_entity_ids().len(), 1);
}

#[test]
fn try_load_world_returns_err_without_aborting_and_preserves_world() {
    // A corrupt save comes back as Err, does NOT kill the process, and leaves
    // the live world untouched — the fall-back-to-yesterday's-backup property.
    let mut vm = run_vm("component Pos { x: 0 }\nlet _live = spawn(\"live\", Pos { x: 1 })");
    let before = vm.get_world().all_entity_ids().len();
    let garbage = Value::from_string(
        vm.gc_mut(),
        "RADWORLD3 0000000000000000000000000000000000000000000000000000000000000000 {\"broken\":"
            .to_string(),
    );
    let r = vm
        .call_builtin(Builtin::TryLoadWorld, vec![garbage])
        .expect("try_load_world must not abort");
    let st = r.as_sum_type().expect("try_load_world returns Result");
    assert_eq!(st.variant, "Err");
    assert_eq!(
        vm.get_world().all_entity_ids().len(),
        before,
        "a failed load must not touch the live world"
    );
}
