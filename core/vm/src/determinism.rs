//! Phase 0 of record & replay: the determinism tripwire.
//!
//! Replay only works if the interpreter is deterministic given identical
//! inputs (RNG seed, io results). These tests run the same program in two
//! fresh VMs and demand byte-identical observable behavior: the full print
//! buffer and a blake3 content digest of the final world.
//!
//! Determinism is defended *by convention* in this codebase — every map/
//! hash-ordered iteration that leaks into program-visible behavior sorts
//! first (`keys`, `entries`, `values`, `GetIter`, `Display`, `value_to_json`,
//! `query`, `all_entity_ids`). A new unsorted iteration silently breaks
//! replay, so this module must stay green in CI forever.

#![cfg(test)]

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::value::Value;
use crate::vm::VM;
use std::collections::HashMap;

/// Compile `src` from scratch and run it in a fresh VM with the given RNG
/// seed. Returns the print buffer and the final world content digest.
fn run_fresh(src: &str, seed: u64) -> (Vec<String>, String) {
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
    vm.set_random_seed(seed);
    vm.load_compile_result(result);
    vm.run(0).expect("run");
    let digest = vm.world.content_digest();
    (vm.print_buffer.clone(), digest)
}

/// Exercises every audited nondeterminism surface: map literals and their
/// four iteration paths (print, keys, for-in, json), RNG flowing into both
/// prints and world state, events, systems over many entities, resources,
/// and fork/simulate/diff.
const GAUNTLET: &str = r#"
    component Pos { x: 0 }
    component Tag { label: "" }
    resource Score { total: 0 }
    event Bump { who }

    system Wander(p: mut Pos) {
        p = Pos { x: p.x + rand_int(1, 6) }
    }

    on Bump(e) {
        let p = get(e.who, Pos) |> unwrap
        set(e.who, Pos { x: p.x + 100 })
    }

    let hero = spawn("hero", Pos { x: 0 }, Tag { label: "hero" })
    for i in range(0, 15) {
        spawn(f"npc{i}", Pos { x: i }, Tag { label: f"npc{i}" })
    }

    let m = { "delta": 4, "alpha": 1, "charlie": 3, "bravo": 2 }
    print(m)
    print(keys(m))
    for k, v in m {
        print(f"{k}={v}")
    }
    print(json_stringify(m))

    print(rand_int(0, 1000000))
    emit Bump { who: hero }
    flush_events()
    set_resource(Score, Score { total: rand_int(0, 1000000) })

    let before = fork()
    let after = simulate(before, [system::Wander], 3)
    print(diff(before, after))
    let h = get(hero, Pos) |> unwrap
    print(h.x)
"#;

#[test]
fn twin_runs_with_same_seed_are_byte_identical() {
    let (out_a, digest_a) = run_fresh(GAUNTLET, 42);
    let (out_b, digest_b) = run_fresh(GAUNTLET, 42);
    assert_eq!(out_a, out_b, "print buffers diverged between twin runs");
    assert_eq!(
        digest_a, digest_b,
        "world content digests diverged between twin runs"
    );
}

#[test]
fn different_seeds_actually_diverge() {
    // Guards the twin test against vacuity: if the gauntlet stopped
    // exercising the RNG, identical twin runs would prove nothing.
    let (_, digest_a) = run_fresh(GAUNTLET, 1);
    let (_, digest_b) = run_fresh(GAUNTLET, 2);
    assert_ne!(
        digest_a, digest_b,
        "different seeds produced identical worlds — the gauntlet no longer exercises the RNG"
    );
}

#[test]
fn content_digest_is_hex_and_resource_sensitive() {
    let (_, digest) = run_fresh("let x = spawn(\"a\")", 7);
    assert_eq!(digest.len(), 64);
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));

    // Resources must be part of the digest (snapshot_json_like alone skips them).
    let (_, d1) = run_fresh("resource R { n: 0 }\nset_resource(R, R { n: 1 })", 7);
    let (_, d2) = run_fresh("resource R { n: 0 }\nset_resource(R, R { n: 2 })", 7);
    assert_ne!(d1, d2, "resource change did not affect content digest");
}

#[test]
fn sum_type_display_sorts_fields() {
    // st.fields is a hash-seeded HashMap; Display must sort or the printed
    // field order differs across processes (the one real leak the Phase 0
    // audit found, fixed in value.rs).
    let mut vm = VM::new();
    let mut fields_a: HashMap<String, Value> = HashMap::new();
    let mut fields_b: HashMap<String, Value> = HashMap::new();
    let names = ["zeta", "alpha", "mid", "beta"];
    for (i, n) in names.iter().enumerate() {
        fields_a.insert(n.to_string(), Value::from_int(&mut vm.gc, i as i64));
    }
    for (i, n) in names.iter().enumerate().rev() {
        fields_b.insert(n.to_string(), Value::from_int(&mut vm.gc, i as i64));
    }
    let a = Value::sum_type(&mut vm.gc, "Shape".into(), "Rect".into(), fields_a);
    let b = Value::sum_type(&mut vm.gc, "Shape".into(), "Rect".into(), fields_b);
    let (sa, sb) = (format!("{}", a), format!("{}", b));
    assert_eq!(sa, sb);
    assert!(
        sa.contains("alpha: 1, beta: 3, mid: 2, zeta: 0"),
        "fields not sorted: {}",
        sa
    );
}
