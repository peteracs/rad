//! Honest numbers, measured — not projected.
//!
//! Run with:
//!   cargo test -p rad-vm --release bench_everything -- --ignored --nocapture
//!
//! Each benchmark runs a full rad program twice — once with only the setup,
//! once with the operation repeated R times — and reports
//! `(t_full - t_setup) / R`. That methodology includes interpreter dispatch
//! (it is what a user pays), excludes compile time, and survives noise by
//! repetition. Numbers land in `docs/src/reference/benchmark-results.md`.

#![cfg(test)]

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::vm::VM;
use std::time::Instant;

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

/// Run a compiled program, return (wall seconds, vm).
fn timed_run(src: &str) -> (f64, VM) {
    let result = compile(src);
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(7);
    vm.load_compile_result(result);
    let t = Instant::now();
    vm.run(0).expect("run");
    (t.elapsed().as_secs_f64(), vm)
}

const DECLS: &str = r#"
component Pos { x: 0.0, y: 0.0 }
component Hp { hp: 100, max: 100 }
resource Tally { n: 0 }
event Ping { amount }
on Ping(p) {
    let t = get_resource(Tally) |> unwrap
    set_resource(Tally, Tally { n: t.n + p.amount })
}
"#;

fn world_setup(n: usize) -> String {
    format!(
        r#"
    for i in range(0, {n}) {{
        let e = spawn(Pos {{ x: float(i), y: 0.5 }})
        set(e, Hp {{ hp: i }})
    }}
"#,
        n = n
    )
}

#[test]
#[ignore = "benchmark: run explicitly with --release --ignored --nocapture"]
fn bench_everything() {
    let n = 10_000;
    let setup = world_setup(n);
    println!();
    println!(
        "== rad benchmarks (N = {} entities, 2 components each) ==",
        n
    );

    // -- world construction ------------------------------------------------
    {
        let src = format!("{}\nfn main() -> nil {{\n{}\n}}", DECLS, setup);
        let (t, _) = timed_run(&src);
        println!(
            "spawn+set {} entities          {:>10.1} ms  ({:.2} us/entity)",
            n,
            t * 1e3,
            t * 1e6 / n as f64
        );
    }

    let with_decls = |body: &str| format!("{}\n{}", DECLS, body);

    // -- fork --------------------------------------------------------------
    {
        let setup_src = with_decls(&format!("fn main() -> nil {{\n{}\n}}", setup));
        let full_src = with_decls(&format!(
            "fn main() -> nil {{\n{}\nfor r in range(0, 100) {{ let _f = fork() }}\n}}",
            setup
        ));
        let (t_base, _) = timed_run(&setup_src);
        let (t_full, _) = timed_run(&full_src);
        println!(
            "fork() @10k                    {:>10.1} us",
            (t_full - t_base).max(0.0) / 100.0 * 1e6
        );
    }

    // -- commit ------------------------------------------------------------
    {
        let op = "let f = fork()\nfor r in range(0, 100) { commit(f) }";
        let setup_src = with_decls(&format!(
            "fn main() -> nil {{\n{}\nlet f = fork()\n}}",
            setup
        ));
        let full_src = with_decls(&format!("fn main() -> nil {{\n{}\n{}\n}}", setup, op));
        let (t_base, _) = timed_run(&setup_src);
        let (t_full, _) = timed_run(&full_src);
        println!(
            "commit() @10k                  {:>10.1} us",
            (t_full - t_base).max(0.0) / 100.0 * 1e6
        );
    }

    // -- wire codec ---------------------------------------------------------
    {
        let src = with_decls(&format!(
            r#"fn main() -> nil {{
{}
    let bytes = fork_to_bytes(fork())
    print(len(bytes))
}}"#,
            setup
        ));
        let (_, vm) = timed_run(&src);
        let size: f64 = vm.print_buffer[0].parse::<f64>().unwrap();
        println!("fork_to_bytes payload @10k     {:>10.1} KB", size / 1024.0);

        let setup_enc = format!("{}\nlet f = fork()", setup);
        let full_enc = format!(
            "{}\nfor r in range(0, 10) {{ let _b = fork_to_bytes(f) }}",
            setup_enc
        );
        let (t_base, _) = timed_run(&with_decls(&format!(
            "fn main() -> nil {{\n{}\n}}",
            setup_enc
        )));
        let (t_full, _) = timed_run(&with_decls(&format!(
            "fn main() -> nil {{\n{}\n}}",
            full_enc
        )));
        println!(
            "fork_to_bytes @10k             {:>10.1} ms",
            (t_full - t_base).max(0.0) / 10.0 * 1e3
        );

        let setup_dec = format!("{}\nlet b = fork_to_bytes(fork())", setup);
        let full_dec = format!(
            "{}\nfor r in range(0, 10) {{ let _f = fork_from_bytes(b) }}",
            setup_dec
        );
        let (t_base, _) = timed_run(&with_decls(&format!(
            "fn main() -> nil {{\n{}\n}}",
            setup_dec
        )));
        let (t_full, _) = timed_run(&with_decls(&format!(
            "fn main() -> nil {{\n{}\n}}",
            full_dec
        )));
        println!(
            "fork_from_bytes @10k           {:>10.1} ms",
            (t_full - t_base).max(0.0) / 10.0 * 1e3
        );
    }

    // -- delta sync ----------------------------------------------------------
    // 200 entities touched out of 10k: the payload and the cost must track
    // the divergence (200), not the world (10k).
    {
        let diverge = r#"
    let base = fork()
    for e in slice(entities(Pos), 0, 200) { update(e, Hp) { hp = 1 } }
    let f = fork()
"#;
        let src = with_decls(&format!(
            "fn main() -> nil {{\n{}\n{}\nprint(len(fork_delta(base, f)))\n}}",
            setup, diverge
        ));
        let (_, vm) = timed_run(&src);
        let size: f64 = vm.print_buffer[0].parse::<f64>().unwrap();
        println!("fork_delta payload @10k/200    {:>10.1} KB", size / 1024.0);

        let setup_enc = format!("{}\n{}", setup, diverge);
        let full_enc = format!(
            "{}\nfor r in range(0, 10) {{ let _d = fork_delta(base, f) }}",
            setup_enc
        );
        let (t_base, _) = timed_run(&with_decls(&format!(
            "fn main() -> nil {{\n{}\n}}",
            setup_enc
        )));
        let (t_full, _) = timed_run(&with_decls(&format!(
            "fn main() -> nil {{\n{}\n}}",
            full_enc
        )));
        println!(
            "fork_delta @10k/200            {:>10.1} ms",
            (t_full - t_base).max(0.0) / 10.0 * 1e3
        );

        let setup_app = format!("{}\nlet d = fork_delta(base, f)", setup_enc);
        let full_app = format!(
            "{}\nfor r in range(0, 10) {{ let _g = fork_apply(base, d) }}",
            setup_app
        );
        let (t_base, _) = timed_run(&with_decls(&format!(
            "fn main() -> nil {{\n{}\n}}",
            setup_app
        )));
        let (t_full, _) = timed_run(&with_decls(&format!(
            "fn main() -> nil {{\n{}\n}}",
            full_app
        )));
        println!(
            "fork_apply @10k/200            {:>10.1} ms",
            (t_full - t_base).max(0.0) / 10.0 * 1e3
        );
    }

    // -- three-way merge -----------------------------------------------------
    {
        let divergence = r#"
    let base = fork()
    for e in slice(entities(Pos), 0, 100) { update(e, Hp) { hp = 1 } }
    let ours = fork()
    commit(base)
    for e in slice(entities(Pos), 200, 300) { update(e, Hp) { hp = 2 } }
    for i in range(0, 50) { let _s = spawn(Pos { x: 9.9, y: 9.9 }) }
    let theirs = fork()
    commit(base)
"#;
        let setup_src = with_decls(&format!(
            "fn main() -> nil {{\n{}\n{}\n}}",
            setup, divergence
        ));
        let full_src = with_decls(&format!(
            "fn main() -> nil {{\n{}\n{}\nfor r in range(0, 10) {{ let _m = merge_forks(base, ours, theirs) }}\n}}",
            setup, divergence
        ));
        let (t_base, _) = timed_run(&setup_src);
        let (t_full, _) = timed_run(&full_src);
        println!(
            "merge_forks @10k (200 edits,   {:>10.1} ms",
            (t_full - t_base).max(0.0) / 10.0 * 1e3
        );
        println!("  50 spawns, 100 vs 100 rows)");
    }

    // -- diff ----------------------------------------------------------------
    {
        let div = r#"
    let base = fork()
    for e in slice(entities(Pos), 0, 100) { update(e, Hp) { hp = 1 } }
    let after = fork()
"#;
        let setup_src = with_decls(&format!("fn main() -> nil {{\n{}\n{}\n}}", setup, div));
        let full_src = with_decls(&format!(
            "fn main() -> nil {{\n{}\n{}\nfor r in range(0, 100) {{ let _d = diff(base, after) }}\n}}",
            setup, div
        ));
        let (t_base, _) = timed_run(&setup_src);
        let (t_full, _) = timed_run(&full_src);
        println!(
            "diff() @10k (shared lineage)   {:>10.1} us",
            (t_full - t_base).max(0.0) / 100.0 * 1e6
        );
    }

    // -- save / load ----------------------------------------------------------
    {
        let setup_src = with_decls(&format!("fn main() -> nil {{\n{}\n}}", setup));
        let full_src = with_decls(&format!(
            "fn main() -> nil {{\n{}\nfor r in range(0, 10) {{ let _s = save_world() }}\n}}",
            setup
        ));
        let (t_base, _) = timed_run(&setup_src);
        let (t_full, _) = timed_run(&full_src);
        println!(
            "save_world() @10k              {:>10.1} ms",
            (t_full - t_base).max(0.0) / 10.0 * 1e3
        );
    }

    // -- events ----------------------------------------------------------------
    {
        let setup_src = with_decls("fn main() -> nil {\nlet _x = 0\n}");
        let full_src = with_decls(
            "fn main() -> nil {\nfor i in range(0, 10000) { emit Ping { amount: 1 } }\nflush_events()\n}",
        );
        let (t_base, _) = timed_run(&setup_src);
        let (t_full, _) = timed_run(&full_src);
        println!(
            "emit+flush handler             {:>10.2} us/event",
            (t_full - t_base).max(0.0) / 10_000.0 * 1e6
        );
    }

    // -- causality: why() and ledger memory -------------------------------------
    {
        let setup_w = format!("{}\nlet target = entities(Pos)[0]", setup);
        let full_w = format!(
            "{}\nfor r in range(0, 1000) {{ let _w = why(target, Hp) }}",
            setup_w
        );
        let (t_base, _) = timed_run(&with_decls(&format!(
            "fn main() -> nil {{\n{}\n}}",
            setup_w
        )));
        let (t_full, _) = timed_run(&with_decls(&format!("fn main() -> nil {{\n{}\n}}", full_w)));
        println!(
            "why() over 20k-write ledger    {:>10.1} us",
            (t_full - t_base).max(0.0) / 1000.0 * 1e6
        );

        // Ledger memory: measured record sizes plus retention behavior.
        use crate::causality::{CausalityLedger, Cause, WriteKind};
        let mut ledger = CausalityLedger::default();
        for i in 0..150_000u32 {
            ledger.record_write(
                0,
                Some(i),
                Some(format!("entity_{}", i)),
                "Hp",
                format!("{{ hp: {}, max: 100 }}", i),
                WriteKind::Set,
                Cause::Main,
            );
        }
        let per_record = std::mem::size_of::<crate::causality::WriteRecord>();
        let strings: usize = ledger
            .writes
            .iter()
            .map(|w| {
                w.value.len() + w.component.len() + w.entity_name.as_ref().map_or(0, |s| s.len())
            })
            .sum();
        println!(
            "ledger after 150k writes       {:>10} records retained (cap {})",
            ledger.writes.len(),
            crate::causality::DEFAULT_RETENTION_CAP
        );
        println!(
            "ledger memory at cap           {:>10.1} MB ({} B/record + strings)",
            (ledger.writes.len() * per_record + strings) as f64 / (1024.0 * 1024.0),
            per_record
        );
        assert_eq!(ledger.writes.len(), crate::causality::DEFAULT_RETENTION_CAP);
    }

    println!("== end ==");
}

/// The scale receipt: every "O(divergence)" claim measured at 1,000,000
/// entities with the *same divergence* as the 10k bench (200 edits, 50
/// spawns). If the claims are honest, those numbers stay put while the
/// O(world) operations grow ~100x. Setup is paid once and operations are
/// timed in-program with clock(), because spawning 1M entities twice per
/// measurement would be most of the wall time.
///
/// Run with:
///   cargo test -p rad-vm --release bench_at_scale_1m -- --ignored --nocapture
#[test]
#[ignore = "benchmark: run explicitly with --release --ignored --nocapture"]
fn bench_at_scale_1m() {
    let src = r#"
component Pos { x: 0.0, y: 0.0 }
component Hp { hp: 100, max: 100 }

fn main() -> nil {
    let n = 1000000
    let mut t0 = clock()
    for i in range(0, n) {
        let e = spawn(Pos { x: float(i), y: 0.5 })
        set(e, Hp { hp: i })
    }
    print(f"spawn+set 1M entities          {(clock() - t0) * 1000.0} ms")

    t0 = clock()
    for r in range(0, 100) { let _f = fork() }
    print(f"fork() @1M                     {(clock() - t0) / 100.0 * 1000000.0} us")

    let f = fork()
    t0 = clock()
    for r in range(0, 100) { commit(f) }
    print(f"commit() @1M                   {(clock() - t0) / 100.0 * 1000000.0} us")

    // The same divergence as the 10k bench: 100 + 100 edits, 50 spawns.
    let es = slice(entities(Pos), 0, 300)
    let base = fork()
    for e in slice(es, 0, 100) { update(e, Hp) { hp = 1 } }
    let ours = fork()
    commit(base)
    for e in slice(es, 200, 300) { update(e, Hp) { hp = 2 } }
    for i in range(0, 50) { let _s = spawn(Pos { x: 9.9, y: 9.9 }) }
    let theirs = fork()
    commit(base)

    t0 = clock()
    for r in range(0, 10) { let _m = merge_forks(base, ours, theirs) }
    print(f"merge_forks @1M (200 edits)    {(clock() - t0) / 10.0 * 1000.0} ms")

    t0 = clock()
    for r in range(0, 100) { let _d = diff(base, ours) }
    print(f"diff() @1M (shared lineage)    {(clock() - t0) / 100.0 * 1000000.0} us")

    t0 = clock()
    for r in range(0, 10) { let _d = fork_delta(base, theirs) }
    print(f"fork_delta @1M/150             {(clock() - t0) / 10.0 * 1000.0} ms")
    let d = fork_delta(base, theirs)
    print(f"fork_delta payload @1M/150     {len(d) / 1024} KB")

    t0 = clock()
    for r in range(0, 10) { let _g = fork_apply(base, d) }
    print(f"fork_apply @1M/150             {(clock() - t0) / 10.0 * 1000.0} ms")

    // O(world) operations, measured honestly: these are *supposed* to grow
    // ~100x from the 10k numbers. Few reps — each one is seconds.
    t0 = clock()
    let s = save_world()
    print(f"save_world() @1M               {(clock() - t0) * 1000.0} ms, {len(s) / 1048576} MB")

    t0 = clock()
    let b = fork_to_bytes(fork())
    print(f"fork_to_bytes @1M              {(clock() - t0) * 1000.0} ms, {len(b) / 1048576} MB")

    t0 = clock()
    let _in = fork_from_bytes(b)
    print(f"fork_from_bytes @1M            {(clock() - t0) * 1000.0} ms")
}
"#;
    let result = compile(src);
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(7);
    vm.load_compile_result(result);
    vm.run(0).expect("run");
    println!();
    println!("== rad benchmarks at scale (N = 1,000,000 entities, 2 components each) ==");
    for line in &vm.print_buffer {
        println!("{}", line);
    }
    println!("== end ==");
}
