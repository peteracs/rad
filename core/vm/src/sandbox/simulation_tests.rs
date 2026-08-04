

/// `simulate_par` determinism and isolation tests.
#[cfg(test)]
mod simulate_par_tests {
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::VM;

    fn run_host(src: &str) -> Vec<String> {
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
        vm.print_buffer.clone()
    }

    #[test]
    fn simulate_par_is_deterministic_per_seed_and_isolated() {
        let out = run_host(
            r#"
            component P { v: 0 }
            system Jitter(p: mut P) {
                p = P { v: p.v + rand_int(1, 1000000) }
            }
            let e = spawn("e", P { v: 0 })
            let f = fork()
            let runs1 = simulate_par(f, [system::Jitter], 5, 4, 42)
            let runs2 = simulate_par(f, [system::Jitter], 5, 4, 42)
            let mut i = 0
            while i < 4 {
                let a = peek(runs1[i], e, P) |> unwrap
                let b = peek(runs2[i], e, P) |> unwrap
                print(f"{a.v}|{b.v}")
                i = i + 1
            }
            let m = get(e, P) |> unwrap
            print(f"main={m.v}")
            "#,
        );
        assert_eq!(out.len(), 5);
        let mut fork_values = Vec::new();
        for line in &out[..4] {
            let (a, b) = line.split_once('|').expect("a|b");
            assert_eq!(a, b, "same (inputs, seed) must be bit-identical: {}", line);
            fork_values.push(a.to_string());
        }
        // Distinct fork indices get distinct derived seeds.
        let distinct: std::collections::HashSet<_> = fork_values.iter().collect();
        assert!(
            distinct.len() > 1,
            "fork seeds should diverge, got {:?}",
            fork_values
        );
        // The live world is never touched by speculation.
        assert_eq!(out[4], "main=0");
    }

    #[test]
    fn simulate_par_multi_tick_writes_accumulate() {
        // Regression guard for the is_worker trap: tick N+1 must observe tick
        // N's writes inside each fork (writes apply directly to the worker's
        // private world instead of being deferred to a command buffer).
        let out = run_host(
            r#"
            component N { v: 0 }
            system Inc(n: mut N) {
                n = N { v: n.v + 1 }
            }
            let e = spawn("e", N { v: 0 })
            let f = fork()
            let runs = simulate_par(f, [system::Inc], 7, 2, 1)
            let a = peek(runs[0], e, N) |> unwrap
            let b = peek(runs[1], e, N) |> unwrap
            print(f"{a.v},{b.v}")
            "#,
        );
        assert_eq!(out, vec!["7,7"]);
    }

    #[test]
    fn fork_with_seeds_a_resource_without_touching_live_world() {
        // dogfood feature seq 150: fork_with overrides a resource in a copy of
        // the fork; the live world's resource is unchanged (no commit()).
        let out = run_host(
            r#"
            resource Policy { rate: 1 }
            component Coin { n: 0 }
            let e = spawn("e", Coin { n: 0 })
            let root = fork()
            let seeded = fork_with(root, Policy { rate: 9 })
            let a = peek_resource(seeded, Policy) |> unwrap
            let b = res(Policy)
            print(f"seeded={a.rate} live={b.rate}")
            "#,
        );
        assert_eq!(out, vec!["seeded=9 live=1"]);
    }

    #[test]
    fn simulate_many_runs_distinct_candidate_forks_in_parallel() {
        // The heterogeneous axis: three candidates seeded to different policy
        // rates, each advanced 4 ticks under the same schedule, evaluated at
        // once. Each future reflects its own seed, and the live world stays 0.
        let out = run_host(
            r#"
            resource Policy { rate: 1 }
            component Coin { n: 0 }
            system Mint(c: mut Coin) { c = Coin { n: c.n + res(Policy).rate } }
            let e = spawn("e", Coin { n: 0 })
            let root = fork()
            let cands = [
                fork_with(root, Policy { rate: 1 }),
                fork_with(root, Policy { rate: 5 }),
                fork_with(root, Policy { rate: 10 }),
            ]
            let futs = simulate_many(cands, [system::Mint], 4, 42)
            let a = peek(futs[0], e, Coin) |> unwrap
            let b = peek(futs[1], e, Coin) |> unwrap
            let c = peek(futs[2], e, Coin) |> unwrap
            let live = get(e, Coin) |> unwrap
            print(f"{a.n},{b.n},{c.n},live={live.n}")
            "#,
        );
        // 4 ticks: rate 1 -> 4, rate 5 -> 20, rate 10 -> 40; live untouched.
        assert_eq!(out, vec!["4,20,40,live=0"]);
    }

    #[test]
    fn simulate_many_is_deterministic_regardless_of_order() {
        // Same list of seeded forks, same seed => bit-identical results,
        // mirroring simulate_par's per-index seeding guarantee.
        let src = r#"
            resource Policy { rate: 2 }
            component Coin { n: 0 }
            system Mint(c: mut Coin) { c = Coin { n: c.n + res(Policy).rate } }
            let e = spawn("e", Coin { n: 0 })
            let root = fork()
            let cands = [fork_with(root, Policy { rate: 3 }), fork_with(root, Policy { rate: 7 })]
            let futs = simulate_many(cands, [system::Mint], 5, 99)
            let a = peek(futs[0], e, Coin) |> unwrap
            let b = peek(futs[1], e, Coin) |> unwrap
            print(f"{a.n},{b.n}")
        "#;
        assert_eq!(run_host(src), run_host(src));
        assert_eq!(run_host(src), vec!["15,35"]);
    }

    #[test]
    fn simulate_par_zero_forks_gives_empty_list() {
        let out = run_host(
            r#"
            component N { v: 0 }
            system Inc(n: mut N) {
                n = N { v: n.v + 1 }
            }
            let e = spawn("e", N { v: 0 })
            let f = fork()
            let runs = simulate_par(f, [system::Inc], 1, 0, 1)
            print(len(runs))
            "#,
        );
        assert_eq!(out, vec!["0"]);
    }

    #[test]
    fn fork_seed_identifies_rollouts_and_simulate_seeded_reproduces_one() {
        // dogfood feature seq 150 follow-on: every simulate_par result knows
        // which effective rng seed produced it (fork_seed), and feeding that
        // seed to simulate_seeded re-runs exactly that rollout in isolation.
        let out = run_host(
            r#"
            component P { v: 0 }
            system Jitter(p: mut P) {
                p = P { v: p.v + rand_int(1, 1000000) }
            }
            let e = spawn("e", P { v: 0 })
            let f = fork()
            let runs = simulate_par(f, [system::Jitter], 3, 3, 42)
            print(f"{fork_seed(runs[0])}|{fork_seed(runs[1])}|{fork_seed(runs[2])}|{fork_seed(f)}")
            let repro = simulate_seeded(f, [system::Jitter], 3, fork_seed(runs[1]))
            let a = peek(runs[1], e, P) |> unwrap
            let b = peek(repro, e, P) |> unwrap
            let same = a.v == b.v
            print(f"reproduced={same}")
            print(f"repro_knows_seed={fork_seed(repro) == fork_seed(runs[1])}")
            "#,
        );
        assert_eq!(out.len(), 3);
        let seeds: Vec<i64> = out[0]
            .split('|')
            .map(|s| s.parse().expect("seed int"))
            .collect();
        // The three rollout seeds are nonzero and pairwise distinct; a plain
        // fork() has no rollout seed (0 is unambiguous — the SplitMix64
        // finalizer never derives 0).
        assert_ne!(seeds[0], 0);
        assert_ne!(seeds[1], 0);
        assert_ne!(seeds[2], 0);
        assert_ne!(seeds[0], seeds[1]);
        assert_ne!(seeds[1], seeds[2]);
        assert_eq!(seeds[3], 0, "plain fork() carries no rollout seed");
        assert_eq!(out[1], "reproduced=true");
        assert_eq!(out[2], "repro_knows_seed=true");
    }

    #[test]
    fn simulate_par_override_list_seeds_candidates_without_commit() {
        // dogfood feature seq 150 #2: the optional 6th argument overrides
        // resources on the base fork at the call site, so a pure search never
        // commit()s a candidate into the live world. The override also marks
        // derived copies as new candidates: fork_with on a rollout result
        // clears its rollout seed.
        let out = run_host(
            r#"
            resource Policy { rate: 1 }
            component Coin { n: 0 }
            system Mint(c: mut Coin) { c = Coin { n: c.n + res(Policy).rate } }
            let e = spawn("e", Coin { n: 0 })
            let f = fork()
            let runs = simulate_par(f, [system::Mint], 4, 2, 7, [Policy { rate: 5 }])
            let a = peek(runs[0], e, Coin) |> unwrap
            let b = peek(runs[1], e, Coin) |> unwrap
            let live = res(Policy)
            print(f"{a.n},{b.n},live={live.rate}")
            let derived = fork_with(runs[0], Policy { rate: 2 })
            print(fork_seed(derived))
            "#,
        );
        // rate 5 for 4 ticks in both rollouts; the live Policy still rate 1.
        assert_eq!(out, vec!["20,20,live=1", "0"]);
    }
}

/// Blast-radius assertion tests (List item #3): `diff` and
/// `assert_only_changed` — testing the negative space.
#[cfg(test)]
mod blast_radius_tests {
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::VM;

    fn run_host(src: &str) -> Result<Vec<String>, String> {
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
        vm.run(0)?;
        Ok(vm.print_buffer.clone())
    }

    const WORLD: &str = r#"
        component Health { hp: 100 }
        component Gold { amount: 1000 }
        component Position { x: 0 }
        system Damage(h: mut Health) {
            h = Health { hp: h.hp - 10 }
        }
        let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 1000 }, Position { x: 0 })
    "#;

    #[test]
    fn diff_reports_only_touched_components() {
        let out = run_host(&format!(
            r#"{WORLD}
            let before = fork()
            let after = simulate(before, [system::Damage], 3)
            let d = diff(before, after)
            print(d["Health"] |> unwrap_or(0))
            print(contains(keys(d), "Gold"))
            print(contains(keys(d), "Position"))
            "#
        ))
        .expect("run");
        assert_eq!(out, vec!["1", "false", "false"]);
    }

    #[test]
    fn diff_of_identical_forks_is_empty() {
        let out = run_host(&format!(
            r#"{WORLD}
            let a = fork()
            let b = fork()
            print(len(diff(a, b)))
            "#
        ))
        .expect("run");
        assert_eq!(out, vec!["0"]);
    }

    #[test]
    fn assert_only_changed_passes_when_within_radius() {
        let out = run_host(&format!(
            r#"{WORLD}
            let before = fork()
            let after = simulate(before, [system::Damage], 1)
            assert_only_changed(before, after, [Health])
            print("ok")
            "#
        ))
        .expect("run");
        assert_eq!(out, vec!["ok"]);
    }

    #[test]
    fn assert_only_changed_fails_outside_radius() {
        // The Damage system writes Health, but the assertion only allows Gold.
        let err = run_host(&format!(
            r#"{WORLD}
            let before = fork()
            let after = simulate(before, [system::Damage], 1)
            assert_only_changed(before, after, [Gold])
            print("unreachable")
            "#
        ))
        .expect_err("assertion must fail");
        assert!(err.contains("unexpected changes"), "got: {}", err);
        assert!(err.contains("Health"), "got: {}", err);
        assert!(err.contains("allowed: [Gold]"), "got: {}", err);
    }

    #[test]
    fn assert_only_changed_event_flow_from_council_sketch() {
        // The ratified one-liner: emit an event, flush, prove the blast
        // radius. String names are accepted alongside component type refs.
        let out = run_host(&format!(
            r#"{WORLD}
            event Hit {{ amount }}
            on Hit(e) {{
                let h = get(hero, Health) |> unwrap
                set(hero, Health {{ hp: h.hp - e.amount }})
            }}
            let before = fork()
            emit Hit {{ amount: 25 }}
            flush_events()
            assert_only_changed(before, fork(), ["Health"])
            let h = get(hero, Health) |> unwrap
            print(h.hp)
            "#
        ))
        .expect("run");
        assert_eq!(out, vec!["75"]);
    }

    #[test]
    fn diff_counts_spawned_and_despawned_rows() {
        let out = run_host(&format!(
            r#"{WORLD}
            let before = fork()
            spawn("goblin", Health {{ hp: 30 }})
            let after = fork()
            let d = diff(before, after)
            print(d["Health"] |> unwrap_or(0))
            "#
        ))
        .expect("run");
        // The goblin lands in a new archetype: 1 new Health row.
        assert_eq!(out, vec!["1"]);
    }

    #[test]
    fn diff_sees_resource_changes() {
        let out = run_host(
            r#"
            resource Score { total: 0 }
            fn main() -> nil {
                let before = fork()
                set_resource(Score, Score { total: 99 })
                let after = fork()
                let d = diff(before, after)
                print(d["Score"] |> unwrap_or(0))
                assert_only_changed(before, after, [Score])
                print("ok")
            }
            "#,
        )
        .expect("run");
        assert_eq!(out, vec!["1", "ok"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_seeds_are_distinct_and_nonzero() {
        let base = 12345u64;
        let a = fork_seed(base, 0);
        let b = fork_seed(base, 1);
        let c = fork_seed(base, 2);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn fork_seed_is_deterministic() {
        assert_eq!(fork_seed(99, 3), fork_seed(99, 3));
    }

    #[test]
    fn caps_write_acl() {
        let mut set = HashSet::new();
        set.insert("Health".to_string());
        let caps = SandboxCaps::new(set, 1000, 1 << 20);
        assert!(caps.may_write("Health"));
        assert!(!caps.may_write("Gold"));
    }

    #[test]
    fn caps_read_defaults_to_wildcard_when_key_absent() {
        // No "read" key => read everything (backward compatible).
        let (caps, _) = SandboxCaps::from_json(r#"{ "write": ["Health"] }"#).unwrap();
        assert!(caps.may_read("Health"));
        assert!(caps.may_read("AnythingElse"));
        assert!(caps.may_read_all());
    }

    #[test]
    fn caps_read_allowlist_is_exact_and_denies_bulk() {
        // An explicit "read" list is an allowlist; it does not include the
        // wildcard, so bulk readers are denied.
        let (caps, _) = SandboxCaps::from_json(r#"{ "write": [], "read": ["Health"] }"#).unwrap();
        assert!(caps.may_read("Health"));
        assert!(!caps.may_read("Vault"));
        assert!(!caps.may_read_all());
    }

    #[test]
    fn caps_empty_read_list_reads_nothing() {
        // Present-but-empty means "read nothing", symmetric with write: [].
        let (caps, _) = SandboxCaps::from_json(r#"{ "read": [] }"#).unwrap();
        assert!(!caps.may_read("Health"));
        assert!(!caps.may_read_all());
    }

    #[test]
    fn caps_read_wildcard_grants_all() {
        let (caps, _) = SandboxCaps::from_json(r#"{ "read": ["*"] }"#).unwrap();
        assert!(caps.may_read("Health"));
        assert!(caps.may_read_all());
    }

    #[test]
    fn caps_read_must_be_array() {
        let err = SandboxCaps::from_json(r#"{ "read": "Health" }"#).unwrap_err();
        assert!(err.contains("'read' must be an array"), "got: {}", err);
    }
}
