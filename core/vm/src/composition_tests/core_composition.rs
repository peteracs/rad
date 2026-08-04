

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