

fn checked_bytebuf_index(value: Value, fn_name: &str) -> Result<usize, String> {
    let idx = value
        .as_int()
        .ok_or_else(|| format!("{} expects an int offset/index", fn_name))?;
    if idx < 0 {
        return Err(format!("{} offset/index must be non-negative", fn_name));
    }
    usize::try_from(idx).map_err(|_| format!("{} offset/index too large", fn_name))
}

fn checked_byte_value(value: Value, fn_name: &str) -> Result<u8, String> {
    let byte = value
        .as_int()
        .ok_or_else(|| format!("{} expects an int byte value", fn_name))?;
    if !(0..=255).contains(&byte) {
        return Err(format!(
            "{} byte value {} out of range 0..255",
            fn_name, byte
        ));
    }
    Ok(byte as u8)
}

#[cfg(test)]
impl VM {
    /// Complete stable-enough signature for adversarial transaction tests.
    /// Pointer identities inside Values are deliberately rendered only while
    /// comparing one reused VM before/after a fault.
    pub(crate) fn observable_state_signature(&self) -> String {
        let mut pending_io: Vec<u64> = self.pending_io.keys().copied().collect();
        pending_io.sort_unstable();
        let frames = self
            .frames
            .iter()
            .map(|frame| {
                (
                    frame.frame_id,
                    frame.chunk_id,
                    frame.ip,
                    frame.stack_base,
                    frame.captures.as_ref().map(|captures| captures.len()),
                    frame.system_writeback.is_some(),
                )
            })
            .collect::<Vec<_>>();
        format!(
            "chunks={};world={};ledger={:?};events={:?}|{:?}|{:?}|{:?};emit_ids={:?}|{:?};globals={:?};output={:?}|{:?};rng={};fuel={};mem={};tasks={:?};next_task={};pending_io={:?};async={};timeline={};event_log={:?};commands={:?};settlement={};next_frame={};next_settlement={};frames={:?};stack={:?};sandbox={:?}|{:?}|{:?}|{};trace={:?}|{};cause={:?}|{};once={}",
            self.chunks.len(),
            self.world.content_digest(),
            self.ledger,
            self.events_current,
            self.events_next,
            self.events_processing,
            self.delayed_events,
            self.emit_ids_current,
            self.emit_ids_next,
            self.globals,
            self.print_buffer,
            self.eprint_buffer,
            self.rng_state,
            self.fuel,
            self.mem_limit,
            self.tasks,
            self.next_task_id,
            pending_io,
            self.in_async_context,
            self.timeline.len(),
            self.event_log,
            self.command_buffer,
            self.settlement.is_some(),
            self.next_frame_id,
            self.next_settlement_id,
            frames,
            self.stack,
            self.sandbox_input_json,
            self.sandbox_output_json,
            self.last_sandbox_output_json,
            self.last_sandbox_fuel_spent,
            self.current_trace_id,
            self.next_trace_id,
            self.current_cause,
            self.causality_frame,
            self.once_guard_passed,
        )
    }
}

/// Regression tests for the ECS scheduling/soundness cluster (dogfood bugs
/// seq 39, 40, 74, 75). They live with the executor because the paths they
/// pin down — the mut-query writeback, the query filter dispatch, and the
/// parallel batch write/event merge — are all implemented here.
#[cfg(test)]
mod scheduling_tests {
    fn run_source(src: &str) -> Vec<String> {
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = crate::parser::Parser::new(tokens).parse();
        let compiler = crate::compiler::Compiler::new();
        let result = compiler.compile(&program).expect("program should compile");
        let mut vm = crate::vm::VM::new();
        vm.load_compile_result(result);
        vm.run(0).expect("program should run");
        vm.print_buffer.clone()
    }

    /// Like `run_source` but through the checker (as `rad file.rad`
    /// compiles), so per-fn effect sets reach the compiler's body-access
    /// analysis.
    fn run_source_checked(src: &str) -> Vec<String> {
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = crate::parser::Parser::new(tokens).parse();
        let mut checker = crate::checker::Checker::new();
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "check errors: {:?}", errors);
        let result = crate::compiler::Compiler::new()
            .with_checker_output(checker.output())
            .compile(&program)
            .expect("program should compile");
        let mut vm = crate::vm::VM::new();
        vm.load_compile_result(result);
        vm.run(0).expect("program should run");
        vm.print_buffer.clone()
    }

    #[test]
    fn query_where_filters_by_component_values_end_to_end() {
        // The readonly-predicate contract, through the full checked
        // pipeline: `get` inside the predicate reads the snapshotted world
        // and the filter returns exactly the matching entities.
        let out = run_source_checked(
            r#"
            component Hero { level: int = 0 }
            let a = spawn("a", Hero { level: 1 })
            let b = spawn("b", Hero { level: 3 })
            let c = spawn("c", Hero { level: 5 })
            let veterans = query_where(Hero, fn(id: entity) -> bool {
                let h = get(id, Hero) |> unwrap
                return h.level >= 3
            })
            print(len(veterans))
            "#,
        );
        assert_eq!(out, vec!["2"]);
    }

    #[test]
    fn query_map_maps_component_values_end_to_end() {
        // query_map's read-only mapper, through the full checked pipeline.
        let out = run_source_checked(
            r#"
            component Hero { level: int = 0 }
            let a = spawn("a", Hero { level: 1 })
            let b = spawn("b", Hero { level: 3 })
            let doubled = query_map(Hero, fn(id: entity) -> int {
                let h = get(id, Hero) |> unwrap
                return h.level * 2
            })
            let mut total = 0
            for v in doubled {
                total = total + v
            }
            print(total)
            "#,
        );
        assert_eq!(out, vec!["8"]);
    }

    #[test]
    fn query_where_read_predicate_inside_simulated_system_is_accepted() {
        // Interaction of the two purity systems: a read-only query_where
        // predicate inside a SYSTEM run by simulate() must pass both the
        // predicate contract (reads ok) and the simulation-purity analysis
        // (reads are legal in simulated systems).
        let out = run_source_checked(
            r#"
            component C { v: int = 0 }
            system Work(c: mut C) {
                let picked = query_where(C, fn(id: entity) -> bool {
                    let row = get(id, C) |> unwrap
                    return row.v >= 0
                })
                c.v = c.v + len(picked)
            }
            let e = spawn("e", C { v: 0 })
            let f = fork()
            let r = simulate(f, [system::Work], 3)
            let got = peek(r, e, C) |> unwrap
            print(got.v)
            "#,
        );
        assert_eq!(out, vec!["3"]);
    }

    /// Like `run_source` but with the `--serial-schedule` lever engaged, so
    /// scheduled systems run one at a time in topological order.
    fn run_source_serial(src: &str) -> Vec<String> {
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = crate::parser::Parser::new(tokens).parse();
        let compiler = crate::compiler::Compiler::new();
        let result = compiler.compile(&program).expect("program should compile");
        let mut vm = crate::vm::VM::new();
        vm.set_serial_schedule(true);
        vm.load_compile_result(result);
        vm.run(0).expect("program should run");
        vm.print_buffer.clone()
    }

    #[test]
    fn serial_schedule_runs_multi_system_phase_correctly() {
        // dogfood feature seq 83: --serial-schedule runs a whole phase one
        // system at a time in topological order (no parallel batch, no merge).
        // Inc writes each entity's own N; Count writes a resource — non-
        // conflicting, so the default path batches them in parallel and the
        // serial path runs them sequentially. Both must give the same, correct
        // answer; the serial run is the correctness-critical/differential mode.
        let src = r#"
            component N { v: 0 }
            resource C { k: 0 }
            system Inc(n: mut N) { n = N { v: n.v + 1 } }
            system Count(c: mut C) { c.k = c.k + 1 }
            let e = spawn("e", N { v: 0 })
            phase P [Inc, Count]
            schedule [P]
            print(f"{(get(e, N) |> unwrap).v},{res(C).k}")
        "#;
        assert_eq!(run_source_serial(src), vec!["1,1"]);
        // Serial mode is behavior-preserving for a well-formed program.
        assert_eq!(run_source_serial(src), run_source(src));
    }

    #[test]
    fn schedule_serial_keyword_runs_and_matches_parallel_result() {
        // dogfood feature seq 83 (per-call spelling): `schedule serial [...]`
        // runs the listed systems one at a time on the main VM — no flag
        // needed — and is behavior-preserving vs the parallel spelling.
        let serial_src = r#"
            component W { n: 1 }
            resource RA { a: 0 }
            resource RB { b: 0 }
            system SA(w: W, r: mut RA) { r.a = r.a + w.n }
            system SB(w: W, r: mut RB) { r.b = r.b + w.n }
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            schedule serial [SA, SB]
            print(f"{res(RA).a},{res(RB).b}")
        "#;
        let parallel_src = &serial_src.replace("schedule serial [", "schedule [");
        assert_eq!(run_source(serial_src), vec!["3,3"]);
        assert_eq!(run_source(serial_src), run_source(parallel_src));
    }

    #[test]
    fn accum_resource_folds_parallel_contributions() {
        // dogfood seq 83 IDEA 02: two `accum` systems of the same resource
        // SHARE a batch (unit-tested in parallel.rs) and the merge folds
        // per-field deltas in schedule order: 3 entities × 2 systems = 6,
        // and the float field accumulates exactly. The serial spelling is
        // the differential check — identical result, no parallel machinery.
        let src = r#"
            component W { n: 1 }
            resource Tally { hits: 0, weight: 0.0 }
            system CountA(w: W, t: accum Tally) {
                t.hits = t.hits + w.n
                t.weight = t.weight + 0.5
            }
            system CountB(w: W, t: accum Tally) { t.hits = t.hits + w.n }
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            schedule [CountA, CountB]
            print(res(Tally).hits)
            print(f"{res(Tally).weight == 1.5}")
        "#;
        assert_eq!(run_source(src), vec!["6", "true"]);
        // Determinism: same program, same answer, every run.
        assert_eq!(run_source(src), run_source(src));
        // Differential: the serial spelling computes the same totals.
        let serial = &src.replace("schedule [", "schedule serial [");
        assert_eq!(run_source(serial), vec!["6", "true"]);
    }

    #[test]
    fn serial_phase_members_run_in_separate_batches_and_stay_correct() {
        // dogfood feature seq 83: `serial phase` members never share a batch
        // (unit-tested in parallel.rs); end to end the declaration parses,
        // compiles, stamps the group, and the schedule still computes the
        // right answer.
        let out = run_source(
            r#"
            component W { n: 1 }
            resource RA { a: 0 }
            resource RB { b: 0 }
            system SA(w: W, r: mut RA) { r.a = r.a + w.n }
            system SB(w: W, r: mut RB) { r.b = r.b + w.n }
            serial phase Line [SA, SB]
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            schedule [Line]
            print(f"{res(RA).a},{res(RB).b}")
        "#,
        );
        assert_eq!(out, vec!["2,2"]);
    }

    /// seq 40: `despawn(id)` inside a `mut` query loop used to die with
    /// "Cannot set component on non-existent entity N" because the
    /// end-of-iteration writeback ran unconditionally.
    #[test]
    fn despawn_inside_mut_query_loop_completes() {
        let out = run_source(
            r#"
            component Pos { y: 0.0 }
            component Vel { dy: 0.0 }
            for i in range(0, 4) {
                spawn(Pos { y: 1.0 - float(i) }, Vel { dy: -1.0 })
            }
            for (id, pos, vel) in query { mut Pos, Vel } {
                pos.y = pos.y + vel.dy
                if pos.y < 0.0 {
                    despawn(id)
                }
            }
            print(len(query { Pos }))
        "#,
        );
        assert_eq!(out, vec!["1"]);
    }

    /// seq 40 (bugs/02b): the crash fired even when the body never assigned
    /// to the `mut` binding — binding mut + despawning was enough.
    #[test]
    fn despawn_with_untouched_mut_binding_completes() {
        let out = run_source(
            r#"
            component A { n: 0 }
            for i in range(0, 4) {
                spawn(A { n: i })
            }
            for (id, a) in query { mut A } {
                if a.n % 2 == 0 {
                    despawn(id)
                }
            }
            print(len(query { A }))
        "#,
        );
        assert_eq!(out, vec!["2"]);
    }

    /// seq 75: `query { mut X } where <cond>` used to die at runtime with
    /// "QueryFilter: expected closure or function" — the mut-loop lowering
    /// pushed the filter chunk id as a bare int instead of a fn value.
    #[test]
    fn mut_query_with_where_filters_and_writes_back() {
        let out = run_source(
            r#"
            component Fsm { n: 0 }
            for i in range(0, 6) {
                spawn(Fsm { n: i })
            }
            let mut hits = 0
            for (_id, f) in query { mut Fsm } where Fsm.n > 2 {
                f.n = f.n + 10
                hits = hits + 1
            }
            print(hits)
            print(len(query { Fsm } where Fsm.n > 12))
        "#,
        );
        assert_eq!(out, vec!["3", "3"]);
    }

    /// seq 75: a filter that captures an enclosing local exercises the
    /// closure (upvalue) packaging path rather than the plain-fn path.
    #[test]
    fn mut_query_with_capturing_where_filter() {
        let out = run_source(
            r#"
            component Fsm { n: 0 }
            for i in range(0, 6) {
                spawn(Fsm { n: i })
            }
            let threshold = 3
            let mut hits = 0
            for (_id, f) in query { mut Fsm } where Fsm.n >= threshold {
                f.n = f.n + 100
                hits = hits + 1
            }
            print(hits)
            print(len(query { Fsm } where Fsm.n >= 100))
        "#,
        );
        assert_eq!(out, vec!["3", "3"]);
    }

    /// seq 39: a per-entity `mut` resource accumulator in a PARALLEL batch
    /// collapsed to a single increment (each iteration recomputed from the
    /// snapshot). Both spellings must count all ten entities.
    #[test]
    fn parallel_batch_resource_accumulation_counts_every_entity() {
        let out = run_source(
            r#"
            component Tag { probe: 0 }
            resource A { n: 0 }
            resource B { n: 0 }
            system BumpA(_t: Tag, a: mut A) { a.n = a.n + 1 }
            system BumpB(_t: Tag, b: mut B) { b.n = b.n + 1 }
            for _i in range(0, 10) {
                spawn(Tag { probe: 0 })
            }
            schedule [BumpA, BumpB]
            print(res(A).n)
            print(res(B).n)
        "#,
        );
        assert_eq!(out, vec!["10", "10"]);
    }

    /// seq 45: a resource written with `update(R)` in a system body used to
    /// be invisible to parallel conflict analysis, so the pair below shared
    /// a batch and one write was silently lost (R1 = 1 instead of 101).
    #[test]
    fn update_in_system_body_conflicts_with_mut_param() {
        let out = run_source(
            r#"
            resource R1 { n: 0 }
            resource D1 { x: 0 }
            resource R2 { n: 0 }
            resource D2 { x: 0 }
            resource D3 { x: 0 }
            system MutParam(r: mut R1) { r.n = r.n + 100 }
            system ViaUpdate(_d: mut D1) { update(R1) { n = res(R1).n + 1 } }
            system UpdA(_d: mut D2) { update(R2) { n = res(R2).n + 100 } }
            system UpdB(_d: mut D3) { update(R2) { n = res(R2).n + 1 } }
            schedule [MutParam, ViaUpdate]
            print(res(R1).n)
            schedule [UpdA, UpdB]
            print(res(R2).n)
        "#,
        );
        assert_eq!(out, vec!["101", "101"]);
    }

    /// seq 45 (case 3): the `update(R)` hidden one call frame deep in a
    /// helper fn — the shape real code has. The helper's checker effects
    /// mark it as a potential ECS writer, which must serialize the caller
    /// against the conflicting `mut` param system.
    #[test]
    fn update_via_helper_fn_serializes_against_mut_param() {
        let out = run_source_checked(
            r#"
            resource R3 { n: 0 }
            resource D1 { x: 0 }
            fn bump3() { update(R3) { n = res(R3).n + 1 } }
            system MutParam3(r: mut R3) { r.n = r.n + 100 }
            system ViaFn(_d: mut D1) { bump3() }
            schedule [MutParam3, ViaFn]
            print(res(R3).n)
        "#,
        );
        assert_eq!(out, vec!["101"]);
    }

    /// A2 seq 124/143 (memory corruption): a pooled worker VM kept its
    /// CREATION-time copy of the main VM's globals whenever the program
    /// (chunks Arc) matched. Global values are main-GC heap handles, and
    /// top-level `let mut` rebinding turns the old objects into garbage the
    /// main collector frees — after which the worker's own collector traced
    /// the stale handles as roots and dereferenced freed memory (the
    /// simulate_par 0xC0000005). sync_from_shared must refresh globals on
    /// EVERY sync.
    #[test]
    fn worker_sync_refreshes_globals_from_shared_state() {
        let src = r#"
            let mut g = 1
            print(g)
        "#;
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = crate::parser::Parser::new(tokens).parse();
        let compiler = crate::compiler::Compiler::new();
        let result = compiler.compile(&program).expect("program should compile");
        let mut vm = crate::vm::VM::new();
        vm.load_compile_result(result);
        vm.run(0).expect("program should run");

        let slot = vm
            .global_names
            .iter()
            .position(|n| n == "g")
            .expect("global g should exist");
        assert_eq!(vm.globals[slot].as_int(), Some(1));

        // Worker pooled while `g` was 1...
        let mut worker = crate::vm::VM::from_shared_state(vm.shared_state());
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(
            worker.io_pool.worker_count(),
            0,
            "parallel worker VMs must not own nested I/O threads"
        );
        // ...main rebinding makes a new value current (the old one would be
        // garbage on the main heap)...
        let new_val = crate::value::Value::from_int(&mut vm.gc, 42);
        vm.globals[slot] = new_val;
        // ...and the next sync against the SAME program must adopt it.
        let shared = vm.shared_state();
        worker.sync_from_shared(&shared);
        assert_eq!(
            worker.globals[slot].as_int(),
            Some(42),
            "pooled worker kept a stale creation-time global"
        );
    }

    /// A2 seq 124/143, end-to-end shape: generations of commit() +
    /// simulate_par() + peeks off result forks, with rebound top-level
    /// globals in between. Deterministic by seeding, so two runs must agree
    /// (and not die with an access violation, as this shape did ~1 in 3).
    #[test]
    fn simulate_par_generations_with_peeks_are_stable() {
        const SRC: &str = r#"
            resource Bank { gold: int = 100 }
            component Body { tag: str = "", hp: int = 10 }
            system Grow(b: mut Body) { b.hp = b.hp + 1 }
            system Drift(b: mut Body) after Grow {
                let r = rand_int(-2, 2)
                b.hp = b.hp + r
            }
            system Earn(b: Body, k: mut Bank) after Drift { k.gold = k.gold + b.hp }

            let mut ents = []
            for i in range(3) {
                ents = push(ents, spawn(f"e{i}", Body { tag: f"body-{i}", hp: 10 + i }))
            }
            let mut beam = [fork()]
            let mut acc = 0
            for gen in range(2) {
                let mut next = []
                for cand in range(3) {
                    commit(beam[0])
                    let outs = simulate_par(fork(), [system::Grow, system::Drift, system::Earn], 3, 4, 77 + gen * 31 + cand)
                    for f in outs {
                        let k = peek_resource(f, Bank) |> unwrap
                        acc = acc + k.gold
                        for e in ents {
                            let b = peek(f, e, Body) |> unwrap
                            acc = acc + b.hp + len(b.tag)
                        }
                    }
                    next = push(next, outs[0])
                }
                beam = [next[0]]
            }
            print(acc)
        "#;
        let first = run_source(SRC);
        let second = run_source(SRC);
        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert!(first[0].parse::<i64>().is_ok(), "acc should be an int");
    }

    /// seq 74: the same parallel schedule must deliver parallel-emitted
    /// events in the same order on every run (spec §7.2: writes merge in
    /// schedule order; events sort by trace id, then event name). Pooled
    /// worker VMs used to carry their trace-id counter from whatever task
    /// last ran on the same rayon thread, so the sort order — and therefore
    /// handler outcomes — changed between runs.
    #[test]
    fn parallel_emitted_events_are_ordered_deterministically() {
        const SRC: &str = r#"
            component T { k: 0 }
            resource Log { s: "" }
            resource GA { n: 0 }
            resource GB { n: 0 }
            event EvA { k: int }
            event EvB { k: int }
            system SysA(t: T, _g: mut GA) { emit EvA { k: t.k } }
            system SysB(t: T, _g: mut GB) { emit EvB { k: t.k } }
            on EvA(e) { update(Log) { s = res(Log).s + "A" + str(e.k) } }
            on EvB(e) { update(Log) { s = res(Log).s + "B" + str(e.k) } }
            for i in range(0, 5) {
                spawn(T { k: i })
            }
            for _t in range(0, 20) {
                schedule [SysA, SysB]
                flush_events()
            }
            print(res(Log).s)
        "#;
        // Per tick: five entities, ids ascending; per emission index the
        // sort places EvA before EvB ("EvA" < "EvB").
        let per_tick = "A0B0A1B1A2B2A3B3A4B4";
        let want = vec![per_tick.repeat(20)];
        let first = run_source(SRC);
        let second = run_source(SRC);
        assert_eq!(first, want);
        assert_eq!(second, want);
    }
}
