/// `explore`, `offer`, `follow`: exhaustive search over forked worlds (GH 0012).
///
/// The expected numbers come from `projects/dogfood/explore/oracle.py`, a
/// breadth-first search that shares no code with the VM.
#[cfg(test)]
mod explore_tests {
    use crate::checker::CheckerOptions;
    use crate::parser::ParserOptions;

    fn run(source: &str) -> Result<Vec<String>, String> {
        crate::test_support::run_checked_source(source, ParserOptions, CheckerOptions::default())
    }

    fn lines(source: &str) -> Vec<String> {
        run(source).unwrap_or_else(|error| panic!("program failed:\n{error}"))
    }

    fn failure(source: &str) -> String {
        match run(source) {
            Ok(output) => panic!("expected a failure, got {output:?}"),
            Err(error) => error,
        }
    }

    /// Two jugs. `Rules` rides in the world so one program covers a solvable
    /// and an unsolvable instance.
    const JUGS: &str = r#"
        resource Rules { cap_a: int = 3, cap_b: int = 5, target: int = 4 }
        resource Jugs { a: int = 0, b: int = 0, done: bool = false }
        ecs fn pour(rules: Rules, a: int, b: int, label: str) -> nil {
            set_resource(Jugs, Jugs { a: a, b: b, done: a == rules.target or b == rules.target })
            offer(label)
        }
        system Pour(rules: Rules, j: Jugs, writes Jugs) {
            pour(rules, rules.cap_a, j.b, "fill a")
            pour(rules, j.a, rules.cap_b, "fill b")
            pour(rules, 0, j.b, "empty a")
            pour(rules, j.a, 0, "empty b")
            let ab = min(j.a, rules.cap_b - j.b)
            pour(rules, j.a - ab, j.b + ab, "a into b")
            let ba = min(j.b, rules.cap_a - j.a)
            pour(rules, j.a + ba, j.b - ba, "b into a")
        }
    "#;

    fn jugs(tail: &str) -> String {
        format!("{JUGS}\n{tail}")
    }

    #[test]
    fn explore_finds_a_shortest_path_and_follow_replays_it() {
        let out = lines(&jugs(
            r#"
            let start = fork()
            let found = explore(start, [system::Pour], {"goal": "Jugs.done"})
            print(found["status"])
            print(join(found["path"], " > "))
            print(f"seen={found["seen"]} depth={found["depth"]}")
            let end = follow(start, [system::Pour], found["path"])
            let jugs = peek_resource(end, Jugs) |> unwrap
            print(f"followed done={jugs.done} same={world_digest(end) == world_digest(found["world"])}")
            let live = get_resource(Jugs) |> unwrap
            print(f"live={live.a + live.b}")
            "#,
        ));
        assert_eq!(
            out,
            [
                "goal",
                "fill b > b into a > empty a > b into a > fill b > b into a",
                "seen=14 depth=6",
                "followed done=true same=true",
                // Searching and following never touch the live world.
                "live=0",
            ]
        );
    }

    #[test]
    fn explore_without_a_goal_visits_every_world_once() {
        let out = lines(&jugs(
            r#"
            let every = explore(fork(), [system::Pour])
            print(f"{every["status"]} seen={every["seen"]} expanded={every["expanded"]} frontier={every["frontier"]} depth={every["depth"]}")
            print(every["levels"])
            print(len(every["path"]))
            "#,
        ));
        assert_eq!(
            out,
            [
                "exhausted seen=16 expanded=16 frontier=0 depth=7",
                "[1, 2, 3, 2, 2, 2, 2, 2]",
                "0",
            ]
        );
    }

    #[test]
    fn exhausted_means_no_reachable_world_is_a_goal() {
        // Jugs of 2 and 4 only ever hold even amounts.
        let out = lines(&jugs(
            r#"
            let even = fork_with(fork(), Rules { cap_a: 2, cap_b: 4, target: 3 })
            let never = explore(even, [system::Pour], {"goal": "Jugs.done"})
            print(f"{never["status"]} seen={never["seen"]} depth={never["depth"]}")
            print(never["levels"])
            "#,
        ));
        assert_eq!(out, ["exhausted seen=6 depth=2", "[1, 2, 3]"]);
    }

    #[test]
    fn limit_and_depth_stop_the_search_and_the_status_says_which() {
        let out = lines(&jugs(
            r#"
            let start = fork()
            let few = explore(start, [system::Pour], {"limit": 5})
            print(f"{few["status"]} seen={few["seen"]} waiting={few["frontier"] > 0}")
            let near = explore(start, [system::Pour], {"depth": 2})
            print(f"{near["status"]} seen={near["seen"]} frontier={near["frontier"]}")
            print(near["levels"])
            let nothing = explore(start, [system::Pour], {"depth": 0})
            print(f"{nothing["status"]} seen={nothing["seen"]}")
            "#,
        ));
        assert_eq!(
            out,
            [
                "limit seen=5 waiting=true",
                "depth seen=6 frontier=0",
                "[1, 2, 3]",
                "depth seen=1",
            ]
        );
    }

    /// A score that jumps around: a successor often outranks the worlds
    /// already waiting, which is the case the batch has to give way for.
    const HOPS: &str = r#"
        resource Spot { n: int = 1, rank: int = 0, done: bool = false }
        ecs fn hop(n: int, label: str) -> nil {
            set_resource(Spot, Spot { n: n, rank: (n * 7919) % 13, done: n == 97 })
            offer(label)
        }
        system Step(s: Spot, writes Spot) {
            if s.n * 2 <= 200 { hop(s.n * 2, "double") }
            if s.n + 3 <= 200 { hop(s.n + 3, "plus3") }
            if s.n > 1 { hop(s.n - 1, "minus1") }
        }
    "#;

    #[test]
    fn the_answer_does_not_depend_on_how_many_worlds_are_expanded_ahead() {
        let out = lines(&format!(
            "{HOPS}\n{}",
            r#"
            let start = fork()
            for batch in [1, 2, 7, 256] {
                let found = explore(start, [system::Step], {
                    "goal": "Spot.done", "score": "Spot.rank", "key": "Spot.n", "batch": batch
                })
                print(f"{found["status"]} seen={found["seen"]} expanded={found["expanded"]} frontier={found["frontier"]} {join(found["path"], ",")}")
            }
            for batch in [1, 2, 7, 256] {
                let every = explore(start, [system::Step], {"score": "Spot.rank", "key": "Spot.n", "batch": batch})
                print(f"{every["status"]} seen={every["seen"]} expanded={every["expanded"]} {every["levels"]}")
            }
            "#
        ));
        assert_eq!(out.len(), 8);
        assert!(out[0].starts_with("goal "), "{}", out[0]);
        for line in &out[1..4] {
            assert_eq!(line, &out[0], "a goal search changed with the batch size");
        }
        assert!(
            out[4].starts_with("exhausted seen=200 expanded=200 "),
            "{}",
            out[4]
        );
        for line in &out[5..] {
            assert_eq!(
                line, &out[4],
                "an exhaustive search changed with the batch size"
            );
        }
    }

    #[test]
    fn the_answer_does_not_depend_on_how_far_a_worker_expands_ahead() {
        // `ahead` is the other speed knob: successors that jump the queue are
        // expanded in the worker that produced them. 0 turns that off.
        let out = lines(&format!(
            "{HOPS}\n{}",
            r#"
            let start = fork()
            for pair in [[1, 0], [1, 1], [3, 2], [256, 0], [256, 32], [7, 500]] {
                let found = explore(start, [system::Step], {
                    "goal": "Spot.done", "score": "Spot.rank", "key": "Spot.n",
                    "batch": pair[0], "ahead": pair[1]
                })
                print(f"{found["status"]} seen={found["seen"]} expanded={found["expanded"]} frontier={found["frontier"]} {join(found["path"], ",")}")
            }
            for pair in [[1, 0], [1, 1], [3, 2], [256, 0], [256, 32], [7, 500]] {
                let every = explore(start, [system::Step], {
                    "score": "Spot.rank", "key": "Spot.n", "collect": "Spot.n", "limit": 120,
                    "batch": pair[0], "ahead": pair[1]
                })
                print(f"{every["status"]} seen={every["seen"]} expanded={every["expanded"]} frontier={every["frontier"]} {every["levels"]} {every["collected"]}")
            }
            "#
        ));
        assert_eq!(out.len(), 12);
        assert!(out[0].starts_with("goal "), "{}", out[0]);
        for line in &out[1..6] {
            assert_eq!(line, &out[0], "a goal search changed with batch or ahead");
        }
        assert!(out[6].starts_with("limit seen=120 "), "{}", out[6]);
        for line in &out[7..] {
            assert_eq!(
                line, &out[6],
                "a limited search changed with batch or ahead"
            );
        }
    }

    /// From the start, "trap" leads to a cheap world whose moves fail and
    /// "win" (or "on") to a dearer one.
    fn trap(win_is_goal: bool, options: &str) -> String {
        format!(
            r#"
            resource Spot {{ n: int = 0, rank: int = 10, done: bool = false }}
            ecs fn hop(n: int, rank: int, done: bool, label: str) -> nil {{
                set_resource(Spot, Spot {{ n: n, rank: rank, done: done }})
                offer(label)
            }}
            system Step(s: Spot, writes Spot) {{
                assert(s.n != 1, "the trap world was expanded")
                if s.n == 0 {{
                    hop(1, 1, false, "trap")
                    hop(2, 5, {win_is_goal}, "win")
                }}
            }}
            let found = explore(fork(), [system::Step], {{
                "goal": "Spot.done", "score": "Spot.rank", "key": "Spot.n", {options}
            }})
            print(f"{{found["status"]}} {{join(found["path"], ",")}} seen={{found["seen"]}}")
            "#
        )
    }

    #[test]
    fn expanding_ahead_never_raises_an_error_the_search_does_not_reach() {
        // The goal is generated in the same expansion as the trap, so the
        // search stops before the trap's turn. A worker that expanded the
        // trap ahead must keep its failure to itself.
        for options in [
            r#""ahead": 0"#,
            r#""ahead": 32"#,
            r#""batch": 1, "ahead": 4"#,
        ] {
            assert_eq!(
                lines(&trap(true, options)),
                ["goal win seen=3"],
                "{options}"
            );
        }
        // Without the goal the search does reach the trap, and says so.
        for options in [r#""ahead": 0"#, r#""ahead": 32"#] {
            let error = failure(&trap(false, options));
            assert!(
                error.contains("the trap world was expanded"),
                "{options}: {error}"
            );
            assert!(error.contains("move system 'Step'"), "{options}: {error}");
        }
    }

    #[test]
    fn a_best_first_path_is_a_real_path() {
        // Not the shortest route, but `follow` must still arrive.
        let out = lines(&format!(
            "{HOPS}\n{}",
            r#"
            let start = fork()
            let found = explore(start, [system::Step], {"goal": "Spot.done", "score": "Spot.rank", "key": "Spot.n"})
            let end = peek_resource(follow(start, [system::Step], found["path"]), Spot) |> unwrap
            print(f"{end.n} {end.done}")
            "#
        ));
        assert_eq!(out, ["97 true"]);
    }

    #[test]
    fn a_system_that_never_offers_is_one_move_named_after_itself() {
        // Every interleaving of two systems; the shortest that loses an update.
        let out = lines(
            r#"
            resource Account { balance: int = 100 }
            resource Audit { broken: bool = false }
            component Job { amount: int = 0, pc: int = 0, read: int = 0 }
            readonly ecs fn audit() -> nil {
                let a = get(require_entity("alice"), Job) |> unwrap
                let b = get(require_entity("bob"), Job) |> unwrap
                let account = get_resource(Account) |> unwrap
                let finished = a.pc == 2 and b.pc == 2
                set_resource(Audit, Audit { broken: finished and account.balance != 100 + a.amount + b.amount })
            }
            readonly ecs fn racy_step(who: str) -> nil {
                let worker = require_entity(who)
                let job = get(worker, Job) |> unwrap
                if job.pc == 0 {
                    let account = get_resource(Account) |> unwrap
                    set(worker, Job { pc: 1, read: account.balance, ..job })
                }
                if job.pc == 1 {
                    set_resource(Account, Account { balance: job.read + job.amount })
                    set(worker, Job { pc: 2, ..job })
                }
                audit()
            }
            readonly ecs fn atomic_step(who: str) -> nil {
                let worker = require_entity(who)
                let job = get(worker, Job) |> unwrap
                if job.pc == 0 {
                    let account = get_resource(Account) |> unwrap
                    set_resource(Account, Account { balance: account.balance + job.amount })
                    set(worker, Job { pc: 2, ..job })
                }
                audit()
            }
            system Alice(reads *, writes Job, writes Account, writes Audit) { racy_step("alice") }
            system Bob(reads *, writes Job, writes Account, writes Audit) { racy_step("bob") }
            system AliceAtomic(reads *, writes Job, writes Account, writes Audit) { atomic_step("alice") }
            system BobAtomic(reads *, writes Job, writes Account, writes Audit) { atomic_step("bob") }

            let _ = spawn("alice", Job { amount: 10 })
            let _ = spawn("bob", Job { amount: 20 })
            let start = fork()
            let racy = explore(start, [system::Alice, system::Bob], {"goal": "Audit.broken"})
            let after = peek_resource(racy["world"], Account) |> unwrap
            print(f"{racy["status"]} seen={racy["seen"]} {join(racy["path"], " ")} balance={after.balance}")
            let atomic = explore(start, [system::AliceAtomic, system::BobAtomic], {"goal": "Audit.broken"})
            print(f"{atomic["status"]} seen={atomic["seen"]}")
            "#,
        );
        assert_eq!(
            out,
            [
                "goal seen=12 Alice Bob Alice Bob balance=120",
                "exhausted seen=4"
            ]
        );
    }

    #[test]
    fn events_a_move_emits_are_delivered_in_the_world_it_offers() {
        let out = lines(
            r#"
            resource Bell { pulled: bool = false, heard: bool = false }
            event Ring { }
            on Ring(e) {
                let bell = get_resource(Bell) |> unwrap
                set_resource(Bell, Bell { heard: true, ..bell })
            }
            system Pull(bell: Bell, writes Bell, emits Ring) {
                if not bell.pulled {
                    set_resource(Bell, Bell { pulled: true, ..bell })
                    emit Ring { }
                    offer("pull")
                }
            }
            let found = explore(fork(), [system::Pull], {"goal": "Bell.heard"})
            print(f"{found["status"]} {join(found["path"], ",")} seen={found["seen"]}")
            let live = get_resource(Bell) |> unwrap
            print(f"live heard={live.heard}")
            "#,
        );
        assert_eq!(out, ["goal pull seen=2", "live heard=false"]);
    }

    #[test]
    fn entity_worlds_are_told_apart_without_a_key() {
        // Two counters, each 0..2: nine worlds, and (1, 0) is not (0, 1).
        let out = lines(
            r#"
            component Count { v: int = 0 }
            readonly ecs fn bump(who: str) -> nil {
                let e = require_entity(who)
                let c = get(e, Count) |> unwrap
                if c.v < 2 {
                    set(e, Count { v: c.v + 1 })
                    offer(who)
                }
            }
            system Bump(reads *, writes Count) {
                bump("left")
                bump("right")
            }
            let _ = spawn("left", Count { v: 0 })
            let _ = spawn("right", Count { v: 0 })
            let every = explore(fork(), [system::Bump])
            print(f"{every["status"]} seen={every["seen"]} {every["levels"]}")
            "#,
        );
        assert_eq!(out, ["exhausted seen=9 [1, 2, 3, 2, 1]"]);
    }

    #[test]
    fn offer_outside_an_exploration_is_an_error() {
        let error = failure(r#"offer("nowhere")"#);
        assert!(error.contains("offer() only means something"), "{error}");
    }

    #[test]
    fn offer_refuses_a_system_whose_writes_land_after_the_rewind() {
        let error = failure(
            r#"
            resource Tally { n: int = 0 }
            system Count(t: mut Tally) {
                t = Tally { n: t.n + 1 }
                offer("count")
            }
            let found = explore(fork(), [system::Count])
            print(found["status"])
            "#,
        );
        assert!(error.contains("`mut` parameters"), "{error}");
        assert!(error.contains("move system 'Count'"), "{error}");
    }

    #[test]
    fn two_moves_from_one_world_may_not_share_a_label() {
        let error = failure(
            r#"
            resource Tally { n: int = 0 }
            system Count(t: Tally, writes Tally) {
                set_resource(Tally, Tally { n: t.n + 1 })
                offer("step")
                set_resource(Tally, Tally { n: t.n + 2 })
                offer("step")
            }
            let found = explore(fork(), [system::Count], {"limit": 10})
            print(found["status"])
            "#,
        );
        assert!(error.contains("both labelled \"step\""), "{error}");
    }

    #[test]
    fn a_misspelt_option_or_field_is_reported_before_the_search() {
        let unknown = failure(&jugs(
            r#"let r = explore(fork(), [system::Pour], {"gaol": "Jugs.done"})"#,
        ));
        assert!(unknown.contains("has no option \"gaol\""), "{unknown}");

        let field = failure(&jugs(
            r#"let r = explore(fork(), [system::Pour], {"goal": "Jugs.finished"})"#,
        ));
        assert!(field.contains("has no field 'finished'"), "{field}");

        let resource = failure(&jugs(
            r#"let r = explore(fork(), [system::Pour], {"goal": "Jug.done"})"#,
        ));
        assert!(resource.contains("reads resource 'Jug'"), "{resource}");

        let kind = failure(&jugs(
            r#"let r = explore(fork(), [system::Pour], {"goal": "Jugs.a"})"#,
        ));
        assert!(kind.contains("to be a bool, got int"), "{kind}");

        let collect = failure(&jugs(
            r#"let r = explore(fork(), [system::Pour], {"collect": "Jugs.done"})"#,
        ));
        assert!(
            collect.contains("to be a str or an int, got bool"),
            "{collect}"
        );
    }

    #[test]
    fn collect_returns_one_value_for_every_world_in_the_order_reached() {
        let out = lines(&jugs(
            r#"
            let start = fork()
            let every = explore(start, [system::Pour], {"collect": "Jugs.b"})
            print(f"{every["status"]} seen={every["seen"]} collected={len(every["collected"])}")
            print(slice(every["collected"], 0, 6))
            let some = explore(start, [system::Pour], {"collect": "Jugs.b", "limit": 5})
            print(f"{some["status"]} {some["collected"]}")
            let found = explore(start, [system::Pour], {"goal": "Jugs.done", "collect": "Jugs.b"})
            let reached = found["collected"]
            print(f"{found["status"]} seen={found["seen"]} collected={len(reached)} last={reached[len(reached) - 1]}")
            let plain = explore(start, [system::Pour])
            print(contains(plain, "collected"))
            "#,
        ));
        assert_eq!(
            out,
            [
                "exhausted seen=16 collected=16",
                // The start, then the worlds one move away in offered order,
                // then those two moves away.
                "[0, 0, 5, 5, 3, 2]",
                "limit [0, 0, 5, 5, 3]",
                // The goal world is the last one reached.
                "goal seen=14 collected=14 last=4",
                "false",
            ]
        );
    }

    #[test]
    fn follow_rejects_a_path_the_moves_do_not_offer() {
        let error = failure(&jugs(
            r#"let end = follow(fork(), [system::Pour], ["fill a", "drink"])"#,
        ));
        assert!(error.contains("step 2 of the path is \"drink\""), "{error}");
    }

    #[test]
    fn a_move_system_may_not_use_randomness() {
        let error = failure(
            r#"
            resource Tally { n: int = 0 }
            system Roll(t: Tally, writes Tally, io true) {
                set_resource(Tally, Tally { n: rand_int(1, 6) })
                offer("roll")
            }
            let found = explore(fork(), [system::Roll])
            "#,
        );
        assert!(error.contains("cannot be used in explore()"), "{error}");
    }

    // GH 0018: an unannotated recursive helper is pure when its body is.
    #[test]
    fn a_move_system_may_call_a_recursive_pure_helper() {
        let out = lines(
            r#"
            resource S { n: int = 0 }
            fn depth(k: int) -> int {
                if k == 0 { return 0 }
                return 1 + depth(k - 1)
            }
            fn even(k: int) -> bool {
                if k == 0 { return true }
                return odd(k - 1)
            }
            fn odd(k: int) -> bool {
                if k == 0 { return false }
                return even(k - 1)
            }
            ecs fn step(n: int) -> nil {
                set_resource(S, S { n: n })
                offer("next")
            }
            system Count(s: S, writes S) {
                if s.n < 3 and even(2) { step(s.n + depth(1)) }
            }
            let r = explore(fork(), [system::Count], {})
            print(f"{r["status"]} seen={r["seen"]}")
            "#,
        );
        assert_eq!(out, vec!["exhausted seen=4"]);
    }

    // GH 0018: optimism about recursion must not hide IO further down.
    #[test]
    fn a_recursive_helper_that_reaches_io_is_still_refused() {
        let error = failure(
            r#"
            resource S { n: int = 0 }
            fn loud(k: int) -> int {
                if k == 0 { print("bottom") return 0 }
                return 1 + loud(k - 1)
            }
            ecs fn step(n: int) -> nil {
                set_resource(S, S { n: n })
                offer("next")
            }
            system Count(s: S, writes S) {
                if s.n < 3 { step(s.n + loud(1)) }
            }
            let r = explore(fork(), [system::Count], {})
            "#,
        );
        assert!(error.contains("cannot be used in explore()"), "{error}");
    }

    #[test]
    fn exploring_inside_a_simulation_is_refused() {
        let error = failure(&jugs(
            r#"
            resource Report { ran: bool = false }
            system Nested(reads *, writes *) {
                let found = explore(fork(), [system::Pour])
                set_resource(Report, Report { ran: true })
            }
            let after = simulate(fork(), [system::Nested], 1)
            "#,
        ));
        assert!(error.contains("cannot run inside a simulation"), "{error}");
    }

    #[test]
    fn the_exploration_builtins_are_denied_in_the_sandbox() {
        use crate::value::Builtin;
        for builtin in [Builtin::Explore, Builtin::Offer, Builtin::Follow] {
            assert!(
                !crate::sandbox::builtin_allowed_in_sandbox(builtin),
                "{} must be denied by name",
                builtin.name()
            );
        }
    }
}
