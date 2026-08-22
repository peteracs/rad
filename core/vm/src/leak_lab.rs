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

use crate::parser::ParserOptions;
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
    static TL_MEASURE_LIVE: Cell<isize> = const { Cell::new(0) };
    static TL_MEASURE_PEAK: Cell<usize> = const { Cell::new(0) };
    static TL_MEASURE_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

fn measure_allocated(bytes: usize) {
    let _ = TL_MEASURE_ACTIVE.try_with(|active| {
        if !active.get() {
            return;
        }
        let _ = TL_MEASURE_LIVE.try_with(|live| {
            let next = live.get().saturating_add_unsigned(bytes);
            live.set(next);
            let _ = TL_MEASURE_PEAK.try_with(|peak| peak.set(peak.get().max(next.max(0) as usize)));
        });
    });
}

fn measure_freed(bytes: usize) {
    let _ = TL_MEASURE_ACTIVE.try_with(|active| {
        if active.get() {
            let _ = TL_MEASURE_LIVE
                .try_with(|live| live.set(live.get().saturating_sub_unsigned(bytes)));
        }
    });
}

struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            crate::allocation_meter::record_allocation(layout.size());
            let _ = TL_ALLOCATED.try_with(|c| c.set(c.get() + layout.size()));
            measure_allocated(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = TL_FREED.try_with(|c| c.set(c.get() + layout.size()));
        measure_freed(layout.size());
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            crate::allocation_meter::record_allocation(new_size);
            let _ = TL_ALLOCATED.try_with(|c| c.set(c.get() + new_size));
            let _ = TL_FREED.try_with(|c| c.set(c.get() + layout.size()));
            measure_freed(layout.size());
            measure_allocated(new_size);
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

/// Measure the peak native allocator growth on the current test thread.
/// Inputs must be prepared before entering this scope; the result includes
/// GC boxes and temporary Rust `Vec`/`String`/map allocations made by the
/// operation under test.
pub(crate) fn measure_peak_bytes<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            let _ = TL_MEASURE_ACTIVE.try_with(|active| active.set(false));
        }
    }

    TL_MEASURE_LIVE.with(|live| live.set(0));
    TL_MEASURE_PEAK.with(|peak| peak.set(0));
    TL_MEASURE_ACTIVE.with(|active| {
        assert!(
            !active.replace(true),
            "native peak measurements may not nest"
        )
    });
    let reset = Reset;
    let result = operation();
    let peak = TL_MEASURE_PEAK.with(Cell::get);
    drop(reset);
    (result, peak)
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
    crate::test_support::compile_source(src, ParserOptions).expect("parse and compile")
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
    measured_slope_with(DECLS, body, n_small, n_big)
}

/// As [`measured_slope`], with the declarations under test supplied by the
/// caller. World-retention phases need their own components: what a retained
/// entity costs depends on which of its fields are `indexed`, `ordered
/// indexed`, or watched by a materialized view, which one fixed declaration
/// set cannot express.
fn measured_slope_with(decls: &str, body: &str, n_small: usize, n_big: usize) -> Slope {
    let pre = fixtures_for(body);
    let _guard = LAB.write().unwrap();
    let run = |n: usize| {
        let src = format!(
            "{decls}\nseed()\n{pre}for lab_i in range(0, {n}) {{\n{body}\n}}\ngc_collect()\n"
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

// ---------------------------------------------------------------------------
// World retention: what one *retained* entity costs, by index kind.
//
// RiskBridge keeps every adjudication, so its footprint is not a leak — it is
// the per-entity price of the world, paid a million times. Process RSS cannot
// say which part of that price is column storage and which is index
// bookkeeping; these phases separate them by adding one index kind at a time.
// ---------------------------------------------------------------------------

const RETENTION_DECLS: &str = r#"
component One { a: u64 = u64(0) }
opaque type ShortId = u64
opaque type AnExtremelyLongOpaqueTypeNameUsedOnlyToMeasureStringCostInStoredValues = u64
component ShortNamed { a: ShortId = ShortId(u64(0)) }
component LongNamed { a: AnExtremelyLongOpaqueTypeNameUsedOnlyToMeasureStringCostInStoredValues = AnExtremelyLongOpaqueTypeNameUsedOnlyToMeasureStringCostInStoredValues(u64(0)) }
component IntOne { a: int = 0 }
component IntTwo { a: int = 0, b: int = 0 }
component FloatTwo { a: float = 0.0, b: float = 0.0 }
component Two { a: u64 = u64(0), b: u64 = u64(0) }
component Eight {
    a: u64 = u64(0), b: u64 = u64(0), c: u64 = u64(0), d: u64 = u64(0),
    e: u64 = u64(0), f: u64 = u64(0), g: u64 = u64(0), h: u64 = u64(0),
}
component P2 { a: u64 = u64(0) }
component P3 { a: u64 = u64(0) }
component P4 { a: u64 = u64(0) }
component Plain { a: u64 = u64(0), b: u64 = u64(0) }
component Hashed { indexed key: u64 = u64(0) }
component Ordered { ordered indexed rank: u64 = u64(0) }
component Viewed { flag: bool = true }
materialized view ViewedRows { depends [Viewed] }
fn seed() -> nil {}
"#;

fn retention_phases() -> Vec<(&'static str, String)> {
    vec![
        ("baseline (no spawn)", "let _x = lab_i".to_string()),
        // The two opaque phases below must report the same figure: a value
        // stores a shared handle to its type's name, not a copy of it. They
        // differed by exactly the 63 extra characters before that was true.
        (
            "opaque field, 7-char type name",
            "let _e = spawn(ShortNamed { a: ShortId(u64(lab_i)) })".to_string(),
        ),
        (
            "opaque field, 70-char type name (must match)",
            "let _e = spawn(LongNamed { a: AnExtremelyLongOpaqueTypeNameUsedOnlyToMeasureStringCostInStoredValues(u64(lab_i)) })".to_string(),
        ),
        (
            "spawn: 1 int field (inline value)",
            "let _e = spawn(IntOne { a: lab_i })".to_string(),
        ),
        (
            "spawn: 2 int fields (inline)",
            "let _e = spawn(IntTwo { a: lab_i, b: lab_i })".to_string(),
        ),
        (
            "spawn: 2 float fields (inline)",
            "let _e = spawn(FloatTwo { a: float(lab_i), b: float(lab_i) })".to_string(),
        ),
        (
            "spawn: 1 component, 1 field",
            "let _e = spawn(One { a: u64(lab_i) })".to_string(),
        ),
        (
            "spawn: 1 component, 2 fields",
            "let _e = spawn(Two { a: u64(lab_i), b: u64(lab_i) })".to_string(),
        ),
        (
            "spawn: 1 component, 8 fields",
            "let _e = spawn(Eight { a: u64(lab_i) })".to_string(),
        ),
        (
            "spawn: 4 components, 1 field each",
            "let _e = spawn(One { a: u64(lab_i) }, P2 { a: u64(lab_i) }, P3 { a: u64(lab_i) }, P4 { a: u64(lab_i) })".to_string(),
        ),
        (
            "spawn: plain component",
            "let _e = spawn(Plain { a: u64(lab_i), b: u64(lab_i) })".to_string(),
        ),
        (
            "spawn: hashed-indexed component",
            "let _e = spawn(Hashed { key: u64(lab_i) })".to_string(),
        ),
        (
            "spawn: ordered-indexed component",
            "let _e = spawn(Ordered { rank: u64(lab_i) })".to_string(),
        ),
        (
            "spawn: view-watched component",
            "let _e = spawn(Viewed { flag: true })".to_string(),
        ),
        (
            "spawn: all four together",
            "let _e = spawn(Plain { a: u64(lab_i), b: u64(lab_i) }, Hashed { key: u64(lab_i) }, Ordered { rank: u64(lab_i) }, Viewed { flag: true })".to_string(),
        ),
    ]
}

#[test]
#[ignore = "diagnostic: run with --release --ignored --nocapture --test-threads=1"]
fn world_retention_report() {
    println!();
    println!("== world retention: live bytes per RETAINED entity ==");
    println!(
        "{:<44} {:>12} {:>12}",
        "phase", "live B/iter", "gc obj/iter"
    );
    for (name, body) in retention_phases() {
        let slope = measured_slope_with(RETENTION_DECLS, &body, 2_000, 6_000);
        println!(
            "{:<44} {:>12.1} {:>12.2}",
            name, slope.live_per_iter, slope.gc_objects_per_iter
        );
    }
}
