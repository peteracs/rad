//! The leak lab: a controlled, in-process environment for hunting memory
//! growth in the world-state machinery — no TCP, no PowerShell samplers, no
//! process working-set noise. Iterations are sub-second.
//!
//! Method: a counting global allocator with per-thread counters (exact
//! bytes, not OS pages, immune to concurrent tests) measures
//! the **slope** — net live bytes as a function of iteration count — for
//! each phase of the syncdesk server's push cycle in isolation. Constants
//! (compile, setup, interning) cancel out across the two runs; only
//! per-iteration growth survives. Each phase ends with `gc_collect()`, so a
//! nonzero live slope means memory the collector *cannot* reclaim, not
//! floating garbage. A second slope is taken after dropping the VM: bytes
//! that survive VM teardown are process-lifetime leaks (lost persistent
//! refcounts, leaked Arcs).
//!
//! Diagnose with:
//!   cargo test -p rad-vm --release leak_lab_report -- --ignored --nocapture --test-threads=1
//!
//! The regression test (`push_cycle_memory_is_flat`) runs in the normal
//! suite and pins the full server push path to a tight per-cycle budget.

#![cfg(test)]

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::vm::VM;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

// ---------------------------------------------------------------------------
// Counting allocator: exact net-live-bytes accounting, PER THREAD. The whole
// measurement (compile, VM run, VM drop) happens on the measuring test's
// thread and the phase bodies are single-threaded (no `schedule`, no rayon
// workers), so per-thread counters make the slope immune to every other test
// in the binary. The previous process-global counters made
// `push_cycle_memory_is_flat` flaky at full test parallelism: concurrent
// suites inflated dropped_per_iter past its budget, and the retry loop could
// not outlast a whole parallel storm. Cell<usize> is const-initialized and
// non-Drop, so access from inside the global allocator cannot itself
// allocate or recurse; try_with tolerates thread-teardown edges.
// ---------------------------------------------------------------------------

thread_local! {
    static TL_ALLOCATED: Cell<usize> = const { Cell::new(0) };
    static TL_FREED: Cell<usize> = const { Cell::new(0) };
}

struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            let _ = TL_ALLOCATED.try_with(|c| c.set(c.get() + layout.size()));
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = TL_FREED.try_with(|c| c.set(c.get() + layout.size()));
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            let _ = TL_ALLOCATED.try_with(|c| c.set(c.get() + new_size));
            let _ = TL_FREED.try_with(|c| c.set(c.get() + layout.size()));
        }
        p
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// Net live bytes allocated by THIS thread.
fn net_bytes() -> i64 {
    let allocated = TL_ALLOCATED.try_with(Cell::get).unwrap_or(0) as i64;
    let freed = TL_FREED.try_with(Cell::get).unwrap_or(0) as i64;
    allocated - freed
}

/// With per-thread counters other tests can no longer pollute a measurement;
/// this lock remains so lab measurements never overlap each other and the
/// allocation-heavy fuzz gates (read side) never stack on top of a
/// measurement's CPU budget. For clean diagnostics run with
/// `--test-threads=1`.
pub(crate) static LAB: std::sync::RwLock<()> = std::sync::RwLock::new(());

// ---------------------------------------------------------------------------
// Harness
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
    Compiler::new().compile(&program).expect("compile")
}

/// The syncdesk-shaped world every phase runs in: two named tickets, an
/// audit resource, an event handler.
const DECLS: &str = r#"
component Ticket { title: "", status: "open", assignee: "" }
resource Audit { log: "" }
event NoteAdded { ticket, note }
on NoteAdded(e) {
    let t = get(e.ticket, Ticket)
    match t {
        Some(tv) => {
            let a = get_resource(Audit) |> unwrap
            set_resource(Audit, Audit { log: a.log + f"[{tv.title}: {e.note}] " })
        }
        None => {}
    }
}
fn seed() {
    let _t1 = spawn("T-1", Ticket { title: "db latency" })
    let _t2 = spawn("T-2", Ticket { title: "login 500s" })
}
"#;

struct Slope {
    live_per_iter: f64,
    dropped_per_iter: f64,
    gc_objects_per_iter: f64,
}

// ---------------------------------------------------------------------------
// Phases: each stage of the server's DPUSH cycle, in isolation, plus the
// whole cycle. Divergence is forced with a per-iteration value so deltas
// are never empty.
// ---------------------------------------------------------------------------

fn phases() -> Vec<(&'static str, String)> {
    vec![
        ("baseline (empty loop)", "let _lab_x = lab_i".to_string()),
        ("fork()", "let _f = fork()".to_string()),
        ("fork + commit", "let f = fork()\ncommit(f)".to_string()),
        (
            "fork_to_bytes (PULL)",
            "let _b = fork_to_bytes(fork())".to_string(),
        ),
        (
            "fork_from_bytes (ingest)",
            // Same bytes each iteration: pure decode cost.
            "let _g = fork_from_bytes(lab_bytes) |> unwrap".to_string(),
        ),
        (
            "fork_delta (encode)",
            r#"let base = fork()
let t = get_entity("T-1")
update(t, Ticket) { assignee = f"a{lab_i}" }
let _d = fork_delta(base, fork())
commit(base)"#
                .to_string(),
        ),
        (
            "fork_apply (decode)",
            // Same delta each iteration against an unchanging base.
            "let _g = fork_apply(lab_base, lab_delta) |> unwrap".to_string(),
        ),
        (
            "merge_forks",
            r#"let base = fork()
let t = get_entity("T-1")
update(t, Ticket) { assignee = f"m{lab_i}" }
let ours = fork()
commit(base)
let _m = merge_forks(base, ours, ours) |> unwrap
commit(base)"#
                .to_string(),
        ),
        (
            "full DPUSH cycle (apply+merge+commit)",
            r#"let base = fork()
let t = get_entity("T-1")
update(t, Ticket) { assignee = f"c{lab_i}" }
let d = fork_delta(base, fork())
commit(base)
let theirs = fork_apply(base, d) |> unwrap
let merged = merge_forks(base, fork(), theirs) |> unwrap
commit(merged)"#
                .to_string(),
        ),
        (
            "note cycle (emit+flush, log grows by design)",
            r#"let t = get_entity("T-1")
emit NoteAdded { ticket: t, note: f"n{lab_i}" }
flush_events()"#
                .to_string(),
        ),
    ]
}

/// Fixtures referenced by phase bodies (`lab_bytes`, `lab_base`,
/// `lab_delta`), prepared once before the measured loop.
fn fixtures_for(body: &str) -> String {
    let mut pre = String::new();
    if body.contains("lab_bytes") {
        pre.push_str("let lab_bytes = fork_to_bytes(fork())\n");
    }
    if body.contains("lab_base") || body.contains("lab_delta") {
        pre.push_str(
            r#"let lab_base = fork()
let lab_t = get_entity("T-1")
update(lab_t, Ticket) { status = "escalated" }
let lab_delta = fork_delta(lab_base, fork())
commit(lab_base)
"#,
        );
    }
    pre
}

/// Measure the per-iteration memory slope of `body` between two iteration
/// counts. Constants (compile, setup, interning) cancel; designed warmup
/// growth (ledger to its cap, gc threshold doubling) is absorbed by
/// `n_small` being past it.
fn measured_slope(body: &str, n_small: usize, n_big: usize) -> Slope {
    let pre = fixtures_for(body);
    let _guard = LAB.write().unwrap();
    let run = |n: usize| {
        let src = format!(
            "{DECLS}\nseed()\n{pre}for lab_i in range(0, {n}) {{\n{body}\n}}\ngc_collect()\n"
        );
        let compiled = compile(&src);
        let before = net_bytes();
        let mut vm = VM::new();
        vm.suppress_output();
        vm.set_random_seed(7);
        // The ledger grows by design until its retention cap; cap it tight
        // so designed growth flattens before `n_small` and any remaining
        // slope is a real leak.
        vm.ledger.set_retention_cap(256);
        vm.load_compile_result(compiled);
        vm.run(0).expect("phase run");
        let live = net_bytes() - before;
        let gc_objects = vm.gc.object_count();
        drop(vm);
        let dropped = net_bytes() - before;
        (live, dropped, gc_objects)
    };
    let (live_s, dropped_s, gc_s) = run(n_small);
    let (live_b, dropped_b, gc_b) = run(n_big);
    let d = (n_big - n_small) as f64;
    Slope {
        live_per_iter: (live_b - live_s) as f64 / d,
        dropped_per_iter: (dropped_b - dropped_s) as f64 / d,
        gc_objects_per_iter: (gc_b as f64 - gc_s as f64) / d,
    }
}

// ---------------------------------------------------------------------------
// The lab report: run every phase, print slopes. This is the diagnostic —
// read the table, find the phase whose slope is fat, open that code.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "diagnostic: run with --release --ignored --nocapture --test-threads=1"]
fn leak_lab_report() {
    println!();
    println!("== leak lab: bytes per iteration (live = after gc_collect, dropped = after VM teardown) ==");
    println!(
        "{:<45} {:>12} {:>14} {:>12}",
        "phase", "live B/iter", "dropped B/iter", "gc obj/iter"
    );
    for (name, body) in phases() {
        let s = measured_slope(&body, 200, 600);
        println!(
            "{:<45} {:>12.1} {:>14.1} {:>12.2}",
            name, s.live_per_iter, s.dropped_per_iter, s.gc_objects_per_iter
        );
    }
    println!("== end ==");
}

// ---------------------------------------------------------------------------
// Regression: the full server push path must be memory-flat per cycle.
// "Flat" still allows small designed costs (free-list churn, map rehash),
// but a soak-killing leak (hundreds of KB per cycle) fails loudly.
// ---------------------------------------------------------------------------

#[test]
fn push_cycle_memory_is_flat() {
    let body = r#"let base = fork()
let t = get_entity("T-1")
update(t, Ticket) { assignee = f"c{lab_i}" }
let d = fork_delta(base, fork())
commit(base)
let theirs = fork_apply(base, d) |> unwrap
let merged = merge_forks(base, fork(), theirs) |> unwrap
commit(merged)"#;
    // Counters are per-thread now, so concurrent tests cannot pollute a
    // sample; the retries stay as insurance (a real leak fails every one).
    let mut last = measured_slope(body, 200, 600);
    for _ in 0..2 {
        if last.dropped_per_iter < 512.0 && last.live_per_iter < 2048.0 {
            break;
        }
        last = measured_slope(body, 200, 600);
    }
    assert!(
        last.dropped_per_iter < 512.0,
        "push cycle leaks {:.0} B/cycle past VM teardown — a 1-hour soak at \
         10 cycles/s would lose {:.0} MB",
        last.dropped_per_iter,
        last.dropped_per_iter * 36_000.0 / 1_048_576.0
    );
    assert!(
        last.live_per_iter < 2048.0,
        "push cycle accumulates {:.0} B/cycle that gc_collect cannot reclaim",
        last.live_per_iter
    );
}
