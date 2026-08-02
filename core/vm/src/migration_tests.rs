//! Schema migration tests (list item #5): `save_world()` / `load_world()`
//! and the `migrate X(old) { … }` declaration.

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::value::{Builtin, Value};
use crate::vm::VM;

fn run_vm(src: &str) -> VM {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(
        parser.errors().is_empty(),
        "parse errors: {:?}",
        parser.errors()
    );
    let result = Compiler::new().compile(&program).expect("compile");
    let mut vm = VM::new();
    vm.suppress_output();
    vm.load_compile_result(result);
    vm.run(0).expect("run");
    vm
}

/// Run `src`, then return its world serialized by `save_world()`.
fn save_of(src: &str) -> String {
    let mut vm = run_vm(src);
    let v = vm
        .call_builtin(Builtin::SaveWorld, vec![])
        .expect("save_world");
    v.as_str().expect("save_world returns str").to_string()
}

/// Run `src` (the new program version), then `load_world(json)` into it.
fn load_into(src: &str, json: &str) -> Result<VM, String> {
    let mut vm = run_vm(src);
    let jv = Value::from_string(vm.gc_mut(), json.to_string());
    vm.call_builtin(Builtin::LoadWorld, vec![jv])?;
    Ok(vm)
}

/// Like `load_into` but the load is expected to fail; returns the error.
fn load_err(src: &str, json: &str) -> String {
    match load_into(src, json) {
        Ok(_) => panic!("load_world was expected to fail"),
        Err(e) => e,
    }
}

/// Like `run_vm` but through the checker (as `rad file.rad` compiles), so
/// declared field types reach the VM — the deserialization boundary
/// validates loaded values against them.
fn run_vm_checked(src: &str) -> VM {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(
        parser.errors().is_empty(),
        "parse errors: {:?}",
        parser.errors()
    );
    let mut checker = crate::checker::Checker::new();
    let errors = checker.check(&program);
    assert!(errors.is_empty(), "check errors: {:?}", errors);
    let result = Compiler::new()
        .with_checker_output(checker.output())
        .compile(&program)
        .expect("compile");
    let mut vm = VM::new();
    vm.suppress_output();
    vm.load_compile_result(result);
    vm.run(0).expect("run");
    vm
}

/// `load_world(json)` into a checker-compiled program.
fn load_into_checked(src: &str, json: &str) -> Result<VM, String> {
    let mut vm = run_vm_checked(src);
    let jv = Value::from_string(vm.gc_mut(), json.to_string());
    vm.call_builtin(Builtin::LoadWorld, vec![jv])?;
    Ok(vm)
}

fn load_err_checked(src: &str, json: &str) -> String {
    match load_into_checked(src, json) {
        Ok(_) => panic!("load_world was expected to fail"),
        Err(e) => e,
    }
}

const V1: &str = r#"
    component Health { hp: 100 }
    component Gold { amount: 0 }
    resource Treasury { gold: 10 }
    let hero = spawn("hero", Health { hp: 73 }, Gold { amount: 5 })
    let mook = spawn(Health { hp: 20 })
    set_resource(Treasury, Treasury { gold: 42 })
"#;

#[test]
fn save_load_roundtrip_is_lossless() {
    let json = save_of(V1);
    let original = run_vm(V1);

    // Same schema: value-identical world after load into a fresh VM.
    // (content_digest is order-sensitive, so compare with the blast-radius
    // diff from #3, which matches per entity + component by value.)
    let vm = load_into(
        r#"
        component Health { hp: 100 }
        component Gold { amount: 0 }
        resource Treasury { gold: 10 }
        "#,
        &json,
    )
    .expect("load");
    // (diff_summary matches archetypes positionally, and the loader builds
    // archetypes in a different order than the original spawns — compare
    // semantically: same entities, names, component values, resources.)
    let wa = original.get_world();
    let wb = vm.get_world();
    assert_eq!(wa.all_entity_ids(), wb.all_entity_ids());
    for eid in wa.all_entity_ids() {
        assert_eq!(wa.entity_name(eid), wb.entity_name(eid), "name of {}", eid);
        let mut ca = wa.components_on_entity(eid);
        let mut cb = wb.components_on_entity(eid);
        ca.sort_by(|x, y| x.type_name.cmp(&y.type_name));
        cb.sort_by(|x, y| x.type_name.cmp(&y.type_name));
        let render = |cs: &[crate::value::ComponentData]| -> Vec<String> {
            cs.iter()
                .map(|c| {
                    let fields: Vec<String> = c
                        .layout
                        .iter()
                        .zip(c.values.iter())
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    format!("{}{{{}}}", c.type_name, fields.join(","))
                })
                .collect()
        };
        assert_eq!(render(&ca), render(&cb), "components of entity {}", eid);
    }

    let eid = vm.get_world().get_entity_by_name("hero").expect("hero");
    let hp = vm.get_world().get_component(eid, "Health").expect("Health");
    assert_eq!(hp.values[0].as_int(), Some(73));
    let gold = vm.get_world().get_component(eid, "Gold").expect("Gold");
    assert_eq!(gold.values[0].as_int(), Some(5));
    assert!(vm
        .get_world()
        .get_component(vm.get_world().get_entity_by_name("hero").unwrap(), "Health")
        .is_some());
    let t = vm.get_world().get_resource("Treasury").expect("Treasury");
    assert_eq!(t.values[0].as_int(), Some(42));
    // The unnamed mook survives too.
    assert_eq!(vm.get_world().all_entity_ids().len(), 2);
}

#[test]
fn load_world_replaces_existing_entities_instead_of_appending() {
    let vm = run_vm(
        r#"
        component Pos { indexed name: "", x: 0 }
        let _saved = spawn(Pos { name: "saved", x: 1 })
        let blob = save_world()
        let _extra = spawn(Pos { name: "extra", x: 2 })

        let n = load_world(blob)
        print(n)
        print(len(entities(Pos)))
        print(lookup(Pos, "name", "saved") |> is_some)
        print(lookup(Pos, "name", "extra") |> is_some)

        let _after = spawn(Pos { name: "after", x: 3 })
        print(len(entities(Pos)))
        print(lookup(Pos, "name", "after") |> is_some)
        "#,
    );

    assert_eq!(
        vm.print_buffer,
        vec!["1", "1", "true", "false", "2", "true"]
    );
}

#[test]
fn field_order_change_loads_without_migration() {
    let json = save_of(
        r#"
        component Pos { x: 1, y: 2 }
        let e = spawn("e", Pos { x: 7, y: 9 })
    "#,
    );
    // Same fields, swapped declaration order: values must follow the names.
    let vm = load_into(
        r#"
        component Pos { y: 2, x: 1 }
        "#,
        &json,
    )
    .expect("load");
    let eid = vm.get_world().get_entity_by_name("e").expect("e");
    let pos = vm.get_world().get_component(eid, "Pos").expect("Pos");
    assert_eq!(pos.layout.as_slice(), ["y", "x"]);
    assert_eq!(pos.values[0].as_int(), Some(9), "y must stay 9");
    assert_eq!(pos.values[1].as_int(), Some(7), "x must stay 7");
}

#[test]
fn added_field_migrates_with_a_migrate_block() {
    let json = save_of(
        r#"
        component Health { hp: 100 }
        let hero = spawn("hero", Health { hp: 60 })
    "#,
    );
    let vm = load_into(
        r#"
        component Health { hp: 100, max_hp: 100 }
        migrate Health(old) {
            return Health { hp: old["hp"], max_hp: old["hp"] * 2 }
        }
        "#,
        &json,
    )
    .expect("load");
    let eid = vm.get_world().get_entity_by_name("hero").expect("hero");
    let hp = vm.get_world().get_component(eid, "Health").expect("Health");
    assert_eq!(hp.layout.as_slice(), ["hp", "max_hp"]);
    assert_eq!(hp.values[0].as_int(), Some(60));
    assert_eq!(hp.values[1].as_int(), Some(120));
}

#[test]
fn renamed_field_migrates() {
    let json = save_of(
        r#"
        component Health { hp: 100 }
        let hero = spawn("hero", Health { hp: 31 })
    "#,
    );
    let vm = load_into(
        r#"
        component Health { points: 0 }
        migrate Health(old) {
            return Health { points: old["hp"] }
        }
        "#,
        &json,
    )
    .expect("load");
    let eid = vm.get_world().get_entity_by_name("hero").expect("hero");
    let hp = vm.get_world().get_component(eid, "Health").expect("Health");
    assert_eq!(hp.values[0].as_int(), Some(31));
}

#[test]
fn shape_drift_without_migration_fails_loudly() {
    let json = save_of(
        r#"
        component Health { hp: 100, armor: 5 }
        let hero = spawn("hero", Health { hp: 60, armor: 1 })
    "#,
    );
    let err = load_err(
        r#"
        component Health { hp: 100, max_hp: 100 }
        "#,
        &json,
    );
    assert!(err.contains("schema of 'Health' changed"), "got: {}", err);
    assert!(err.contains("added: [max_hp]"), "got: {}", err);
    assert!(err.contains("removed: [armor]"), "got: {}", err);
    assert!(err.contains("migrate Health(old)"), "got: {}", err);
}

#[test]
fn resource_migration_runs() {
    let json = save_of(
        r#"
        resource Treasury { gold: 0 }
        set_resource(Treasury, Treasury { gold: 77 })
    "#,
    );
    let vm = load_into(
        r#"
        resource Treasury { gold: 0, vault: "main" }
        migrate Treasury(old) {
            return Treasury { gold: old["gold"], vault: "migrated" }
        }
        "#,
        &json,
    )
    .expect("load");
    let t = vm.get_world().get_resource("Treasury").expect("Treasury");
    assert_eq!(t.values[0].as_int(), Some(77));
    assert_eq!(t.values[1].as_str(), Some("migrated"));
}

#[test]
fn migration_must_return_the_right_component() {
    let json = save_of(
        r#"
        component Health { hp: 100 }
        let hero = spawn("hero", Health { hp: 60 })
    "#,
    );
    // Fallthrough (no return) yields NIL.
    let err = load_err(
        r#"
        component Health { points: 0 }
        migrate Health(old) {
            let _x = old["hp"]
        }
        "#,
        &json,
    );
    assert!(err.contains("must `return Health"), "got: {}", err);
}

#[test]
fn unknown_component_in_save_errors() {
    let json = save_of(
        r#"
        component Legacy { x: 0 }
        let e = spawn("e", Legacy { x: 1 })
    "#,
    );
    let err = load_err(r#"component Other { y: 0 }"#, &json);
    assert!(err.contains("'Legacy'"), "got: {}", err);
    assert!(err.contains("not declared"), "got: {}", err);
}

#[test]
fn loaded_entities_have_causal_provenance() {
    let json = save_of(
        r#"
        component Health { hp: 100 }
        let hero = spawn("hero", Health { hp: 60 })
    "#,
    );
    let vm = load_into(r#"component Health { hp: 100 }"#, &json).expect("load");
    let eid = vm.get_world().get_entity_by_name("hero").expect("hero");
    let why = vm
        .causality_ledger()
        .explain_entity(eid, "Health", u64::MAX);
    assert!(why.contains("spawned in frame 0"), "got: {}", why);
}

#[test]
fn end_to_end_in_language_roundtrip() {
    // The whole flow inside RAD itself: save in v1, load + migrate in v2.
    let json = save_of(
        r#"
        component Score { points: 0 }
        let p = spawn("player", Score { points: 991 })
    "#,
    );
    let dir = std::env::temp_dir().join("rad_migration_p5_test");
    let _ = std::fs::create_dir_all(&dir);
    let save_path = dir.join("world.radw");
    std::fs::write(&save_path, &json).expect("write save");
    let path = save_path.to_string_lossy().replace('\\', "/");

    let v2 = format!(
        r#"
        component Score {{ points: 0, rank: "unranked" }}
        migrate Score(old) {{
            return Score {{ points: old["points"], rank: "veteran" }}
        }}
        let n = load_world(read_file("{path}"))
        print(n)
        let p = get_entity("player")
        let s = get(p, Score) |> unwrap
        print(f"{{s.points}} {{s.rank}}")
        "#
    );
    let vm = run_vm(&v2);
    assert_eq!(vm.print_buffer, vec!["1", "991 veteran"]);
}

// ---------------------------------------------------------------------------
// The deserialization boundary is type-AWARE (dogfood A4 BUG 01/02 and the
// silently-accepted corruption-matrix cases S9/S12/S13/S14 of BUG 03).
// Field-NAME drift was already loud; these lock in that field-TYPE drift,
// wrong-typed `migrate` results, duplicate entity names, and empty entity
// names are refused at every path where persisted rows become components.
// ---------------------------------------------------------------------------

const INCIDENT_DECL: &str = r#"component Incident { code: "", sev: 1, open: true }"#;

/// A4 BUG 01 (= corruption case S10): a save whose field SET matches the
/// declaration exactly but whose value TYPES do not (sev str, open int).
#[test]
fn load_world_rejects_wrong_typed_values_with_matching_field_set() {
    let poisoned = r#"RADWORLD2 {"entities":[["inc-1001",[["Incident",["DISK_FULL","3",0]]]]],"resources":[],"schema":[["Incident",["code","sev","open"]]]}"#;
    let err = load_err_checked(INCIDENT_DECL, poisoned);
    assert!(err.contains("type drift in 'Incident.sev'"), "got: {}", err);
    assert!(err.contains("declared int"), "got: {}", err);
    assert!(err.contains("str"), "got: {}", err);
}

/// A4 BUG 03 case S14: a JSON null in a bool-declared field loaded as a
/// silent nil — the exact thing the docs promise never happens.
#[test]
fn load_world_rejects_null_in_typed_field() {
    let nulled = r#"RADWORLD2 {"entities":[["inc-1001",[["Incident",["DISK_FULL",3,null]]]]],"resources":[],"schema":[["Incident",["code","sev","open"]]]}"#;
    let err = load_err_checked(INCIDENT_DECL, nulled);
    assert!(
        err.contains("type drift in 'Incident.open'"),
        "got: {}",
        err
    );
    assert!(err.contains("declared bool"), "got: {}", err);
    assert!(err.contains("nil"), "got: {}", err);
}

/// A4 BUG 03 case S13: an integer past i64 range parses as a float and
/// used to land silently in an int-declared field (type AND value changed).
#[test]
fn load_world_rejects_out_of_range_integer_in_int_field() {
    let huge = r#"RADWORLD2 {"entities":[["inc-1001",[["Incident",["DISK_FULL",99999999999999999999999999,true]]]]],"resources":[],"schema":[["Incident",["code","sev","open"]]]}"#;
    let err = load_err_checked(INCIDENT_DECL, huge);
    assert!(err.contains("type drift in 'Incident.sev'"), "got: {}", err);
    assert!(err.contains("declared int"), "got: {}", err);
    assert!(err.contains("float"), "got: {}", err);
}

/// A4 BUG 03 case S9: two entities sharing a name used to load as two
/// entities with one silently stripped of its name (unreachable via
/// get_entity, count still "right").
#[test]
fn load_world_rejects_duplicate_entity_names() {
    let dup = r#"RADWORLD2 {"entities":[["inc-1001",[["Incident",["A",1,true]]]],["inc-1001",[["Incident",["B",2,false]]]]],"resources":[],"schema":[["Incident",["code","sev","open"]]]}"#;
    let err = load_err_checked(INCIDENT_DECL, dup);
    assert!(
        err.contains("two entities named 'inc-1001'"),
        "got: {}",
        err
    );
}

/// A4 BUG 03 case S12: an entity name blanked to "" used to load as an
/// unnamed entity. A live world can never hold an empty name (spawn refuses
/// to record one), so the payload is corrupt.
#[test]
fn load_world_rejects_empty_entity_name() {
    let blank = r#"RADWORLD2 {"entities":[["",[["Incident",["A",1,true]]]]],"resources":[],"schema":[["Incident",["code","sev","open"]]]}"#;
    let err = load_err_checked(INCIDENT_DECL, blank);
    assert!(err.contains("entity named \"\""), "got: {}", err);
}

/// A4 BUG 02: the sanctioned path — a `migrate` block that grabs the wrong
/// old key returns a wrong-typed field. `old` is map<str, any>, so the
/// static checker cannot see it; the boundary must.
#[test]
fn migrate_result_with_wrong_typed_field_is_rejected() {
    let json = save_of(
        r#"
        component Incident { code: "", sev: 1, open: true }
        let _i = spawn("inc-1001", Incident { code: "DISK_FULL", sev: 3, open: true })
    "#,
    );
    let err = load_err_checked(
        r#"
        component Incident { code: "", severity: 1, open: true, opened_at: 0 }
        migrate Incident(old) {
            return Incident {
                code: old["code"],
                severity: old["code"],
                open: old["open"],
                opened_at: 0
            }
        }
        "#,
        &json,
    );
    assert!(err.contains("migrate Incident(old)"), "got: {}", err);
    assert!(
        err.contains("type drift in 'Incident.severity'"),
        "got: {}",
        err
    );
    assert!(err.contains("declared int"), "got: {}", err);
}

/// A correct migration through the same checked pipeline still loads — the
/// boundary rejects type drift, not migration itself.
#[test]
fn correct_migration_still_loads_under_type_validation() {
    let json = save_of(
        r#"
        component Incident { code: "", sev: 1, open: true }
        let _i = spawn("inc-1001", Incident { code: "DISK_FULL", sev: 3, open: true })
    "#,
    );
    let vm = load_into_checked(
        r#"
        component Incident { code: "", severity: 1, open: true, opened_at: 0 }
        migrate Incident(old) {
            return Incident {
                code: old["code"],
                severity: old["sev"],
                open: old["open"],
                opened_at: 0
            }
        }
        "#,
        &json,
    )
    .expect("correct migration must load");
    let eid = vm.get_world().get_entity_by_name("inc-1001").expect("e");
    let inc = vm.get_world().get_component(eid, "Incident").expect("c");
    assert_eq!(inc.values[1].as_int(), Some(3), "severity from old sev");
}

/// Wrong-typed resource values are refused too (same choke point).
#[test]
fn load_world_rejects_wrong_typed_resource_value() {
    let poisoned = r#"RADWORLD2 {"entities":[],"resources":[["Ledger",["us-east",2]]],"schema":[["Ledger",["count","site"]]]}"#;
    let err = load_err_checked(r#"resource Ledger { count: 0, site: "" }"#, poisoned);
    assert!(err.contains("type drift in 'Ledger.count'"), "got: {}", err);
    assert!(err.contains("declared int"), "got: {}", err);
}

/// The same hole through fork bytes: a payload with a verified digest but
/// wrong-typed values used to decode into an ingestible fork. The codec now
/// refuses AFTER integrity passes — an integrity digest over corrupt state
/// must not become an Ok fork.
#[test]
fn fork_from_bytes_rejects_wrong_typed_values() {
    let body = r#"{"entities":[[0,"e",[["Pt",["oops"]]]]],"resources":[],"schema":[["Pt",["x"]]],"next_id":1,"free_ids":[]}"#;
    let payload = format!(
        "RADFORK2 {} {}",
        blake3::hash(body.as_bytes()).to_hex(),
        body
    );
    let mut vm = run_vm_checked(r#"component Pt { x: 1 }"#);
    let pv = Value::from_string(vm.gc_mut(), payload);
    let res = vm
        .call_builtin(Builtin::ForkFromBytes, vec![pv])
        .expect("fork_from_bytes returns a Result value");
    let st = res.as_sum_type().expect("Result sum type");
    assert_eq!(st.variant, "Err", "wrong-typed payload must be Err");
    let msg = format!("{}", st.fields["message"]);
    assert!(msg.contains("type drift in 'Pt.x'"), "got: {}", msg);
    assert!(msg.contains("declared int"), "got: {}", msg);
}

/// fork_from_bytes also refuses duplicate and empty entity names (the name
/// maps in a real fork are consistent by construction, so both are corrupt).
#[test]
fn fork_from_bytes_rejects_duplicate_and_empty_names() {
    let mk = |body: &str| {
        format!(
            "RADFORK2 {} {}",
            blake3::hash(body.as_bytes()).to_hex(),
            body
        )
    };
    let mut vm = run_vm_checked(r#"component Pt { x: 1 }"#);

    let dup = mk(
        r#"{"entities":[[0,"e",[["Pt",[1]]]],[1,"e",[["Pt",[2]]]]],"resources":[],"schema":[["Pt",["x"]]],"next_id":2,"free_ids":[]}"#,
    );
    let pv = Value::from_string(vm.gc_mut(), dup);
    let res = vm.call_builtin(Builtin::ForkFromBytes, vec![pv]).unwrap();
    let st = res.as_sum_type().expect("Result");
    assert_eq!(st.variant, "Err");
    let msg = format!("{}", st.fields["message"]);
    assert!(msg.contains("two entities named 'e'"), "got: {}", msg);

    let blank = mk(
        r#"{"entities":[[0,"",[["Pt",[1]]]]],"resources":[],"schema":[["Pt",["x"]]],"next_id":1,"free_ids":[]}"#,
    );
    let pv = Value::from_string(vm.gc_mut(), blank);
    let res = vm.call_builtin(Builtin::ForkFromBytes, vec![pv]).unwrap();
    let st = res.as_sum_type().expect("Result");
    assert_eq!(st.variant, "Err");
    let msg = format!("{}", st.fields["message"]);
    assert!(msg.contains("cannot hold an empty name"), "got: {}", msg);
}

/// The one deliberate coercion the boundary allows: a float-declared field
/// accepts an int (the checker allows that lossless direction at
/// construction, so well-formed worlds legitimately hold ints there). The
/// reverse — a float in an int field — stays refused (S13 above).
#[test]
fn int_in_float_declared_field_round_trips_under_validation() {
    let vm = run_vm_checked(
        r#"
        component Big { x: 0.0 }
        let _e = spawn("e", Big { x: 1 })
        let s = save_world()
        print(load_world(s))
        let b = get(get_entity("e"), Big) |> unwrap
        print(typeof(b.x))
        "#,
    );
    assert_eq!(vm.print_buffer, vec!["1", "int"]);
}

/// Checker-less compiles (replay of embedded trace source) carry no field
/// types, so the boundary stays permissive there — a trace recorded before
/// this fix must still replay.
#[test]
fn checkerless_compile_skips_type_validation() {
    let poisoned = r#"RADWORLD2 {"entities":[["inc-1001",[["Incident",["DISK_FULL","3",0]]]]],"resources":[],"schema":[["Incident",["code","sev","open"]]]}"#;
    let vm = load_into(INCIDENT_DECL, poisoned).expect("bare compile stays permissive");
    assert_eq!(vm.get_world().all_entity_ids().len(), 1);
}

// ---------------------------------------------------------------------------
// Tier-1 #3: the convergence receipt across a schema migration.
//
// `world_digest()` hashes the canonical body, which embeds the schema — so
// a v1 world and its v2-migrated twin digest differently BY CONSTRUCTION,
// and a raw digest comparison during a rolling upgrade reads as "DIVERGED"
// when nothing diverged. The fix is two primitives:
//   - `schema_digest()`: the program's schema fingerprint, so peers can
//     DETECT version skew instead of misreading it as divergence.
//   - `world_digest(fork)`: digest a fork's state without committing it —
//     the v2 side decodes the v1 peer's bytes (migrate-on-ingest shapes
//     them to v2) and digests THAT view; both sides of the comparison now
//     carry the same schema, so equality means logical convergence.
// ---------------------------------------------------------------------------

/// One v1 program, one v2 program (rename + derived field), same logical
/// data. The receipt: v2 certifies convergence by digesting the migrated
/// view of v1's bytes; raw digests differ; schema_digest names why.
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
    // integrity envelope fork_to_bytes already had. A small save stays legacy
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
fn radworld2_legacy_save_still_loads() {
    // The digest-less legacy format is accepted forever (decoders-accept-both).
    let legacy = r#"RADWORLD2 {"entities":[["e",[["Pos",[3,4]]]]],"resources":[],"schema":[["Pos",["x","y"]]]}"#;
    let vm = load_into("component Pos { x: 0, y: 0 }", legacy)
        .expect("legacy RADWORLD2 must still load");
    assert_eq!(vm.get_world().all_entity_ids().len(), 1);
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

// === Declared schema versions + `migrate X(old, from_version)` (dogfood seq 69 IDEA 03) ===

/// Field value of a component by name (layout-order independent).
fn field_of(data: &crate::value::ComponentData, name: &str) -> Value {
    let i = data
        .layout
        .iter()
        .position(|f| f == name)
        .unwrap_or_else(|| panic!("no field '{}' in {:?}", name, data.layout));
    data.values[i]
}

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
