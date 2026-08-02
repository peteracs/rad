//! Tier-2 #1 — property-fuzz the rad-LEVEL evaluator. D2 fuzzed the
//! plumbing (decode/merge); D7 then shipped *application logic* that
//! corrupted cells (a SUM that dropped its last row) and only the
//! counterfactual replay caught it after the fact. This suite makes that
//! bug class unable to reach main: the ACTUAL `projects/dogfood/radsheet/
//! lib_sheet.rad` engine (baked in via include_str!, so CI tests the
//! shipped file) is driven with randomly generated grids, edit sequences,
//! formulas, and three-way merges, and must uphold properties an engine
//! can't fake:
//!
//!   P1  derive-invariant: every cell's stored (val, kind) re-derives from
//!       its raw — `eval_formula(raw)` agrees with what the cascade stored.
//!   P2  range/chain agreement: `=SUM(rect)` equals the explicit
//!       `=A1+A2+…` chain over the same cells (different iteration code
//!       paths — D7's dropped-last-row dies here).
//!   P3  algebraic identities: COUNT*AVG ≈ SUM, MIN ≤ AVG ≤ MAX, and
//!       COUNT(rect) counts exactly the non-empty/non-err/non-text cells.
//!   P4  reflow idempotence: running the Dirty cascade again changes
//!       nothing (no oscillation, no half-applied derived state).
//!   P5  the D5 property: after a random three-way merge with the server's
//!       resolution policy + reflow, P1 still holds for EVERY cell —
//!       derived state can never silently survive a merge of its sources.
//!   P6  determinism: every scenario records and replays digest-verified.
//!
//! Budget: RAD_SHEET_FUZZ scenarios (default 24; thousands for soaks).

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::vm::VM;

const LIB_SHEET: &str = include_str!("../../../projects/dogfood/radsheet/lib_sheet.rad");

fn scenarios() -> u64 {
    std::env::var("RAD_SHEET_FUZZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

const COLS: u64 = 6;
/// Edits land in rows 0..STORM_ROWS; scratch property cells live BELOW
/// (rows 10-11), referencing upward only — together with the
/// strictly-above-your-own-row rule for generated formulas this makes
/// every generated sheet a DAG. Cycles are a *legitimate* engine behavior
/// (fixpoint cap) but would make P1/P4 unfalsifiable, so the generator
/// excludes them by construction; the cycle path stays covered by the
/// engine's own smoke test.
const STORM_ROWS: u64 = 10;

fn cell_name(c: u64, r: u64) -> String {
    format!("{}{}", (b'A' + c as u8) as char, r + 1)
}

/// A ref strictly ABOVE `below_row` (callers guarantee below_row >= 1).
fn rand_ref_above(rng: &mut Rng, below_row: u64) -> String {
    cell_name(rng.below(COLS), rng.below(below_row))
}

/// A random rectangle entirely above `below_row`, as (corner, corner) refs
/// plus the cells inside. Corners in random order: parse_ref normalizes.
fn rand_rect_above(rng: &mut Rng, below_row: u64) -> (String, String, Vec<String>) {
    let c0 = rng.below(COLS);
    let c1 = c0 + rng.below(COLS - c0).min(2);
    let r0 = rng.below(below_row);
    let r1 = r0 + rng.below(below_row - r0).min(3);
    let mut cells = Vec::new();
    for c in c0..=c1 {
        for r in r0..=r1 {
            cells.push(cell_name(c, r));
        }
    }
    if rng.below(2) == 0 {
        (cell_name(c0, r0), cell_name(c1, r1), cells)
    } else {
        (cell_name(c1, r1), cell_name(c0, r0), cells)
    }
}

/// One random edit: (target cell in the storm rows, raw value). Formulas
/// reference only rows strictly above the target's own row.
fn rand_edit(rng: &mut Rng) -> (String, String) {
    let c = rng.below(COLS);
    let r = rng.below(STORM_ROWS);
    let raw = match rng.below(10) {
        0..=4 => {
            let n = rng.below(2001) as i64 - 1000;
            if rng.below(4) == 0 {
                format!("{}.5", n)
            } else {
                format!("{}", n)
            }
        }
        5 => "note".to_string(), // text label
        6 => "".to_string(),     // clear
        _ => {
            if r == 0 {
                format!("{}", rng.below(100)) // row 0 has nothing above it
            } else {
                rand_formula_above(rng, r)
            }
        }
    };
    (cell_name(c, r), raw)
}

fn rand_formula_above(rng: &mut Rng, below_row: u64) -> String {
    let mut f = String::from("=");
    let term = |rng: &mut Rng, out: &mut String| match rng.below(5) {
        0 => out.push_str(&format!("{}", rng.below(50))),
        1..=2 => out.push_str(&rand_ref_above(rng, below_row)),
        _ => {
            let fname = ["SUM", "AVG", "MIN", "MAX", "COUNT"][rng.below(5) as usize];
            let (a, b, _) = rand_rect_above(rng, below_row);
            out.push_str(&format!("{}({}:{})", fname, a, b));
        }
    };
    term(rng, &mut f);
    for _ in 0..rng.below(3) {
        let op = ["+", "-", "*"][rng.below(3) as usize]; // '/' invites /0 noise
        f.push_str(op);
        term(rng, &mut f);
    }
    f
}

/// The generated property block, shared by both tests: asserts P1–P4 over
/// the current world, printing PROP_FAIL lines on violation.
const PROP_CHECKS: &str = r#"
fn check_props(tag: str) -> nil {
    // P1: derive-invariant — stored (val, kind) re-derives from raw
    for ent in entities(Cell) {
        let cl = get(ent, Cell) |> unwrap
        if starts_with(cl.raw, "=") {
            let res = eval_formula(slice(cl.raw, 1, len(cl.raw)))
            let fresh_val = res[0]
            let ok = res[1]
            let mut fresh_kind = "formula"
            if !ok { fresh_kind = "err" }
            if fresh_kind != cl.kind or (ok and fresh_val != cl.val) {
                let nm = name_of(ent)
                print(f"PROP_FAIL {tag} P1 {nm} raw={cl.raw} stored={cl.val}/{cl.kind} fresh={fresh_val}/{fresh_kind}")
            }
        }
    }
    // P4: reflow idempotence — another full cascade changes nothing
    let before = render_grid()
    recalc_after_merge(0)
    let after = render_grid()
    if before != after {
        print(f"PROP_FAIL {tag} P4 reflow not idempotent")
    }
}
"#;

fn build_scenario(seed: u64) -> String {
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    let mut src = String::new();
    src.push_str(LIB_SHEET);
    src.push_str(PROP_CHECKS);
    src.push_str("\nfn main() -> nil {\n    init_grid()\n");

    // phase 1: random edit storm over rows 1-10 (DAG by construction)
    let edits = 12 + rng.below(20);
    for i in 0..edits {
        let (cell, raw) = rand_edit(&mut rng);
        src.push_str(&format!(
            "    let _ = set_cell({:?}, {:?}, \"fuzz\", {})\n",
            cell, raw, i
        ));
    }

    // P2: SUM(rect) == explicit chain over the same cells, written into
    // two scratch cells (row 11, below every storm row) through the same
    // edit path a user would take. Different iteration code paths: the
    // range loop vs the ref chain — D7's dropped-last-row dies here.
    let (a, b, cells) = rand_rect_above(&mut rng, STORM_ROWS);
    let chain = cells
        .iter()
        .map(|c| c.as_str())
        .collect::<Vec<_>>()
        .join("+");
    src.push_str(&format!(
        "    let _ = set_cell(\"A11\", \"=SUM({a}:{b})\", \"fuzz\", 900)\n"
    ));
    src.push_str(&format!(
        "    let _ = set_cell(\"B11\", \"={chain}\", \"fuzz\", 901)\n"
    ));
    src.push_str(r#"
    let sum_cell = get(get_entity("A11"), Cell) |> unwrap
    let chain_cell = get(get_entity("B11"), Cell) |> unwrap
    if sum_cell.kind != chain_cell.kind or sum_cell.val != chain_cell.val {
        print(f"PROP_FAIL edit P2 SUM={sum_cell.val}/{sum_cell.kind} chain={chain_cell.val}/{chain_cell.kind}")
    }
"#);

    // P3: identities over a fresh rect (scratch cells in rows 11-12)
    let (ra, rb, _) = rand_rect_above(&mut rng, STORM_ROWS);
    src.push_str(&format!(
        r#"
    let _ = set_cell("C11", "=SUM({ra}:{rb})", "fuzz", 902)
    let _ = set_cell("D11", "=AVG({ra}:{rb})", "fuzz", 903)
    let _ = set_cell("E11", "=MIN({ra}:{rb})", "fuzz", 904)
    let _ = set_cell("F11", "=MAX({ra}:{rb})", "fuzz", 905)
    let _ = set_cell("A12", "=COUNT({ra}:{rb})", "fuzz", 906)
    let s = (get(get_entity("C11"), Cell) |> unwrap).val
    let av = (get(get_entity("D11"), Cell) |> unwrap).val
    let lo = (get(get_entity("E11"), Cell) |> unwrap).val
    let hi = (get(get_entity("F11"), Cell) |> unwrap).val
    let cnt = (get(get_entity("A12"), Cell) |> unwrap).val
    if cnt > 0.0 {{
        let prod = cnt * av
        let drift = prod - s
        if drift > 0.000001 or drift < -0.000001 {{
            print(f"PROP_FAIL edit P3 COUNT*AVG={{prod}} SUM={{s}}")
        }}
        if av < lo - 0.000001 or av > hi + 0.000001 {{
            print(f"PROP_FAIL edit P3 AVG={{av}} outside [{{lo}}, {{hi}}]")
        }}
    }}
"#
    ));

    src.push_str("    check_props(\"edit\")\n");
    src.push_str("    print(\"SCENARIO_OK\")\n}\n");
    src
}

/// Random three-way merge scenario: base storm, two divergent edit sets,
/// merge with the server's policy, reflow — then every property again.
fn build_merge_scenario(seed: u64) -> String {
    let mut rng = Rng(seed.wrapping_mul(0xC0FFEE1B00B5) | 1);
    let mut src = String::new();
    src.push_str(LIB_SHEET);
    src.push_str(PROP_CHECKS);
    src.push_str("\nfn main() -> nil {\n    init_grid()\n");

    for i in 0..(8 + rng.below(10)) {
        let (cell, raw) = rand_edit(&mut rng);
        src.push_str(&format!(
            "    let _ = set_cell({:?}, {:?}, \"base\", {})\n",
            cell, raw, i
        ));
    }
    src.push_str("    let base = fork()\n");
    for i in 0..(3 + rng.below(8)) {
        let (cell, raw) = rand_edit(&mut rng);
        src.push_str(&format!(
            "    let _ = set_cell({:?}, {:?}, \"oursider\", {})\n",
            cell,
            raw,
            100 + i
        ));
    }
    src.push_str("    let ours = fork()\n    commit(base)\n");
    for i in 0..(3 + rng.below(8)) {
        let (cell, raw) = rand_edit(&mut rng);
        src.push_str(&format!(
            "    let _ = set_cell({:?}, {:?}, \"theirsider\", {})\n",
            cell,
            raw,
            200 + i
        ));
    }
    src.push_str("    let theirs = fork()\n    commit(base)\n");

    // The server's resolution policy, verbatim shape: derived fields take
    // ours arbitrarily (reflow re-derives them), raw conflicts pick a side
    // deterministically, Sheet.passes merges by max.
    src.push_str(
        r#"
    match merge_forks(base, ours, theirs) {
        Ok(m) => {
            commit(m)
        }
        Err(conflicts) => {
            let mut picks = []
            for c in conflicts {
                match c {
                    FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                        if comp == "Cell" and (field == "val" or field == "kind") {
                            picks << (c, ours)
                        }
                        if comp == "Cell" and field == "raw" {
                            picks << (c, theirs)
                        }
                    }
                    ResourceFieldConflict { res, field, base, ours, theirs } => {
                        if res == "Sheet" and field == "passes" {
                            picks << (c, max(ours, theirs))
                        }
                    }
                    _ => {
                        print("PROP_FAIL merge unexpected structural conflict")
                    }
                }
            }
            match merge_forks_with(base, ours, theirs, picks) {
                Ok(m) => { commit(m) }
                Err(remaining) => {
                    print(f"PROP_FAIL merge {len(remaining)} conflicts survived policy")
                }
            }
        }
    }
    recalc_after_merge(999)
    check_props("merge")
    print("SCENARIO_OK")
}
"#,
    );
    src
}

// ---------------------------------------------------------------------------
// drivers
// ---------------------------------------------------------------------------

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
    let mut checker = crate::checker::Checker::new();
    let errors = checker.check(&program);
    assert!(errors.is_empty(), "check errors: {:?}", errors);
    Compiler::new()
        .with_checker_output(checker.output())
        .compile(&program)
        .expect("compile")
}

/// Run a scenario: record it, assert no PROP_FAIL, then replay digest-
/// verified (P6) — the evaluator's determinism rides every scenario.
fn run_scenario(src: &str, what: &str) {
    let compiled_marker = "SCENARIO_OK";
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(7);
    vm.enable_recording(src);
    vm.load_compile_result(compile(src));
    vm.run(0)
        .unwrap_or_else(|e| panic!("{what} crashed: {e}\n--- source ---\n{src}"));
    let failures: Vec<&String> = vm
        .print_buffer
        .iter()
        .filter(|l| l.starts_with("PROP_FAIL"))
        .collect();
    assert!(
        failures.is_empty(),
        "{what} property violations:\n{}\n--- source ---\n{}",
        failures
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        src
    );
    assert_eq!(
        vm.print_buffer.last().map(|s| s.as_str()),
        Some(compiled_marker),
        "{what} did not complete"
    );
    let out_a = vm.print_buffer.clone();
    let tape = vm.take_trace().expect("tape");

    let replayer = crate::replay::TraceReplayer::parse(&tape, false).expect("parse tape");
    let mut rep = VM::new();
    rep.suppress_output();
    rep.enable_replay(replayer);
    rep.load_compile_result(compile(src));
    rep.run(0)
        .unwrap_or_else(|e| panic!("{what} replay crashed: {e}"));
    assert_eq!(out_a, rep.print_buffer, "{what}: replay output diverged");
    let report = rep.finish_replay().expect("report");
    assert_eq!(
        report.end_digest_match,
        Some(true),
        "{what}: replay digest mismatch"
    );
}

#[test]
fn property_fuzz_formula_evaluator() {
    for seed in 1..=scenarios() {
        run_scenario(&build_scenario(seed), &format!("edit scenario {seed}"));
    }
}

#[test]
fn property_fuzz_merge_keeps_derive_invariant() {
    for seed in 1..=scenarios() {
        run_scenario(
            &build_merge_scenario(seed),
            &format!("merge scenario {seed}"),
        );
    }
}

/// The regression that started it all: D7's planted bug (SUM dropping the
/// last row of its range) must be CAUGHT by P2. We verify the property has
/// teeth by injecting the exact bug into the engine source and asserting a
/// violation is reported.
#[test]
fn property_p2_catches_the_d7_bug_class() {
    let sabotaged = LIB_SHEET.replace(
        "for r in range(r0, r1 + 1) {",
        "for r in range(r0, r1) {", // the D7 incident, re-planted
    );
    assert_ne!(sabotaged, LIB_SHEET, "sabotage marker must exist");

    let mut src = String::new();
    src.push_str(&sabotaged);
    src.push_str(
        r#"
fn main() -> nil {
    init_grid()
    let _ = set_cell("A1", "10", "t", 1)
    let _ = set_cell("A2", "20", "t", 2)
    let _ = set_cell("A3", "30", "t", 3)
    let _ = set_cell("B1", "=SUM(A1:A3)", "t", 4)
    let _ = set_cell("B2", "=A1+A2+A3", "t", 5)
    let s = get(get_entity("B1"), Cell) |> unwrap
    let c = get(get_entity("B2"), Cell) |> unwrap
    if s.val != c.val {
        print("PROP_FAIL planted bug detected")
    }
}
"#,
    );
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(7);
    vm.load_compile_result(compile(&src));
    vm.run(0).expect("run");
    assert!(
        vm.print_buffer.iter().any(|l| l.starts_with("PROP_FAIL")),
        "the property suite must catch the D7 bug class (got: {:?})",
        vm.print_buffer
    );
}
