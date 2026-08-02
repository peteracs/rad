//! Indexed-query battle tests (Tier-1 #1): `indexed` fields, `lookup`,
//! `lookup_all`, and — most importantly — index SURVIVAL across every world
//! boundary the language has: set/update/remove/despawn, id reuse,
//! fork/commit, the wire codec, saves, deltas, merges, migration, GC
//! pressure, and replay determinism.
//!
//! The founding bug (found by this suite's recon, fixed alongside it):
//! `fork_from_bytes` built its world with empty index declarations, so
//! `commit()` of any wire-ingested fork silently WIPED the live world's
//! indexes — every RADTRACK client lost `lookup()` the moment it pulled.
//! Indexes are now seeded into decode worlds and reconciled at commit
//! (the program's declarations are the source of truth; snapshots carry
//! only derived state).

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::vm::VM;

fn compile(src: &str) -> crate::compiler::CompileResult {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(
        parser.errors().is_empty(),
        "parse errors: {:?}",
        parser.errors()
    );
    // `indexed` declarations flow compiler-ward through the CHECKER's
    // component table, with an AST-derived fallback for checker-less
    // compiles (see replay_carries_indexes_without_checker_pass below —
    // the fallback's absence aborted `rad replay` of any indexed program).
    let mut checker = crate::checker::Checker::new();
    let errors = checker.check(&program);
    assert!(errors.is_empty(), "check errors: {:?}", errors);
    Compiler::new()
        .with_checker_output(checker.output())
        .compile(&program)
        .expect("compile")
}

fn run(src: &str) -> VM {
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(7);
    vm.load_compile_result(compile(src));
    vm.run(0).expect("run");
    vm
}

fn run_err(src: &str) -> String {
    let mut vm = VM::new();
    vm.suppress_output();
    vm.load_compile_result(compile(src));
    vm.run(0).expect_err("expected a runtime error")
}

const DECLS: &str = r#"
component User { indexed name: "", indexed level: 0, hp: 100 }
component Tag { indexed label: "" }
"#;

// ---------------------------------------------------------------------------
// basics
// ---------------------------------------------------------------------------

#[test]
fn lookup_hit_miss_and_lookup_all_sorted() {
    let vm = run(&format!(
        r#"{DECLS}
        let _a = spawn(User {{ name: "alice", level: 3, hp: 10 }})
        let _b = spawn(User {{ name: "bob", level: 3, hp: 20 }})
        let _c = spawn(User {{ name: "carol", level: 9, hp: 30 }})
        match lookup(User, "name", "bob") {{
            Some(e) => {{ print(f"hit {{(get(e, User) |> unwrap).hp}}") }}
            None => {{ print("miss") }}
        }}
        match lookup(User, "name", "nobody") {{
            Some(_) => {{ print("hit?!") }}
            None => {{ print("miss") }}
        }}
        // multi-match: ids ascending, regardless of spawn order
        let l3 = lookup_all(User, "level", 3)
        print(len(l3))
        let hps = map(l3, fn(e) {{ return (get(e, User) |> unwrap).hp }})
        print(hps)
        print(len(lookup_all(User, "level", 99)))
    "#
    ));
    assert_eq!(
        vm.print_buffer,
        vec!["hit 20", "miss", "2", "[10, 20]", "0"]
    );
}

#[test]
fn duplicate_keys_lookup_returns_min_id() {
    // Spawn high-hp first so "first inserted" != "lowest id" can't hide.
    let vm = run(&format!(
        r#"{DECLS}
        let a = spawn(User {{ name: "dup", level: 1, hp: 1 }})
        let b = spawn(User {{ name: "other", level: 1, hp: 2 }})
        // rename b's key onto "dup" via set: now two entities share the key
        set(b, User {{ name: "dup", level: 1, hp: 2 }})
        let hit = lookup(User, "name", "dup") |> unwrap
        print(hit == a)
        print(len(lookup_all(User, "name", "dup")))
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["true", "2"]);
}

// ---------------------------------------------------------------------------
// maintenance: set / update / remove / despawn / id reuse
// ---------------------------------------------------------------------------

#[test]
fn set_replacement_moves_index_entry() {
    let vm = run(&format!(
        r#"{DECLS}
        let e = spawn(User {{ name: "before", level: 1, hp: 1 }})
        set(e, User {{ name: "after", level: 1, hp: 1 }})
        print(lookup(User, "name", "before") |> is_some)
        print(lookup(User, "name", "after") |> is_some)
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["false", "true"]);
}

#[test]
fn update_block_writeback_moves_index_entry() {
    let vm = run(&format!(
        r#"{DECLS}
        let e = spawn(User {{ name: "u", level: 5, hp: 1 }})
        update(e, User) {{ level = 6 }}
        print(len(lookup_all(User, "level", 5)))
        print(len(lookup_all(User, "level", 6)))
        // non-indexed field update must not disturb the index
        update(e, User) {{ hp = 99 }}
        print(len(lookup_all(User, "level", 6)))
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["0", "1", "1"]);
}

#[test]
fn remove_component_and_despawn_clear_entries() {
    let vm = run(&format!(
        r#"{DECLS}
        let e = spawn(User {{ name: "gone", level: 1, hp: 1 }}, Tag {{ label: "x" }})
        remove(e, Tag)
        print(lookup(Tag, "label", "x") |> is_some)
        print(lookup(User, "name", "gone") |> is_some)
        despawn(e)
        print(lookup(User, "name", "gone") |> is_some)
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["false", "true", "false"]);
}

#[test]
fn id_reuse_after_despawn_does_not_ghost_match() {
    let vm = run(&format!(
        r#"{DECLS}
        let a = spawn(User {{ name: "old", level: 1, hp: 1 }})
        despawn(a)
        // free-list reuse: b takes a's id, with a different key
        let b = spawn(User {{ name: "new", level: 2, hp: 2 }})
        print(a == b)   // same handle (reused id) — the trap
        print(lookup(User, "name", "old") |> is_some)
        let hit = lookup(User, "name", "new") |> unwrap
        print((get(hit, User) |> unwrap).hp)
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["true", "false", "2"]);
}

#[test]
fn handler_writes_maintain_index() {
    let vm = run(&format!(
        r#"{DECLS}
        event Promote {{ who: entity }}
        on Promote(e) {{
            update(e.who, User) {{ level = 10 }}
        }}
        let e = spawn(User {{ name: "h", level: 1, hp: 1 }})
        emit Promote {{ who: e }}
        flush_events()
        print(len(lookup_all(User, "level", 1)))
        print(len(lookup_all(User, "level", 10)))
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["0", "1"]);
}

// ---------------------------------------------------------------------------
// key-type edges
// ---------------------------------------------------------------------------

#[test]
fn string_keys_unicode_and_empty() {
    let vm = run(&format!(
        r#"{DECLS}
        let _a = spawn(User {{ name: "κόσμος🎉", level: 1, hp: 7 }})
        let _b = spawn(User {{ name: "", level: 1, hp: 8 }})
        print((get(lookup(User, "name", "κόσμος🎉") |> unwrap, User) |> unwrap).hp)
        print((get(lookup(User, "name", "") |> unwrap, User) |> unwrap).hp)
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["7", "8"]);
}

#[test]
fn bool_float_and_entity_keys() {
    let vm = run(r#"
        component Flag { indexed enabled: false }
        component Score { indexed v: 0.0 }
        component Owner { indexed boss: entity = 0 }
        let a = spawn("anchor", Flag { enabled: true })
        let _b = spawn(Flag { enabled: false })
        let _s = spawn(Score { v: 1.5 })
        let _o = spawn(Owner { boss: a })
        print((lookup(Flag, "enabled", true) |> unwrap) == a)
        print(lookup(Score, "v", 1.5) |> is_some)
        // entity-valued keys: find who points at `a`
        print(len(lookup_all(Owner, "boss", a)))
    "#);
    assert_eq!(vm.print_buffer, vec!["true", "true", "1"]);
}

/// Pinned semantics, stated honestly: float index keys are BIT-pattern
/// keys. `0.0` and `-0.0` compare equal in rad but are distinct index
/// buckets, and an int probe never matches a float key. This is the
/// documented trade-off for hashability — the test exists so any change
/// to it is a conscious one.
#[test]
fn float_key_bit_pattern_semantics_pinned() {
    let vm = run(r#"
        component Score { indexed v: 0.0 }
        let _z = spawn(Score { v: 0.0 })
        print(-0.0 == 0.0)                          // value equality: true
        print(lookup(Score, "v", 0.0) |> is_some)   // same bits: hit
        print(lookup(Score, "v", -0.0) |> is_some)  // different bits: miss
        print(lookup(Score, "v", 0) |> is_some)     // int probe: miss
    "#);
    assert_eq!(vm.print_buffer, vec!["true", "true", "false", "false"]);
}

// ---------------------------------------------------------------------------
// error edges
// ---------------------------------------------------------------------------

#[test]
fn unindexed_field_is_a_loud_error() {
    let err = run_err(&format!(
        r#"{DECLS}
        let _e = spawn(User {{ name: "x", level: 1, hp: 1 }})
        let _ = lookup(User, "hp", 1)
    "#
    ));
    assert!(err.contains("not indexed"), "got: {err}");

    let err = run_err(&format!(
        r#"{DECLS}
        let _e = spawn(User {{ name: "x", level: 1, hp: 1 }})
        let _ = lookup_all(User, "hp", 1)
    "#
    ));
    assert!(err.contains("not indexed"), "got: {err}");
}

// ---------------------------------------------------------------------------
// survival across world boundaries — the founding bug lives here
// ---------------------------------------------------------------------------

#[test]
fn index_survives_fork_commit_rewind() {
    let vm = run(&format!(
        r#"{DECLS}
        let _a = spawn(User {{ name: "kept", level: 1, hp: 1 }})
        let base = fork()
        let _b = spawn(User {{ name: "speculative", level: 1, hp: 2 }})
        print(lookup(User, "name", "speculative") |> is_some)
        commit(base)    // rewind: speculative spawn never happened
        print(lookup(User, "name", "speculative") |> is_some)
        print(lookup(User, "name", "kept") |> is_some)
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["true", "false", "true"]);
}

/// THE regression: ingest a fork that crossed the wire, commit it, and the
/// index must still answer — both for rows that came over the wire and for
/// edits made after the commit.
#[test]
fn index_survives_wire_roundtrip_commit() {
    let vm = run(&format!(
        r#"{DECLS}
        let _a = spawn(User {{ name: "alice", level: 3, hp: 10 }})
        let _b = spawn(User {{ name: "bob", level: 3, hp: 20 }})
        let bytes = fork_to_bytes(fork())
        let remote = fork_from_bytes(bytes) |> unwrap
        commit(remote)

        // wire-ingested rows are indexed
        print((get(lookup(User, "name", "alice") |> unwrap, User) |> unwrap).hp)
        print(len(lookup_all(User, "level", 3)))

        // and the index is LIVE: post-commit edits maintain it
        let c = spawn(User {{ name: "carol", level: 3, hp: 30 }})
        print(len(lookup_all(User, "level", 3)))
        update(c, User) {{ level = 4 }}
        print(len(lookup_all(User, "level", 3)))
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["10", "2", "3", "2"]);
}

#[test]
fn index_survives_save_load_roundtrip() {
    let vm = run(&format!(
        r#"{DECLS}
        let _a = spawn(User {{ name: "saved", level: 7, hp: 70 }})
        let blob = save_world()
        despawn(lookup(User, "name", "saved") |> unwrap)
        let n = load_world(blob)
        print(n)
        print((get(lookup(User, "name", "saved") |> unwrap, User) |> unwrap).hp)
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["1", "70"]);
}

#[test]
fn index_survives_delta_apply_and_merge() {
    let vm = run(&format!(
        r#"{DECLS}
        let _a = spawn(User {{ name: "base", level: 1, hp: 1 }})
        let base = fork()

        // ours: a local edit
        let b = spawn(User {{ name: "ours", level: 2, hp: 2 }})
        let ours = fork()

        // theirs: reconstructed from base + delta, as a remote peer would
        let delta = fork_delta(base, ours)
        let theirs = fork_apply(base, delta) |> unwrap

        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)

        print(lookup(User, "name", "base") |> is_some)
        print(lookup(User, "name", "ours") |> is_some)
        let c = spawn(User {{ name: "after", level: 3, hp: 3 }})
        print((lookup(User, "name", "after") |> unwrap) == c)
        let _ = b
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["true", "true", "true"]);
}

/// Migration boundary: a v2 program (field renamed, still indexed) loads a
/// v1 save — the index must be built over the MIGRATED rows.
#[test]
fn index_survives_schema_migration_load() {
    let v1 = run(r#"
        component Acct { indexed owner: "", bal: 0 }
        let _a = spawn(Acct { owner: "alice", bal: 10 })
        let _b = spawn(Acct { owner: "bob", bal: 20 })
        print(save_world())
    "#);
    let save = vm_first_print(&v1);

    let v2_src = format!(
        r#"
        component Acct {{ indexed holder: "", bal: 0 }}
        migrate Acct(old) {{
            return Acct {{ holder: old["owner"], bal: old["bal"] }}
        }}
        let n = load_world({save:?})
        print(n)
        print((get(lookup(Acct, "holder", "bob") |> unwrap, Acct) |> unwrap).bal)
    "#
    );
    let vm = run(&v2_src);
    assert_eq!(vm.print_buffer, vec!["2", "20"]);
}

fn vm_first_print(vm: &VM) -> String {
    vm.print_buffer.first().cloned().expect("print output")
}

// ---------------------------------------------------------------------------
// scale + cross-check + pressure
// ---------------------------------------------------------------------------

/// Oracle test: at 5k entities with colliding keys, lookup_all must agree
/// exactly with the naive entities() scan, and lookup with the min of it.
#[test]
fn lookup_all_agrees_with_full_scan_oracle() {
    let vm = run(&format!(
        r#"{DECLS}
        rand_seed(42)
        let mut i = 0
        while i < 5000 {{
            let _ = spawn(User {{ name: f"u{{i % 97}}", level: i % 13, hp: i }})
            i = i + 1
        }}
        // index answer
        let idx = lookup_all(User, "level", 7)
        // oracle: full scan
        let mut scan = []
        for e in entities(User) {{
            if (get(e, User) |> unwrap).level == 7 {{ scan << e }}
        }}
        print(len(idx) == len(scan))
        print(len(idx))
        // same SET (scan order is archetype order; compare as sorted ids)
        let mut same = true
        for e in idx {{
            if !contains(scan, e) {{ same = false }}
        }}
        print(same)
        // lookup = min id of the bucket (lookup_all is already sorted)
        let first = lookup(User, "level", 7) |> unwrap
        print(first == idx[0])
    "#
    ));
    assert_eq!(vm.print_buffer, vec!["true", "385", "true", "true"]);
}

/// The gc_pause class: collector firing at EVERY allocation while the
/// index machinery churns (spawn/set/despawn/lookup in a loop).
#[test]
fn index_machinery_survives_gc_pressure() {
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(7);
    vm.gc.set_collect_threshold_for_test(0);
    vm.load_compile_result(compile(&format!(
        r#"{DECLS}
        let mut i = 0
        while i < 200 {{
            let e = spawn(User {{ name: f"gc{{i}}", level: i % 3, hp: i }})
            update(e, User) {{ level = (i + 1) % 3 }}
            if i % 4 == 0 {{ despawn(e) }}
            i = i + 1
        }}
        print(len(lookup_all(User, "level", 0)) + len(lookup_all(User, "level", 1)) + len(lookup_all(User, "level", 2)))
    "#
    )));
    vm.run(0).expect("gc pressure run");
    assert_eq!(vm.print_buffer, vec!["150"]);
}

/// Replay determinism: a recorded session whose output depends on
/// lookup_all ordering must replay byte-identically.
#[test]
fn lookup_all_order_is_replay_deterministic() {
    let src = format!(
        r#"{DECLS}
        let _c = spawn(User {{ name: "c", level: 1, hp: 3 }})
        let _a = spawn(User {{ name: "a", level: 1, hp: 1 }})
        let _b = spawn(User {{ name: "b", level: 1, hp: 2 }})
        despawn(_a)
        let _d = spawn(User {{ name: "d", level: 1, hp: 4 }})   // reuses a's id
        let hps = map(lookup_all(User, "level", 1), fn(e) {{ return (get(e, User) |> unwrap).hp }})
        print(hps)
    "#
    );
    let mut rec_vm = VM::new();
    rec_vm.suppress_output();
    rec_vm.set_random_seed(7);
    rec_vm.enable_recording(&src);
    rec_vm.load_compile_result(compile(&src));
    rec_vm.run(0).expect("record run");
    let out_a = rec_vm.print_buffer.clone();
    let tape = rec_vm.take_trace().expect("tape");

    let replayer = crate::replay::TraceReplayer::parse(&tape, false).expect("parse");
    let mut rep_vm = VM::new();
    rep_vm.suppress_output();
    rep_vm.enable_replay(replayer);
    rep_vm.load_compile_result(compile(&src));
    rep_vm.run(0).expect("replay run");
    assert_eq!(out_a, rep_vm.print_buffer);
    let report = rep_vm.finish_replay().expect("report");
    assert_eq!(report.end_digest_match, Some(true), "digest must verify");
}

/// The CLI's `rad replay` compiles the tape's embedded source WITHOUT a
/// checker pass ("the program already ran once"). `indexed` declarations
/// must survive that bare compile, or replaying any session that calls
/// `lookup`/`lookup_all` aborts mid-tape with "field is not indexed" —
/// found by recording the tutorial's task board, whose `list` command is
/// one `lookup_all` probe. The fix derives indexes from the AST in the
/// compiler's checker-less fallback; this test replays exactly like main.rs.
#[test]
fn replay_carries_indexes_without_checker_pass() {
    let src = format!(
        r#"{DECLS}
        let _a = spawn(User {{ name: "a", level: 1, hp: 1 }})
        let _b = spawn(User {{ name: "b", level: 1, hp: 2 }})
        let hps = map(lookup_all(User, "level", 1), fn(e) {{ return (get(e, User) |> unwrap).hp }})
        print(hps)
    "#
    );
    let mut rec_vm = VM::new();
    rec_vm.suppress_output();
    rec_vm.set_random_seed(7);
    rec_vm.enable_recording(&src);
    rec_vm.load_compile_result(compile(&src));
    rec_vm.run(0).expect("record run");
    let out_a = rec_vm.print_buffer.clone();
    let tape = rec_vm.take_trace().expect("tape");

    // Bare compile — no with_checker_output — exactly like the replay CLI.
    let mut lexer = Lexer::new(&src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(parser.errors().is_empty(), "parse: {:?}", parser.errors());
    let bare = Compiler::new().compile(&program).expect("bare compile");

    let replayer = crate::replay::TraceReplayer::parse(&tape, false).expect("parse");
    let mut rep_vm = VM::new();
    rep_vm.suppress_output();
    rep_vm.enable_replay(replayer);
    rep_vm.load_compile_result(bare);
    rep_vm
        .run(0)
        .expect("replay must not lose the index declarations");
    assert_eq!(out_a, rep_vm.print_buffer);
    let report = rep_vm.finish_replay().expect("report");
    assert_eq!(report.end_digest_match, Some(true), "digest must verify");
}
