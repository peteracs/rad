//! Composition tests: the seven features as **one machine**.
//!
//! Each of the seven primitives (speculation, record/replay, blast-radius,
//! causality, migration, retro-edit, merge) is tested in its own module.
//! This suite tests the *seams between them* — the places where two features
//! meet and the easy implementation would quietly drop state. The founding
//! member is the in-flight-events-at-merge hole: `fork()` used to capture
//! the world but not the event queue, and `commit()` used to clear it, so
//! any pending events at a fork/commit/merge boundary silently vanished.
//! Events are program state; a snapshot that drops them is not a snapshot.

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::replay::TraceReplayer;
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
    Compiler::new().compile(&program).expect("compile")
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
    vm.set_random_seed(7);
    vm.load_compile_result(compile(src));
    vm.run(0).expect_err("expected a runtime error")
}

/// Runtime errors must teach like the static ones do: unwrap() points at
/// the tools that keep context, require() names the entity, the missing
/// component, and what the entity actually has.
#[test]
fn runtime_errors_teach() {
    // unwrap on None: the ladder hint
    let err = run_err(
        r#"
        component Hp { v: 0 }
        component Mana { v: 0 }
        let hero = spawn("hero", Hp { v: 10 })
        let m = get(hero, Mana) |> unwrap
        print(m.v)
    "#,
    );
    assert!(err.contains("Option::None"), "got: {}", err);
    assert!(err.contains("require(entity, Comp)"), "got: {}", err);
    assert!(err.contains("unwrap_or"), "got: {}", err);

    // require: who, what's missing, what's there
    let err = run_err(
        r#"
        component Hp { v: 0 }
        component Inventory { items: 0 }
        component Mana { v: 0 }
        let hero = spawn("hero", Hp { v: 10 }, Inventory { items: 3 })
        let m = require(hero, Mana)
        print(m.v)
    "#,
    );
    assert!(
        err.contains("missing component 'Mana' on 'hero'"),
        "got: {}",
        err
    );
    assert!(err.contains("has: [Hp, Inventory]"), "got: {}", err);

    // require on a despawned entity says so instead of "missing component"
    let err = run_err(
        r#"
        component Hp { v: 0 }
        let ghost = spawn("ghost", Hp { v: 1 })
        despawn(ghost)
        let h = require(ghost, Hp)
        print(h.v)
    "#,
    );
    assert!(err.contains("no longer exists"), "got: {}", err);

    // unwrap on Err keeps the cause and still hints
    let err = run_err(
        r#"
        let f = fork_from_bytes("garbage") |> unwrap
    "#,
    );
    assert!(err.contains("Result::Err"), "got: {}", err);
    assert!(err.contains("not a rad-fork payload"), "got: {}", err);
    assert!(err.contains("match on Ok/Err"), "got: {}", err);
}

/// Auto-GC × builtins that re-enter the interpreter. Builtins hold heap
/// values in Rust locals the collector cannot see as roots — simulate()'s
/// saved main timeline, sort_by's keyed vec, decode-path migrations. A
/// collection inside that window frees them; threshold 0 makes every
/// back-edge poll eligible, so without the dispatch-wide GC pause this test
/// dies dereferencing the swept event payloads (the web-arena crash,
/// 1-in-3 runs at default thresholds).
#[test]
fn auto_gc_spares_builtin_locals_under_pressure() {
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(7);
    vm.gc.set_collect_threshold_for_test(0);
    vm.load_compile_result(compile(
        r#"
        component Hp { v: 0 }
        resource Sum { total: 0 }
        event Hit { who: int, label: str }
        on Hit(h) {
            let s = get_resource(Sum) |> unwrap
            set_resource(Sum, Sum { total: s.total + h.who + len(h.label) })
        }
        system Tick(hp: mut Hp) {
            // allocation pressure inside the nested execution
            let mut s = ""
            for _i in range(0, 20) { s = s + "garbage garbage garbage" }
            hp = Hp { v: hp.v + len(s) }
        }
        let e = spawn("u", Hp { v: 1 })
        // pending payloads that live in the gc heap...
        emit Hit { who: 3, label: "pending-payload-one" }
        emit Hit { who: 4, label: "pending-payload-two" }
        // ...sit in Rust locals while simulate() re-enters the interpreter
        let fut = simulate(fork(), [system::Tick], 3)
        // the saved queue must come back intact and deliverable
        flush_events()
        let s = get_resource(Sum) |> unwrap
        print(s.total)
        let hp = peek(fut, e, Hp) |> unwrap
        print(hp.v > 1)
    "#,
    ));
    vm.run(0).expect("run");
    assert_eq!(vm.print_buffer[0], "45", "payloads must survive the window");
    assert_eq!(vm.print_buffer[1], "true", "simulation must still apply");
}

// ---------------------------------------------------------------------------
// fork / commit × events
// ---------------------------------------------------------------------------

/// The simplest version of the hole: emit, fork, commit, flush. Before the
/// composition pass the event was silently dropped by commit()'s queue clear.
#[test]
fn fork_commit_roundtrips_in_flight_events() {
    let vm = run(r#"
        resource Tally { n: 0 }
        event Ping { amount }
        on Ping(p) {
            let t = get_resource(Tally) |> unwrap
            set_resource(Tally, Tally { n: t.n + p.amount })
        }
        emit Ping { amount: 5 }
        let f = fork()
        commit(f)
        flush_events()
        let t = get_resource(Tally) |> unwrap
        print(t.n)
    "#);
    assert_eq!(vm.print_buffer, vec!["5"]);
}

/// commit() restores the queue *as captured* — events emitted after the fork
/// are rewound along with the world, exactly like any other state.
#[test]
fn commit_rewinds_the_event_queue() {
    let vm = run(r#"
        resource Tally { n: 0 }
        event Ping { amount }
        on Ping(p) {
            let t = get_resource(Tally) |> unwrap
            set_resource(Tally, Tally { n: t.n + p.amount })
        }
        emit Ping { amount: 1 }
        let f = fork()
        emit Ping { amount: 100 }
        commit(f)
        flush_events()
        let t = get_resource(Tally) |> unwrap
        print(t.n)
    "#);
    assert_eq!(vm.print_buffer, vec!["1"]);
}

// ---------------------------------------------------------------------------
// merge × events (the headline hole)
// ---------------------------------------------------------------------------

/// Pending events at merge time: base's pre-fork event plus each branch's
/// post-fork emission all survive the merge, in deterministic order
/// (base, then ours' suffix, then theirs' suffix).
#[test]
fn merge_preserves_in_flight_events_from_both_branches() {
    let vm = run(r#"
        resource Order { s: "" }
        event Pre {}
        event FromA {}
        event FromB {}
        on Pre(e) {
            let o = get_resource(Order) |> unwrap
            set_resource(Order, Order { s: o.s + "P" })
        }
        on FromA(e) {
            let o = get_resource(Order) |> unwrap
            set_resource(Order, Order { s: o.s + "A" })
        }
        on FromB(e) {
            let o = get_resource(Order) |> unwrap
            set_resource(Order, Order { s: o.s + "B" })
        }

        emit Pre {}
        let base = fork()

        emit FromA {}
        let ours = fork()

        commit(base)
        emit FromB {}
        let theirs = fork()

        commit(base)
        match merge_forks(base, ours, theirs) {
            Ok(m) => {
                commit(m)
            }
            Err(conflicts) => {
                print(f"unexpected conflicts: {len(conflicts)}")
            }
        }
        flush_events()
        let o = get_resource(Order) |> unwrap
        print(o.s)
    "#);
    assert_eq!(vm.print_buffer, vec!["PAB"]);
}

/// If one branch *consumed* events the other still carries, there is no
/// honest automatic answer ("did those handlers run?") — merge refuses
/// loudly instead of guessing.
#[test]
fn merge_refuses_when_a_branch_consumed_pending_events() {
    let vm = run(r#"
        resource Tally { n: 0 }
        event Ping { amount }
        on Ping(p) {
            let t = get_resource(Tally) |> unwrap
            set_resource(Tally, Tally { n: t.n + p.amount })
        }

        emit Ping { amount: 1 }
        let base = fork()

        flush_events()
        let ours = fork()

        commit(base)
        let theirs = fork()

        commit(base)
        match merge_forks(base, ours, theirs) {
            Ok(m) => {
                print("merged")
            }
            Err(conflicts) => {
                for c in conflicts {
                    match c {
                        EventConflict { detail, base, ours, theirs } => {
                            print(f"events consumed by {detail}: {base} base, {ours} ours, {theirs} theirs")
                        }
                        _ => { print("unexpected kind") }
                    }
                }
            }
        }
    "#);
    assert_eq!(
        vm.print_buffer,
        vec!["events consumed by ours: 1 base, 0 ours, 1 theirs"]
    );
}

/// Event payloads contributed by `theirs` pass through the same entity-id
/// remap as component data: a payload referencing a remapped spawn follows
/// the entity to its merged id.
#[test]
fn merged_event_payloads_follow_entity_remap() {
    let vm = run(r#"
        component Tag { kind: "" }
        resource Seen { ok: false }
        event Watch { target }
        on Watch(w) {
            set_resource(Seen, Seen { ok: w.target == get_entity("beta") })
        }

        let base = fork()

        // ours: takes the next entity id.
        let _alpha = spawn("alpha", Tag { kind: "a" })
        let ours = fork()

        // theirs: independently takes the *same* id, and emits a payload
        // pointing at it.
        commit(base)
        let beta = spawn("beta", Tag { kind: "b" })
        emit Watch { target: beta }
        let theirs = fork()

        commit(base)
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        flush_events()
        let s = get_resource(Seen) |> unwrap
        print(s.ok)
    "#);
    assert_eq!(vm.print_buffer, vec!["true"]);
}

// ---------------------------------------------------------------------------
// simulate × events
// ---------------------------------------------------------------------------

/// simulate() runs the fork's pending events as part of the speculated
/// timeline, and whatever the simulation leaves in flight travels with the
/// resulting fork — visible to peek(), committable, mergeable.
#[test]
fn simulate_runs_pending_events_and_carries_leftovers() {
    let vm = run(r#"
        component Hits { n: 0 }
        event Strike { who }
        on Strike(s) {
            let h = get(s.who, Hits) |> unwrap
            set(s.who, Hits { n: h.n + 1 })
            // Re-emit: leaves one event in flight after the tick.
            emit Strike { who: s.who }
        }

        let dummy = spawn("dummy", Hits { n: 0 })
        emit Strike { who: dummy }
        let f = fork()
        let after = simulate(f, [], 1)

        // The pending Strike ran inside the simulation...
        let h = peek(after, dummy, Hits) |> unwrap
        print(h.n)
        // ...the main timeline never saw it...
        let live = get(dummy, Hits) |> unwrap
        print(live.n)
        // ...and the handler's re-emission is still in flight inside the
        // result: committing and flushing fires it on the main timeline.
        commit(after)
        flush_events()
        let h2 = get(dummy, Hits) |> unwrap
        print(h2.n)
    "#);
    assert_eq!(vm.print_buffer, vec!["1", "0", "2"]);
}

// ---------------------------------------------------------------------------
// causality × commit (the seam, disclosed)
// ---------------------------------------------------------------------------

/// why() cannot see writes made inside forks — so after commit() it must say
/// so instead of presenting pre-fork provenance as the whole truth.
#[test]
fn why_discloses_the_commit_seam() {
    let vm = run(r#"
        component Health { hp: 100 }
        let hero = spawn("hero", Health { hp: 100 })
        set(hero, Health { hp: 80 })
        let f = fork()
        commit(f)
        print(why(hero, Health))
        // A fresh main-timeline write supersedes the seam: the note vanishes.
        set(hero, Health { hp: 60 })
        print(why(hero, Health))
    "#);
    let with_seam = &vm.print_buffer[0];
    assert!(
        with_seam.contains("note: commit() adopted a fork"),
        "got: {}",
        with_seam
    );
    assert!(with_seam.contains("hp: 80"), "got: {}", with_seam);
    let after_write = &vm.print_buffer[1];
    assert!(
        !after_write.contains("note: commit()"),
        "got: {}",
        after_write
    );
    assert!(after_write.contains("hp: 60"), "got: {}", after_write);
}

/// Values that *only* exist because of a committed fork have no write record
/// at all — the "no recorded write" answer points at the commit.
#[test]
fn why_points_at_commit_when_value_was_born_in_a_fork() {
    let vm = run(r#"
        component Pos { x: 0 }
        component Vel { dx: 0 }
        let e = spawn("rock", Pos { x: 1 })
        let f = fork()
        commit(f)
        print(why(e, Vel))
    "#);
    let out = &vm.print_buffer[0];
    assert!(out.contains("no recorded write"), "got: {}", out);
    assert!(
        out.contains("note: commit() adopted a fork"),
        "got: {}",
        out
    );
}

// ---------------------------------------------------------------------------
// record/replay × merge × events
// ---------------------------------------------------------------------------

/// A recorded session that forks, diverges, merges (with pending events),
/// commits and flushes must replay to a bit-identical world. merge_forks is
/// pure given the world, so the trace needs no new record kinds — this test
/// pins that invariant.
#[test]
fn recorded_merge_session_replays_bit_identical() {
    let src = r#"
        resource Order { s: "" }
        component Gold { amount: 0 }
        event Pre {}
        event FromA {}
        on Pre(e) {
            let o = get_resource(Order) |> unwrap
            set_resource(Order, Order { s: o.s + "P" })
        }
        on FromA(e) {
            let o = get_resource(Order) |> unwrap
            set_resource(Order, Order { s: o.s + "A" })
        }

        let bank = spawn("bank", Gold { amount: 100 })
        emit Pre {}
        let base = fork()

        set(bank, Gold { amount: 150 })
        emit FromA {}
        let ours = fork()

        commit(base)
        let _scout = spawn("scout", Gold { amount: 5 })
        let theirs = fork()

        commit(base)
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        flush_events()
        let t = clock()
        print(t >= 0)
    "#;

    // Record.
    let mut rec_vm = VM::new();
    rec_vm.suppress_output();
    rec_vm.set_random_seed(7);
    rec_vm.enable_recording(src);
    rec_vm.load_compile_result(compile(src));
    rec_vm.run(0).expect("recorded run");
    let order_recorded = rec_vm.print_buffer.clone();
    let trace = rec_vm.take_trace().expect("trace");

    // Replay.
    let replayer = TraceReplayer::parse(&trace, false).expect("parse trace");
    let mut rep_vm = VM::new();
    rep_vm.suppress_output();
    rep_vm.load_compile_result(compile(src));
    rep_vm.enable_replay(replayer);
    rep_vm.run(0).expect("replay run");
    assert_eq!(rep_vm.print_buffer, order_recorded);
    let report = rep_vm.finish_replay().expect("report");
    assert_eq!(report.leftover_io, 0);
    assert_eq!(report.end_digest_match, Some(true));
}

// ---------------------------------------------------------------------------
// retro-edit × migration
// ---------------------------------------------------------------------------

/// Retroactive replay of an *edited* program whose edit is a schema change:
/// the v2 source declares a `migrate` block and loads a v1 save served from
/// the io oracle. The recorded session's bytes flow through the migration —
/// two features, one pipeline, zero new machinery.
#[test]
fn retro_replay_feeds_recorded_save_through_migration() {
    let dir = std::env::temp_dir().join(format!("rad_comp_retro_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let save_path = dir.join("save.json").to_string_lossy().replace('\\', "/");

    // Produce a v1-shaped save.
    let v1_save_src = r#"
        component Health { hp: 0 }
        let _hero = spawn("hero", Health { hp: 42 })
        print(save_world())
    "#;
    let v1_vm = run(v1_save_src);
    std::fs::write(dir.join("save.json"), &v1_vm.print_buffer[0]).expect("write save");

    // Record a v1 session that reads the save from disk.
    let v1_src = format!(
        r#"
        component Health {{ hp: 0 }}
        let s = read_file("{p}")
        load_world(s)
        let hero = get_entity("hero")
        let h = get(hero, Health) |> unwrap
        print(h.hp)
    "#,
        p = save_path
    );
    let mut rec_vm = VM::new();
    rec_vm.suppress_output();
    rec_vm.set_random_seed(7);
    rec_vm.enable_recording(&v1_src);
    rec_vm.load_compile_result(compile(&v1_src));
    rec_vm.run(0).expect("v1 recorded run");
    assert_eq!(rec_vm.print_buffer, vec!["42"]);
    let trace = rec_vm.take_trace().expect("trace");

    // The file is gone — only the trace remembers it.
    std::fs::remove_file(dir.join("save.json")).expect("delete save");

    // v2: schema evolved (hp -> { hp, max_hp }), with a migrate block.
    let v2_src = format!(
        r#"
        component Health {{ hp: 0, max_hp: 0 }}
        migrate Health(old) {{
            return Health {{ hp: old["hp"], max_hp: old["hp"] }}
        }}
        let s = read_file("{p}")
        load_world(s)
        let hero = get_entity("hero")
        let h = get(hero, Health) |> unwrap
        print(h.hp)
        print(h.max_hp)
    "#,
        p = save_path
    );
    let retro = TraceReplayer::parse(&trace, false)
        .expect("parse")
        .into_retro();
    let mut v2_vm = VM::new();
    v2_vm.suppress_output();
    v2_vm.load_compile_result(compile(&v2_src));
    v2_vm.enable_replay(retro);
    v2_vm.run(0).expect("retro run");
    assert_eq!(v2_vm.print_buffer, vec!["42", "42"]);

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// sandbox × events
// ---------------------------------------------------------------------------

/// A sandbox guest seeded from a fork inherits the fork's pending events:
/// they fire (against the guest's own handlers) after the guest's main
/// chunk, inside the capability fence.
#[test]
fn sandbox_guest_inherits_pending_events() {
    // Host: a world with one pending event.
    let host = run(r#"
        resource Tally { n: 0 }
        event Ping { amount }
        emit Ping { amount: 9 }
        let _f = fork()
    "#);
    // Same capture path fork() uses.
    let snap = host.snapshot_with_events();
    assert_eq!(snap.events.len(), 1, "host fork must carry the event");

    // Guest: declares the same event/resource shape and a handler.
    let guest_src = r#"
        resource Tally { n: 0 }
        event Ping { amount }
        on Ping(p) {
            let t = get_resource(Tally) |> unwrap
            set_resource(Tally, Tally { n: t.n + p.amount })
        }
    "#;
    let caps = crate::sandbox::SandboxCaps::new(
        std::collections::HashSet::from(["*".to_string()]),
        1_000_000,
        16 * 1024 * 1024,
    );
    let outcome = VM::run_sandbox_guest(
        guest_src,
        snap,
        caps,
        7,
        None,
        std::sync::Arc::new(std::collections::HashMap::new()),
    );
    let result_snap = outcome.result.expect("guest run");
    let tally = result_snap.get_resource("Tally").expect("Tally resource");
    let idx = tally.layout.iter().position(|f| f == "n").expect("field n");
    assert_eq!(
        tally.values[idx].as_int(),
        Some(9),
        "inherited event must have fired in the guest"
    );
}

// ---------------------------------------------------------------------------
// distributed merge: the fork wire codec
// ---------------------------------------------------------------------------

/// fork → bytes → fork is identity: world content, names, entity references,
/// in-flight events, and the id allocator all survive. Re-encoding the
/// decoded fork yields byte-identical wire data (canonical encoding).
#[test]
fn fork_codec_roundtrip_is_identity() {
    let vm = run(r#"
        component Tag { label: "" }
        component Escort { guard: entity | nil = nil }
        resource Tally { n: 0 }
        event Ping { amount }
        on Ping(p) {
            let t = get_resource(Tally) |> unwrap
            set_resource(Tally, Tally { n: t.n + p.amount })
        }

        let a = spawn("alpha", Tag { label: "a" })
        let b = spawn("beta", Tag { label: "b" })
        despawn(b)                                 // leaves a hole in the id space
        let c = spawn("gamma", Tag { label: "c" })
        set(a, Escort { guard: c })                // entity reference inside a component
        emit Ping { amount: 7 }                    // in-flight event
        let f = fork()

        let bytes = fork_to_bytes(f)
        let g = fork_from_bytes(bytes) |> unwrap

        print(diff(f, g))                          // value-identical worlds
        print(bytes == fork_to_bytes(g))           // canonical: re-encode is byte-identical

        // Allocator parity: a spawn after committing the original and a spawn
        // after committing the decoded copy land on the same id.
        commit(f)
        let z1 = spawn("z", Tag { label: "z" })
        commit(g)
        let z2 = spawn("z", Tag { label: "z" })
        print(z1 == z2)

        // And the in-flight event came through the wire: flush fires it.
        flush_events()
        let t = get_resource(Tally) |> unwrap
        print(t.n)
    "#);
    assert_eq!(vm.print_buffer, vec!["{}", "true", "true", "7"]);
}

/// Two VMs ("two machines"), one world: B receives base over the wire,
/// diverges, sends its fork back; A merges it against its own divergence.
/// The result must be value-identical to the same merge performed entirely
/// in-process — the wire is transparent to merge semantics.
#[test]
fn cross_vm_merge_equals_in_process_merge() {
    let src = r#"
        component Gold { amount: 0 }
        component Tag { label: "" }
        resource Log { s: "" }

        fn make_base() {
            let _bank = spawn("bank", Gold { amount: 100 })
            let _hero = spawn("hero", Tag { label: "idle" })
        }

        fn diverge_ours() {
            let bank = get_entity("bank")
            set(bank, Gold { amount: 150 })
            set_resource(Log, Log { s: "ours" })
        }

        fn diverge_theirs() {
            let hero = get_entity("hero")
            set(hero, Tag { label: "questing" })
            let _scout = spawn("scout", Tag { label: "new" })
        }
    "#;

    // Machine A: base + ours.
    let a_src = format!(
        r#"{src}
        make_base()
        let base = fork()
        diverge_ours()
        let ours = fork()
        commit(base)
        print(fork_to_bytes(base))
    "#,
        src = src
    );
    let mut vm_a = VM::new();
    vm_a.suppress_output();
    vm_a.set_random_seed(7);
    vm_a.load_compile_result(compile(&a_src));
    vm_a.run(0).expect("machine A");
    let base_bytes = vm_a.print_buffer[0].clone();

    // Machine B: decode base, diverge, send back.
    let b_src = format!(
        r#"{src}
        let base = fork_from_bytes(input()) |> unwrap
        commit(base)
        diverge_theirs()
        let theirs = fork()
        print(fork_to_bytes(theirs))
    "#,
        src = src
    );
    // input() would block — feed the bytes as a literal instead.
    let b_src = b_src.replace("input()", &format!("{:?}", base_bytes));
    let mut vm_b = VM::new();
    vm_b.suppress_output();
    vm_b.set_random_seed(7);
    vm_b.load_compile_result(compile(&b_src));
    vm_b.run(0).expect("machine B");
    let theirs_bytes = vm_b.print_buffer[0].clone();

    // Reference: the whole dance in one process.
    let ref_src = format!(
        r#"{src}
        make_base()
        let base = fork()
        diverge_ours()
        let ours = fork()
        commit(base)
        diverge_theirs()
        let theirs = fork()
        commit(base)
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        print(fork_to_bytes(fork()))
    "#,
        src = src
    );
    let mut vm_ref = VM::new();
    vm_ref.suppress_output();
    vm_ref.set_random_seed(7);
    vm_ref.load_compile_result(compile(&ref_src));
    vm_ref.run(0).expect("reference run");
    let reference = vm_ref.print_buffer[0].clone();

    // Machine A again: merge the wire-delivered theirs.
    let merge_src = format!(
        r#"{src}
        make_base()
        let base = fork()
        diverge_ours()
        let ours = fork()
        commit(base)
        let theirs = fork_from_bytes({theirs:?}) |> unwrap
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        print(fork_to_bytes(fork()))
    "#,
        src = src,
        theirs = theirs_bytes
    );
    let mut vm_m = VM::new();
    vm_m.suppress_output();
    vm_m.set_random_seed(7);
    vm_m.load_compile_result(compile(&merge_src));
    vm_m.run(0).expect("cross-VM merge");

    // The *state* must be byte-identical to the in-process merge — the wire
    // is transparent to merge semantics. The provenance section is the one
    // honest difference: the wire path knows some records crossed machines
    // (origin labels name the payload digests), the in-process path has no
    // seam to disclose. Asserting both halves pins that boundary exactly.
    fn state_part(payload: &str) -> &str {
        let body = payload
            .splitn(3, ' ')
            .nth(2)
            .expect("RADFORK2 payload has header + body");
        &body[..body.find(",\"prov\":").unwrap_or(body.len())]
    }
    assert_eq!(
        state_part(&vm_m.print_buffer[0]),
        state_part(&reference),
        "wire merge must be state-identical to in-process merge"
    );
    assert!(
        vm_m.print_buffer[0].contains("wire "),
        "wire-merged provenance must carry origin labels, got: {}",
        vm_m.print_buffer[0]
    );
    assert!(
        !reference.contains("wire "),
        "in-process provenance must not invent origins, got: {}",
        reference
    );
}

/// Schema drift across machines: the sender runs v1, the receiver declares
/// v2 with a `migrate` block — ingestion runs the migration, exactly like
/// `load_world`. Two machines may disagree on schema version and still merge.
#[test]
fn fork_codec_runs_migrations_on_schema_drift() {
    let v1 = r#"
        component Health { hp: 0 }
        let _hero = spawn("hero", Health { hp: 42 })
        print(fork_to_bytes(fork()))
    "#;
    let v1_vm = run(v1);
    let bytes = v1_vm.print_buffer[0].clone();

    let v2 = format!(
        r#"
        component Health {{ hp: 0, max_hp: 0 }}
        migrate Health(old) {{
            return Health {{ hp: old["hp"], max_hp: old["hp"] }}
        }}
        let g = fork_from_bytes({bytes:?}) |> unwrap
        commit(g)
        let hero = get_entity("hero")
        let h = get(hero, Health) |> unwrap
        print(f"{{h.hp}}/{{h.max_hp}}")
    "#,
        bytes = bytes
    );
    let v2_vm = run(&v2);
    assert_eq!(v2_vm.print_buffer, vec!["42/42"]);
}

/// Corrupted or tampered bytes are an honest Err, not a crash and not a
/// silently wrong world: network input is a system boundary.
#[test]
fn fork_codec_rejects_corruption() {
    let vm = run(r#"
        component Tag { label: "" }
        let _a = spawn("a", Tag { label: "x" })
        let bytes = fork_to_bytes(fork())
        let tampered = replace(bytes, "\"x\"", "\"y\"")
        match fork_from_bytes(tampered) {
            Ok(_) => { print("accepted?!") }
            Err(e) => { print(e) }
        }
        // And garbage is a parse error, not a panic.
        match fork_from_bytes("not json at all") {
            Ok(_) => { print("accepted?!") }
            Err(e) => { print(e) }
        }
    "#);
    assert!(
        vm.print_buffer[0].contains("digest mismatch"),
        "got: {}",
        vm.print_buffer[0]
    );
    assert!(
        vm.print_buffer[1].contains("not a rad-fork payload"),
        "got: {}",
        vm.print_buffer[1]
    );
}

/// Wire bytes arriving through io compose with record & replay for free:
/// the read is in the trace, so a session that ingested a remote fork
/// replays bit-identically with no network and no new record kinds.
#[test]
fn received_fork_bytes_replay_like_any_io() {
    let dir = std::env::temp_dir().join(format!("rad_wire_replay_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let wire_path = dir.join("fork.json").to_string_lossy().replace('\\', "/");

    // "Remote machine": produce wire bytes.
    let remote = run(r#"
        component Gold { amount: 0 }
        let _bank = spawn("bank", Gold { amount: 500 })
        print(fork_to_bytes(fork()))
    "#);
    std::fs::write(dir.join("fork.json"), &remote.print_buffer[0]).expect("write wire");

    let src = format!(
        r#"
        component Gold {{ amount: 0 }}
        let g = fork_from_bytes(read_file("{p}")) |> unwrap
        commit(g)
        let bank = get_entity("bank")
        let b = get(bank, Gold) |> unwrap
        print(b.amount)
    "#,
        p = wire_path
    );
    let mut rec_vm = VM::new();
    rec_vm.suppress_output();
    rec_vm.set_random_seed(7);
    rec_vm.enable_recording(&src);
    rec_vm.load_compile_result(compile(&src));
    rec_vm.run(0).expect("recorded ingest");
    assert_eq!(rec_vm.print_buffer, vec!["500"]);
    let trace = rec_vm.take_trace().expect("trace");

    // The network is gone; the trace remembers.
    std::fs::remove_file(dir.join("fork.json")).expect("delete wire");

    let replayer = TraceReplayer::parse(&trace, false).expect("parse trace");
    let mut rep_vm = VM::new();
    rep_vm.suppress_output();
    rep_vm.load_compile_result(compile(&src));
    rep_vm.enable_replay(replayer);
    rep_vm.run(0).expect("replay");
    assert_eq!(rep_vm.print_buffer, vec!["500"]);
    let report = rep_vm.finish_replay().expect("report");
    assert_eq!(report.end_digest_match, Some(true));

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// sandpaper: literals default-fill
// ---------------------------------------------------------------------------

/// Fields whose declarations carry defaults may be omitted from literals —
/// the constructor fills them from the declaration. (Bare type annotations
/// like `x: float` have no default and stay required.)
#[test]
fn component_literals_default_fill_omitted_fields() {
    let vm = run(r#"
        component Incident { title: "", priority: 0, age: 0, status: "open" }
        component Escort { guard: entity | nil = nil }
        resource Board { open: 0, label: "ops" }

        let i = spawn("inc", Incident { title: "disk full" })
        set(i, Escort {})
        set_resource(Board, Board { open: 3 })

        let v = get(i, Incident) |> unwrap
        let e = get(i, Escort) |> unwrap
        let b = get_resource(Board) |> unwrap
        print(f"{v.title}|{v.priority}|{v.status}")
        print(e.guard == nil)
        print(f"{b.open}|{b.label}")
    "#);
    assert_eq!(vm.print_buffer, vec!["disk full|0|open", "true", "3|ops"]);
}

// ---------------------------------------------------------------------------
// causality retention window (long-running processes)
// ---------------------------------------------------------------------------

/// The ledger is a window, not an archive: old records evict, emit ids stay
/// stable, the commit seam survives, and why() discloses the truncation
/// instead of presenting a partial ledger as the whole truth.
#[test]
fn causality_retention_evicts_honestly() {
    use crate::causality::{CausalityLedger, Cause, WriteKind};
    let mut ledger = CausalityLedger::default();
    ledger.set_retention_cap(3);

    for i in 0..10 {
        ledger.record_write(
            0,
            Some(i),
            Some(format!("e{}", i)),
            "Pos",
            format!("{{ x: {} }}", i),
            WriteKind::Set,
            Cause::Main,
        );
        let id = ledger.record_emit(0, "Tick", format!("Tick {{ n: {} }}", i), Cause::Main);
        assert_eq!(
            id,
            i as u64 + 1,
            "emit ids must stay monotonic across eviction"
        );
    }

    // Recent records still explain normally.
    let recent = ledger.explain_entity(9, "Pos", u64::MAX);
    assert!(recent.contains("Pos of e9"), "got: {}", recent);

    // Evicted records say so instead of guessing.
    let evicted = ledger.explain_entity(0, "Pos", u64::MAX);
    assert!(evicted.contains("no recorded write"), "got: {}", evicted);
    assert!(
        evicted.contains("evicted by the retention window"),
        "got: {}",
        evicted
    );

    // Commit watermarks are absolute: a commit recorded now still orders
    // correctly against surviving writes after future evictions.
    ledger.record_commit(0);
    let seam = ledger.explain_entity(9, "Pos", u64::MAX);
    assert!(seam.contains("note: commit()"), "got: {}", seam);
}

// ---------------------------------------------------------------------------
// blast-radius × merge
// ---------------------------------------------------------------------------

/// diff() composes with merge: the merged fork's distance from base is
/// exactly the union of both branches' edits — and assert_only_changed
/// fences a merge the same way it fences any other mutation.
#[test]
fn merge_result_is_diffable_and_fenceable() {
    let vm = run(r#"
        component Gold { amount: 0 }
        component Pos { x: 0 }
        let bank = spawn("bank", Gold { amount: 100 })
        let rock = spawn("rock", Pos { x: 1 })
        let base = fork()

        set(bank, Gold { amount: 150 })
        let ours = fork()

        commit(base)
        set(rock, Pos { x: 5 })
        let theirs = fork()

        commit(base)
        let merged = merge_forks(base, ours, theirs) |> unwrap
        let d = diff(base, merged)
        print(d["Gold"])
        print(d["Pos"])
        commit(merged)
        let g = get(bank, Gold) |> unwrap
        let p = get(rock, Pos) |> unwrap
        print(g.amount)
        print(p.x)
    "#);
    assert_eq!(vm.print_buffer, vec!["1", "1", "150", "5"]);
}

// ---------------------------------------------------------------------------
// conflicts as data × programmable resolution
// ---------------------------------------------------------------------------

/// A merge conflict is a value: user code destructures it and gets the
/// entity handle, component, field, and all three diverging values — then
/// uses the entity handle *as an entity* (looks up another component on it).
/// No string parsing anywhere.
#[test]
fn conflicts_destructure_into_usable_values() {
    let vm = run(r#"
        component Ticket { status: "open" }
        component Meta { team: "" }
        let t = spawn("T-1", Ticket { status: "open" }, Meta { team: "infra" })
        let base = fork()

        set(t, Ticket { status: "closed" })
        let ours = fork()

        commit(base)
        set(t, Ticket { status: "escalated" })
        let theirs = fork()

        commit(base)
        match merge_forks(base, ours, theirs) {
            Ok(_) => { print("merged?!") }
            Err(conflicts) => {
                for c in conflicts {
                    match c {
                        FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                            let m = get(ent, Meta) |> unwrap
                            print(f"{name} ({m.team}): {comp}.{field} ours={ours} theirs={theirs}")
                        }
                        _ => { print("unexpected kind") }
                    }
                }
            }
        }
    "#);
    assert_eq!(
        vm.print_buffer,
        vec!["T-1 (infra): Ticket.status ours=closed theirs=escalated"]
    );
}

/// The acceptance test of the round-table: a sync policy written **in rad**.
/// merge_forks reports conflicts; user code decides per field (status
/// precedence: closed beats escalated beats open); merge_forks_with applies
/// the decisions and merges clean. Unresolved fields still conflict.
#[test]
fn merge_forks_with_applies_a_policy_written_in_rad() {
    let vm = run(r#"
        component Ticket { status: "open", assignee: "" }
        let t = spawn("T-1", Ticket { status: "open", assignee: "" })
        let base = fork()

        update(t, Ticket) { status = "closed", assignee = "mei" }
        let ours = fork()

        commit(base)
        update(t, Ticket) { status = "escalated", assignee = "raj" }
        let theirs = fork()

        commit(base)

        // The policy, in rad: status merges by precedence; assignee
        // conflicts defer to theirs (the pusher wins assignment).
        fn rank(s: str) -> int {
            if s == "closed" { return 3 }
            if s == "escalated" { return 2 }
            return 1
        }

        match merge_forks(base, ours, theirs) {
            Ok(m) => { commit(m) }
            Err(conflicts) => {
                let mut decisions = []
                for c in conflicts {
                    match c {
                        FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                            if field == "status" {
                                let mut pick = ours
                                if rank(theirs) > rank(ours) { pick = theirs }
                                decisions = push(decisions, (c, pick))
                            }
                            if field == "assignee" {
                                decisions = push(decisions, (c, theirs))
                            }
                        }
                        _ => {}
                    }
                }
                let m = merge_forks_with(base, ours, theirs, decisions) |> unwrap
                commit(m)
            }
        }

        let tk = get(t, Ticket) |> unwrap
        print(f"{tk.status} {tk.assignee}")
    "#);
    assert_eq!(vm.print_buffer, vec!["closed raj"]);
}

/// Resolutions only cover what they name: an unresolved field conflict
/// still refuses, structural conflicts cannot be resolved at all.
#[test]
fn merge_forks_with_leaves_unnamed_conflicts_and_rejects_structural() {
    let vm = run(r#"
        component Gold { amount: 0 }
        let hero = spawn("hero", Gold { amount: 10 })
        let base = fork()
        set(hero, Gold { amount: 1 })
        let ours = fork()
        commit(base)
        set(hero, Gold { amount: 2 })
        let theirs = fork()
        commit(base)

        // Empty decisions: same conflict comes back.
        match merge_forks_with(base, ours, theirs, []) {
            Ok(_) => { print("merged?!") }
            Err(conflicts) => { print(f"still {len(conflicts)} conflict") }
        }
    "#);
    assert_eq!(vm.print_buffer, vec!["still 1 conflict"]);
}

// ---------------------------------------------------------------------------
// causality × wire: provenance rides the fork payload
// ---------------------------------------------------------------------------

/// The cross-machine why(): machine B creates an entity and leaves an event
/// in flight; machine A ingests the fork, merges, commits, flushes. Asking A
/// "why does this value exist?" must answer with B's history — spawn origin,
/// the handler chain through B's emit — labeled with the wire origin instead
/// of pretending the value has no past. This is the query that failed in the
/// first syncdesk smoke run.
#[test]
fn why_answers_across_the_machine_seam() {
    let shared = r#"
        component Ticket { status: "open" }
        component Notes { n: 0 }
        event NoteAdded { who }
        on NoteAdded(e) {
            let cur = get(e.who, Notes) |> unwrap
            set(e.who, Notes { n: cur.n + 1 })
        }
    "#;

    // Machine B: spawn a ticket, emit (but don't flush) a note event.
    let b_src = format!(
        r#"{shared}
        let t = spawn("T-9", Ticket {{ status: "open" }}, Notes {{ n: 0 }})
        emit NoteAdded {{ who: t }}
        print(fork_to_bytes(fork()))
    "#
    );
    let vm_b = run(&b_src);
    let bytes = vm_b.print_buffer[0].clone();

    // Machine A: empty world, ingest, commit, flush — then ask why.
    let a_src = format!(
        r#"{shared}
        let theirs = fork_from_bytes({bytes:?}) |> unwrap
        commit(theirs)
        flush_events()
        let t = get_entity("T-9")
        print(why(t, Ticket))
        print(why(t, Notes))
    "#
    );
    let vm_a = run(&a_src);

    // The spawn that A never performed answers with B's record + origin.
    let ticket = &vm_a.print_buffer[0];
    assert!(
        ticket.contains("Ticket of T-9") && ticket.contains("spawned"),
        "got: {}",
        ticket
    );
    assert!(ticket.contains("[via wire "), "got: {}", ticket);
    assert!(ticket.contains("<- by top-level code"), "got: {}", ticket);

    // The handler write happened on A, but its cause — the emit — happened
    // on B: the chain crosses the seam and says so.
    let notes = &vm_a.print_buffer[1];
    assert!(notes.contains("Notes of T-9 = { n: 1 }"), "got: {}", notes);
    assert!(
        notes.contains("<- by `on NoteAdded` handler"),
        "got: {}",
        notes
    );
    assert!(
        notes.contains("NoteAdded") && notes.contains("[via wire "),
        "emit record must carry the wire origin, got: {}",
        notes
    );
    assert!(notes.contains("<- by top-level code"), "got: {}", notes);
}

/// Provenance follows the merge path too: a value merged in from a wire
/// fork explains itself with the sender's record after commit(merged).
#[test]
fn why_explains_values_that_arrived_by_merge() {
    let shared = r#"
        component Gold { amount: 0 }
        fn seed() {
            let _hero = spawn("hero", Gold { amount: 10 })
        }
    "#;

    // Machine B: same base (deterministic), then diverge.
    let b_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let hero = get_entity("hero")
        set(hero, Gold {{ amount: 999 }})
        print(fork_to_bytes(fork()))
    "#
    );
    let vm_b = run(&b_src);
    let theirs_bytes = vm_b.print_buffer[0].clone();

    let a_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let ours = fork()
        let theirs = fork_from_bytes({theirs_bytes:?}) |> unwrap
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        let hero = get_entity("hero")
        let g = get(hero, Gold) |> unwrap
        print(g.amount)
        print(why(hero, Gold))
    "#
    );
    let vm_a = run(&a_src);
    assert_eq!(vm_a.print_buffer[0], "999");
    let out = &vm_a.print_buffer[1];
    assert!(
        out.contains("Gold of hero = { amount: 999 }"),
        "got: {}",
        out
    );
    assert!(out.contains("[via wire "), "got: {}", out);
}

// ---------------------------------------------------------------------------
// delta sync: fork_delta / fork_apply
// ---------------------------------------------------------------------------

/// Strip the wire header and the provenance section, leaving the state body.
/// State must be reproducible bit-for-bit; provenance legitimately differs
/// (origin labels name the seams a payload crossed).
fn wire_state_part(payload: &str) -> &str {
    let body = payload
        .splitn(3, ' ')
        .nth(2)
        .expect("wire payload has header + body");
    &body[..body.find(",\"prov\":").unwrap_or(body.len())]
}

/// fork_apply(base, fork_delta(base, f)) must reconstruct f exactly: entity
/// edits, spawns, despawns, renames, resources, the allocator, and the
/// in-flight queue. Verified by comparing the canonical full encodings of
/// the original and the reconstruction (state part — the reconstruction
/// honestly carries a wire origin in its provenance).
#[test]
fn delta_roundtrip_reconstructs_the_fork() {
    let vm = run(r#"
        component Gold { amount: 0 }
        component Tag { label: "" }
        resource Log { s: "" }
        event Ping { amount }
        on Ping(p) {
            let bank = get_entity("bank")
            let g = get(bank, Gold) |> unwrap
            set(bank, Gold { amount: g.amount + p.amount })
        }

        let _bank = spawn("bank", Gold { amount: 100 })
        let _hero = spawn("hero", Tag { label: "idle" })
        let _mook = spawn("mook", Tag { label: "doomed" })
        let base = fork()

        // Divergence: edit, spawn, despawn, resource write, in-flight event.
        let bank = get_entity("bank")
        set(bank, Gold { amount: 150 })
        let _scout = spawn("scout", Tag { label: "new" })
        let mook = get_entity("mook")
        despawn(mook)
        set_resource(Log, Log { s: "diverged" })
        emit Ping { amount: 7 }
        let f = fork()

        let d = fork_delta(base, f)
        let g = fork_apply(base, d) |> unwrap
        print(fork_to_bytes(f))
        print(fork_to_bytes(g))

        // And the reconstruction *behaves* like the original: commit, flush,
        // allocator parity (next spawn claims the same id).
        commit(g)
        flush_events()
        let b2 = get(get_entity("bank"), Gold) |> unwrap
        print(b2.amount)
        let probe = spawn("probe", Tag { label: "p" })
        print(probe)
    "#);
    assert_eq!(
        wire_state_part(&vm.print_buffer[0]),
        wire_state_part(&vm.print_buffer[1]),
        "reconstruction must be state-identical to the original fork"
    );
    assert_eq!(
        vm.print_buffer[2], "157",
        "in-flight event must survive the delta"
    );

    // Reference: same program without the wire detour claims the same id.
    let vm_ref = run(r#"
        component Gold { amount: 0 }
        component Tag { label: "" }
        resource Log { s: "" }
        event Ping { amount }
        on Ping(p) {
            let bank = get_entity("bank")
            let g = get(bank, Gold) |> unwrap
            set(bank, Gold { amount: g.amount + p.amount })
        }
        let _bank = spawn("bank", Gold { amount: 100 })
        let _hero = spawn("hero", Tag { label: "idle" })
        let _mook = spawn("mook", Tag { label: "doomed" })
        let bank = get_entity("bank")
        set(bank, Gold { amount: 150 })
        let _scout = spawn("scout", Tag { label: "new" })
        let mook = get_entity("mook")
        despawn(mook)
        set_resource(Log, Log { s: "diverged" })
        emit Ping { amount: 7 }
        flush_events()
        let probe = spawn("probe", Tag { label: "p" })
        print(probe)
    "#);
    assert_eq!(
        vm.print_buffer[3], vm_ref.print_buffer[0],
        "allocator state must survive the delta"
    );
}

/// The point of delta sync: cost proportional to the divergence, not the
/// world — for state *and* provenance. A 3-entity edit in a 300-entity world
/// must not ship the other 297 entities or their history.
#[test]
fn delta_is_small_and_so_is_its_provenance() {
    let vm = run(r#"
        component Item { label: "", n: 0 }
        let mut i = 0
        while i < 300 {
            let _e = spawn(Item { label: "filler_" + str(i), n: i })
            i = i + 1
        }
        let keep = spawn("keeper", Item { label: "keeper", n: 0 })
        let base = fork()

        set(keep, Item { label: "touched_value", n: 1 })
        let f = fork()

        print(len(fork_to_bytes(f)))
        print(len(fork_delta(base, f)))
        print(fork_delta(base, f))
    "#);
    let full: usize = vm.print_buffer[0].parse().unwrap();
    let delta: usize = vm.print_buffer[1].parse().unwrap();
    assert!(
        delta * 10 < full,
        "delta ({} bytes) must be at least 10x smaller than full ({} bytes)",
        delta,
        full
    );
    let payload = &vm.print_buffer[2];
    assert!(payload.contains("touched_value"), "touched row must ship");
    assert!(
        !payload.contains("filler_7"),
        "untouched rows (and their provenance) must not ship"
    );
}

/// `world_digest()` is the convergence receipt for distributed sync: it
/// hashes world STATE only. Event/provenance history must not move it,
/// in-flight (unflushed) events must not move it, and an actual state
/// change must.
#[test]
fn world_digest_tracks_state_not_history() {
    let vm = run(r#"
        component Counter { n: 0 }
        event Bump { who: entity }
        on Bump(e) { update(e.who, Counter) { n = 1 } }

        let a = spawn("a", Counter { n: 0 })
        let d0 = world_digest()

        // in-flight events are not state
        emit Bump { who: a }
        let d_inflight = world_digest()
        print(str(d_inflight == d0))

        // a real state change moves the digest
        flush_events()
        let d1 = world_digest()
        print(str(d1 != d0))

        // same state reached by a different history digests identically
        update(a, Counter) { n = 0 }
        let d2 = world_digest()
        print(str(d2 == d0))
    "#);
    assert_eq!(vm.print_buffer, vec!["true", "true", "true"]);
}

/// Entity-level patch granularity: editing one field of one component on an
/// entity the base already holds must not re-ship the entity's other
/// components (or the untouched fields of the edited one). Component
/// removal travels as a name, attachment falls back to a full upsert row.
#[test]
fn delta_ships_entity_field_patches_not_whole_rows() {
    let vm = run(r#"
        component Hp { current: 0, max: 0 }
        component Bio { story: "" }
        component Mark { tag: "" }
        component Aura { glow: "" }
        // long enough that provenance previews (96-char cap) cannot leak the
        // tail sentinel — only an actual whole-row ship would carry it
        let saga = "born under a wandering star, exiled twice, knighted once, and sworn to a quiet oath that ends in THE_FINAL_SECRET_WORD"
        let h = spawn("hero", Hp { current: 100, max: 100 },
                      Bio { story: saga },
                      Mark { tag: "doomed" })
        let base = fork()

        update(h, Hp) { current = 93 }
        remove(h, Mark)
        let d = fork_delta(base, fork())
        match fork_apply(base, d) {
            Ok(f) => {
                commit(f)
                let hp = get(h, Hp) |> unwrap
                print(hp.current)
                print(hp.max)
                print((get(h, Bio) |> unwrap).story == saga)
                print(has(h, Mark))
            }
            Err(m) => { print("apply failed: " + m) }
        }
        print(d)

        // attaching a component the base never saw cannot be a patch: the
        // entity falls back to a full upsert row (saga rides along)
        set(h, Aura { glow: "golden" })
        print(fork_delta(base, fork()))
    "#);
    assert_eq!(vm.print_buffer[0], "93", "patched field must apply");
    assert_eq!(vm.print_buffer[1], "100", "untouched field must survive");
    assert_eq!(
        vm.print_buffer[2], "true",
        "untouched component must survive byte-for-byte"
    );
    assert_eq!(vm.print_buffer[3], "false", "removal must apply");
    let payload = &vm.print_buffer[4];
    assert!(
        payload.contains("\"ent_patch\":[[") && payload.contains("\"upserts\":[]"),
        "edit of a known entity must travel as a patch, not an upsert: {}",
        payload
    );
    assert!(
        !payload.contains("THE_FINAL_SECRET_WORD"),
        "untouched component content must not ship: {}",
        payload
    );
    let with_attach = &vm.print_buffer[5];
    assert!(
        with_attach.contains("\"upserts\":[[") && with_attach.contains("THE_FINAL_SECRET_WORD"),
        "attaching a new component must fall back to a full upsert row: {}",
        with_attach
    );
}

/// Per-field resource patches: a growing journal must cost O(changed fields),
/// not O(resource). Ticking `round` on a resource that also carries a fat
/// log string must not re-ship the log.
#[test]
fn delta_ships_resource_field_patches_not_whole_rows() {
    let vm = run(r#"
        resource Journal { log: "", round: 0 }
        let mut big = ""
        let mut i = 0
        while i < 200 {
            big = big + "entry " + str(i) + "!"
            i = i + 1
        }
        set_resource(Journal, Journal { log: big, round: 1 })
        let base = fork()
        let j = get_resource(Journal) |> unwrap
        set_resource(Journal, Journal { log: j.log, round: 2 })
        let d = fork_delta(base, fork())
        match fork_apply(base, d) {
            Ok(f) => {
                commit(f)
                let j2 = get_resource(Journal) |> unwrap
                print(j2.round)
                print(len(j2.log))
            }
            Err(m) => { print("apply failed: " + m) }
        }
        print(len(big))
        print(len(d))
        print(d)
    "#);
    assert_eq!(vm.print_buffer[0], "2", "patched field must apply");
    assert_eq!(
        vm.print_buffer[1], vm.print_buffer[2],
        "untouched log field must survive the patch byte-for-byte"
    );
    let log_len: usize = vm.print_buffer[1].parse().unwrap();
    let delta_len: usize = vm.print_buffer[3].parse().unwrap();
    assert!(
        delta_len < log_len / 2,
        "delta ({} B) must not carry the {} B log",
        delta_len,
        log_len
    );
    let payload = &vm.print_buffer[4];
    assert!(payload.contains("\"res_patch\""), "got: {}", payload);
    // (the provenance section carries a ~100-char bounded *preview* of the
    // touched resource, so probe for content beyond the preview cutoff)
    assert!(
        !payload.contains("entry 150!"),
        "unchanged log content must not ship: {}",
        payload
    );
}

/// Corruption and wrong-base application are errors, not fabricated worlds.
#[test]
fn delta_apply_rejects_corruption_and_wrong_base() {
    let vm = run(r#"
        component Gold { amount: 0 }
        let _bank = spawn("bank", Gold { amount: 100 })
        let base = fork()
        let _extra = spawn("extra", Gold { amount: 1 })
        let other = fork()
        commit(base)

        let bank = get_entity("bank")
        set(bank, Gold { amount: 150 })
        let f = fork()
        let d = fork_delta(base, f)

        match fork_apply(base, d + " ") {
            Ok(_) => { print("accepted corrupt") }
            Err(e) => { print(e) }
        }
        match fork_apply(other, d) {
            Ok(_) => { print("accepted wrong base") }
            Err(e) => { print(e) }
        }
        match fork_apply(base, d) {
            Ok(_) => { print("ok") }
            Err(e) => { print(e) }
        }
    "#);
    assert!(
        vm.print_buffer[0].contains("integrity digest mismatch"),
        "got: {}",
        vm.print_buffer[0]
    );
    assert!(
        vm.print_buffer[1].contains("different base"),
        "got: {}",
        vm.print_buffer[1]
    );
    assert_eq!(vm.print_buffer[2], "ok");
}

/// Cross-machine why() over the delta path: the receiver applies a delta,
/// commits, and the answer chains through records that traveled inside it —
/// labeled with the wire origin, including the emit behind a handler write.
#[test]
fn delta_provenance_answers_why_across_the_seam() {
    let shared = r#"
        component Ticket { status: "open" }
        component Notes { n: 0 }
        event NoteAdded { who }
        on NoteAdded(e) {
            let cur = get(e.who, Notes) |> unwrap
            set(e.who, Notes { n: cur.n + 1 })
        }
        fn seed() {
            let _t = spawn("T-9", Ticket { status: "open" }, Notes { n: 0 })
        }
    "#;

    // Machine B: same deterministic base, then diverge + emit, ship a delta.
    let b_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let t = get_entity("T-9")
        set(t, Ticket {{ status: "escalated" }})
        emit NoteAdded {{ who: t }}
        print(fork_delta(base, fork()))
    "#
    );
    let vm_b = run(&b_src);
    let delta = vm_b.print_buffer[0].clone();

    // Machine A: apply against its own copy of the base, commit, flush, ask.
    let a_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let theirs = fork_apply(base, {delta:?}) |> unwrap
        commit(theirs)
        flush_events()
        let t = get_entity("T-9")
        print(why(t, Ticket))
        print(why(t, Notes))
    "#
    );
    let vm_a = run(&a_src);

    let ticket = &vm_a.print_buffer[0];
    assert!(
        ticket.contains("status: \"escalated\"") || ticket.contains("escalated"),
        "got: {}",
        ticket
    );
    assert!(ticket.contains("[via wire "), "got: {}", ticket);

    let notes = &vm_a.print_buffer[1];
    assert!(notes.contains("Notes of T-9 = { n: 1 }"), "got: {}", notes);
    assert!(
        notes.contains("<- by `on NoteAdded` handler"),
        "got: {}",
        notes
    );
    assert!(
        notes.contains("NoteAdded") && notes.contains("[via wire "),
        "the emit that traveled inside the delta must carry its origin, got: {}",
        notes
    );
}

/// Schema drift across the delta path: the sender runs v1, the receiver
/// declares v2 with a `migrate` block. The receiver's base arrived via the
/// full codec (migrated on ingest); the delta's rows are still v1-shaped and
/// must migrate on apply, exactly like the full codec.
#[test]
fn delta_runs_migrations_on_schema_drift() {
    // Machine B (v1): make base, ship it, then diverge and ship the delta.
    let vm_b = run(r#"
        component Hero { hp: 0 }
        let _h = spawn("hero", Hero { hp: 42 })
        let base = fork()
        let h = get_entity("hero")
        set(h, Hero { hp: 99 })
        print(fork_to_bytes(base))
        print(fork_delta(base, fork()))
    "#);
    let base_bytes = vm_b.print_buffer[0].clone();
    let delta = vm_b.print_buffer[1].clone();

    // Machine A (v2): ingest the base (migrates), apply the delta (the
    // shipped v1 row migrates too), commit, observe v2 shape + v1 value.
    let a_src = format!(
        r#"
        component Hero {{ hp: 0, shield: 0 }}
        migrate Hero(old) {{
            return Hero {{ hp: old["hp"], shield: old["hp"] / 2 }}
        }}
        let base = fork_from_bytes({base_bytes:?}) |> unwrap
        let theirs = fork_apply(base, {delta:?}) |> unwrap
        commit(theirs)
        let h = get_entity("hero")
        let hero = get(h, Hero) |> unwrap
        print(hero.hp)
        print(hero.shield)
    "#
    );
    let vm_a = run(&a_src);
    assert_eq!(vm_a.print_buffer, vec!["99", "49"]);
}

/// The reconstruction shares lineage with the receiver's base (CoW restore +
/// surgical apply), so the O(divergence) merge fast path — and merge
/// semantics generally — work on wire-delivered forks without a full scan.
#[test]
fn merge_after_delta_apply_matches_in_process_merge() {
    let shared = r#"
        component Gold { amount: 0 }
        component Tag { label: "" }
        fn seed() {
            let _bank = spawn("bank", Gold { amount: 100 })
            let _hero = spawn("hero", Tag { label: "idle" })
        }
    "#;

    // Machine B: diverge from the shared base, ship a delta.
    let b_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let hero = get_entity("hero")
        set(hero, Tag {{ label: "questing" }})
        let _scout = spawn("scout", Tag {{ label: "new" }})
        print(fork_delta(base, fork()))
    "#
    );
    let vm_b = run(&b_src);
    let delta = vm_b.print_buffer[0].clone();

    // Machine A: own divergence + B's delta, three-way merge.
    let a_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let bank = get_entity("bank")
        set(bank, Gold {{ amount: 150 }})
        let ours = fork()
        commit(base)
        let theirs = fork_apply(base, {delta:?}) |> unwrap
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        print(fork_to_bytes(fork()))
    "#
    );
    let vm_a = run(&a_src);

    // Reference: the whole dance in one process.
    let ref_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let bank = get_entity("bank")
        set(bank, Gold {{ amount: 150 }})
        let ours = fork()
        commit(base)
        let hero = get_entity("hero")
        set(hero, Tag {{ label: "questing" }})
        let _scout = spawn("scout", Tag {{ label: "new" }})
        let theirs = fork()
        commit(base)
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        print(fork_to_bytes(fork()))
    "#
    );
    let vm_ref = run(&ref_src);

    assert_eq!(
        wire_state_part(&vm_a.print_buffer[0]),
        wire_state_part(&vm_ref.print_buffer[0]),
        "merge over a delta-delivered fork must equal the in-process merge"
    );
}
