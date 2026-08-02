//! Executes compiled `test` declarations (spec: `test_decl`).
//!
//! `compile_test_decl` lowers every `test NAME { … }` block to an arity-0
//! function bound to the global `__test_NAME`; running chunk 0 defines
//! those globals but nothing ever called them, so a suite full of failing
//! assertions reported green (A4 BUG 08, seq 73). This module is the
//! missing half: enumerate the `__test_` globals in declaration order
//! (global slots are minted in compile order) and invoke each one,
//! catching per-test runtime errors so one failing test cannot stop the
//! rest of the suite.

use crate::vm::VM;

/// Result of one executed `test` block.
#[derive(Debug)]
pub struct TestOutcome {
    /// The test's declared name (the `__test_` global prefix stripped).
    pub name: String,
    /// `None` when the test passed; the runtime error message otherwise.
    pub error: Option<String>,
}

/// Run every `test` declaration the loaded program defined, in source
/// order, against the world its top-level code left behind (the fixture).
/// Tests run sequentially and share that world. VM control state (value
/// stack, call frames) is restored between tests, so an assertion that
/// fails deep inside a call chain cannot poison the tests after it.
pub fn run_tests(vm: &mut VM) -> Vec<TestOutcome> {
    let tests: Vec<(usize, String)> = vm
        .global_names
        .iter()
        .enumerate()
        .filter_map(|(slot, n)| n.strip_prefix("__test_").map(|t| (slot, t.to_string())))
        .collect();

    let mut outcomes = Vec::with_capacity(tests.len());
    for (slot, name) in tests {
        let callee = vm.globals[slot];
        if callee.as_fn().is_none_or(|f| f.arity != 0) {
            // Not a compiled test block (e.g. a user global that happens
            // to start with the reserved prefix) — nothing to run.
            continue;
        }
        let frames_before = vm.frames.len();
        let stack_before = vm.stack.len();
        let error = vm.call_value(&callee, Vec::new()).err();
        // On success call_value already popped its frame and return value;
        // on failure the aborted call's frames and operands are still
        // there. Restore both so the next test starts clean.
        vm.frames.truncate(frames_before);
        vm.stack.truncate(stack_before);
        outcomes.push(TestOutcome { name, error });
    }
    outcomes
}
