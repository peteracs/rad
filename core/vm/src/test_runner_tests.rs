//! Regression tests for A4 BUG 08/08b (seq 73) and A4 BUG 04 (seq 51).
//!
//! BUG 08: `test` blocks were compiled to `__test_*` globals that nothing
//! ever invoked, so `rad test` reported PASS for a suite asserting `1 == 2`.
//! BUG 08b: the documented property form `test X for v in gen_int() { … }`
//! did not parse (`for` lexes as a keyword, the parser matched an Ident).
//! BUG 04: `save_world()` wrote f64::MAX as a 309-digit decimal expansion
//! that `load_world()`/`fork_from_bytes()` rejected ("number out of range").

use crate::parser::ParserOptions;
use crate::test_runner::run_tests;
use crate::vm::VM;

fn run_vm(src: &str) -> VM {
    let result =
        crate::test_support::compile_source(src, ParserOptions).expect("parse and compile");
    let mut vm = VM::new();
    vm.suppress_output();
    vm.load_compile_result(result);
    vm.run(0).expect("run");
    vm
}

/// The headline of BUG 08: a failing `test` block must produce a failed
/// outcome — and it must not stop the tests declared after it.
#[test]
fn failing_test_blocks_fail_and_later_tests_still_run() {
    let mut vm = run_vm(
        r#"
        test this_test_must_fail { assert_eq(1, 2) }
        test this_test_must_also_fail { assert(false, "deliberate failure") }
        test this_one_would_pass { assert_eq(1, 1) }
        "#,
    );
    let outcomes = run_tests(&mut vm);
    let names: Vec<&str> = outcomes.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "this_test_must_fail",
            "this_test_must_also_fail",
            "this_one_would_pass"
        ],
        "tests run in declaration order"
    );
    let e0 = outcomes[0].error.as_deref().expect("assert_eq(1,2) fails");
    assert!(e0.contains("assert_eq failed"), "unexpected error: {}", e0);
    let e1 = outcomes[1].error.as_deref().expect("assert(false) fails");
    assert!(
        e1.contains("deliberate failure"),
        "unexpected error: {}",
        e1
    );
    assert!(
        outcomes[2].error.is_none(),
        "a passing test after two failures must still pass: {:?}",
        outcomes[2].error
    );
}

/// Tests run against the world the file's top-level code set up.
#[test]
fn test_blocks_see_top_level_fixtures() {
    let mut vm = run_vm(
        r#"
        component Health { hp: 100 }
        spawn("hero", Health { hp: 73 })
        test hero_fixture_visible {
            let h = get(get_entity("hero"), Health) |> unwrap
            assert_eq(h.hp, 73)
        }
        "#,
    );
    let outcomes = run_tests(&mut vm);
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

/// BUG 08b: the documented property form must parse, and the body must run
/// once per generated value (not once with the whole list bound).
#[test]
fn property_form_parses_and_runs_per_value() {
    let mut vm = run_vm(
        r#"
        test reflexive for v in gen_int() {
            assert_eq(v, v)
        }
        test one_print_per_value for v in [10, 20, 30] {
            print(v)
        }
        "#,
    );
    let outcomes = run_tests(&mut vm);
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
    assert!(outcomes[1].error.is_none(), "{:?}", outcomes[1].error);
    assert_eq!(
        vm.print_buffer,
        vec!["10", "20", "30"],
        "body must execute once per element"
    );
}

/// Multiple generators nest (cartesian product, first generator outermost),
/// and a sample that violates the property fails the test.
#[test]
fn property_generators_nest_and_failures_surface() {
    let mut vm = run_vm(
        r#"
        test cartesian for a in [1, 2], b in [10, 20] {
            print(a + b)
        }
        test must_fail for v in [1, 2, 3] {
            assert(v < 3, "hit the failing sample")
        }
        "#,
    );
    let outcomes = run_tests(&mut vm);
    assert_eq!(vm.print_buffer, vec!["11", "21", "12", "22"]);
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
    let e = outcomes[1]
        .error
        .as_deref()
        .expect("v = 3 violates the property");
    assert!(
        e.contains("hit the failing sample"),
        "unexpected error: {}",
        e
    );
}

/// A4 BUG 04 (seq 51): a world holding f64::MAX must survive both
/// persistence paths — save_world()/load_world() and
/// fork_to_bytes()/fork_from_bytes() — value- and type-identical.
#[test]
fn f64_max_roundtrips_through_save_load_and_fork_bytes() {
    let vm = run_vm(
        r#"
        component Big { x: 0.0 }
        spawn("e", Big { x: 1.7976931348623157e308 })
        let bytes = fork_to_bytes(fork())
        match fork_from_bytes(bytes) {
            Ok(_f) => { print("wire-ok") }
            Err(m) => { print(f"wire-broken: {m}") }
        }
        let saved = save_world()
        let _n = load_world(saved)
        let back = get(get_entity("e"), Big) |> unwrap
        print(back.x == 1.7976931348623157e308)
        print(typeof(back.x))
        "#,
    );
    assert_eq!(vm.print_buffer, vec!["wire-ok", "true", "float"]);
}

/// The fix must not make integral floats decay to int: 3.0 keeps its float
/// marker and value through save/load.
#[test]
fn integral_float_stays_float_through_save_load() {
    let vm = run_vm(
        r#"
        component P { y: 0.0 }
        spawn("p", P { y: 3.0 })
        let saved = save_world()
        let _n = load_world(saved)
        let back = get(get_entity("p"), P) |> unwrap
        print(typeof(back.y))
        print(back.y == 3.0)
        "#,
    );
    assert_eq!(vm.print_buffer, vec!["float", "true"]);
}

/// Each `test` starts from the fixture, not from whatever the previous test
/// left behind. AccessLens' second workflow test seeded a directory of ten
/// users and asserted ten existed, but the first test had already seeded a
/// hundred into the same world — it failed for a reason that had nothing to do
/// with the code under test, and would have passed if run alone or first.
#[test]
fn each_test_starts_from_the_fixture_world() {
    let mut vm = run_vm(
        r#"
        component Marker { tag: int = 0 }

        test first_seeds_two {
            spawn(Marker { tag: 1 })
            spawn(Marker { tag: 2 })
            assert_eq(len(entities(Marker)), 2)
        }

        test second_sees_only_its_own {
            spawn(Marker { tag: 3 })
            assert_eq(len(entities(Marker)), 1)
        }
        "#,
    );
    let outcomes = run_tests(&mut vm);
    assert_eq!(outcomes.len(), 2);
    for outcome in &outcomes {
        assert!(
            outcome.error.is_none(),
            "{} leaked state between tests: {:?}",
            outcome.name,
            outcome.error
        );
    }
}

/// World state produced by the file's top-level code is the fixture every test
/// starts from, so isolation must not mean "empty".
#[test]
fn fixture_state_is_visible_to_every_test() {
    let mut vm = run_vm(
        r#"
        component Marker { tag: int = 0 }
        spawn(Marker { tag: 7 })

        test first_sees_the_fixture { assert_eq(len(entities(Marker)), 1) }
        test second_sees_the_same_fixture { assert_eq(len(entities(Marker)), 1) }
        "#,
    );
    let outcomes = run_tests(&mut vm);
    assert_eq!(outcomes.len(), 2);
    for outcome in &outcomes {
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
    }
}

/// `shared test` opts back in to the previous test's world, so a suite that
/// deliberately asserts across one lifecycle stays expressible — and says so.
#[test]
fn shared_tests_observe_the_previous_test_world() {
    let mut vm = run_vm(
        r#"
        component Marker { tag: int = 0 }

        test seeds_the_world { spawn(Marker { tag: 1 }) }
        shared test observes_the_previous_world { assert_eq(len(entities(Marker)), 1) }
        "#,
    );
    let outcomes = run_tests(&mut vm);
    assert_eq!(outcomes.len(), 2);
    for outcome in &outcomes {
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
    }
}

#[test]
fn isolation_restores_named_entity_indexes_after_a_shared_chain() {
    let mut vm = run_vm(
        r#"
        component Marker { tag: int = 0 }

        test producer { spawn("shared-probe", Marker { tag: 1 }) }
        shared test consumer {
            let probe = require_entity("shared-probe")
            set(probe, Marker { tag: 2 })
        }
        shared test chain {
            assert_eq(require(require_entity("shared-probe"), Marker).tag, 2)
        }
        test isolated_again {
            assert_eq(len(entities(Marker)), 0)
            assert(get_entity("shared-probe") == nil, "named fixture index leaked")
        }
        "#,
    );
    let outcomes = run_tests(&mut vm);
    for outcome in &outcomes {
        assert!(
            outcome.error.is_none(),
            "{} failed: {:?}",
            outcome.name,
            outcome.error
        );
    }
}
