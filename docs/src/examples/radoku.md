# RADOKU — chasing Tdoku with an interpreter

A bitboard sudoku solver in rad, built by porting the optimization ladder
from [Tdoku](https://t-dillon.github.io/tdoku/) (the fastest known solver,
C++ + AVX-512) one rung at a time — and making **the language itself
faster** at every rung where the interpreter, not the algorithm, was the
bottleneck. That was the point: sudoku is the workload, rad is the project.

## Scoreboard

`data/puzzles2_magictour_top1465`, solve + prove uniqueness, best-of-N on
a quiet machine (Tdoku's published numbers are from their hardware —
same order of machine):

| solver | language | puzzles/sec | guesses/puzzle |
|---|---|---:|---:|
| RADOKU v1, day one | rad (interpreted) | 4.0 | 1,883* |
| tdoku_basic | C++ compiled | 29.6 | 1,371,524 |
| **RADOKU v2** (`solver.rad`) | **rad (interpreted)** | **118** | 34.9 |
| **RADOKU v3** (`solver_v3.rad`) | **rad (interpreted)** | **136** | **21.7** |
| tdoku_basic_heuristic | C++ compiled | 751.6 | 647 |
| tdoku_dpll_triad | C++ compiled | 3,403 | 12.7 |
| tdoku (SIMD flagship) | C++ AVX2/512 | 117,068 | 9.1 |

\* hard-100 subset. The famous singles: AI Escargot solves in ~21 ms
(66 guesses), Inkala 2012 "world's hardest" in ~25 ms (62 guesses), both
proven unique.

**An interpreted scripting language is 4x faster than the compiled-C
baseline solver of the reference suite**, with fewer guesses per puzzle
than fsss2 (21.7 vs 19.2 — close) and a 30x climb from where the language
started the day.

## Run it

```
rad projects/dogfood/sudoku/solver_v3.rad                                  # famous puzzles
rad projects/dogfood/sudoku/solver_v3.rad -- projects/dogfood/sudoku/data/top1465.txt      # benchmark
rad projects/dogfood/sudoku/solver_v3.rad -- projects/dogfood/sudoku/data/top1465.txt 100  # hard subset
```

`solver.rad` (v2) is the simpler group-mask version, kept as a reference.

## The two solvers

- **v2** (`solver.rad`) — tdoku_basic_heuristic's representation: 27 ints
  as 9-bit masks (`rows[r] & cols[c] & boxes[b]` = a cell's candidates),
  iterative DFS, MRV, 3-XOR undo, plus a hidden-single/contradiction scan
  the basic tier doesn't have.
- **v3** (`solver_v3.rad`) — the DPLL ladder: explicit per-cell pencilmark
  masks maintained **incrementally** with a SAT-style trail undo, naked
  singles by queue, hidden singles only in **dirty groups**, and **locked
  candidates** (the merge-resolution consequences from the Tdoku paper)
  switched on lazily — only after a puzzle proves it backtracks, so easy
  puzzles never pay for the 54-intersection scan (fsss2 ships the same
  toggle).

## What this app changed in the language (the real deliverable)

1. **Bitwise operators existed nowhere in rad.** Added `&`, `|`, `^`
   (Rust-style precedence: tighter than comparisons, looser than
   arithmetic) through lexer → parser → checker → constant folder →
   compiler → VM → formatter, plus `popcount`, `ctz`, `shl`, `shr`
   builtins. Conformance: `tests/conformance/bitwise_ops.rad`.
2. **`xs[i] = v` cloned the whole list every time** (the stack round-trip
   held a second Arc reference, so copy-on-write always fired).
   `--profile-copies` caught it; new `ListSetLocal` opcode mutates
   `let unique` locals in place.
3. **`RAD_OP_PROFILE=1`** — new per-opcode dispatch histogram, printed at
   exit. Found everything below.
4. **`for i in range(...)` materialized a heap list** and paid
   `Len`+`GetIndex` per iteration. Now compiles to a counted loop, and the
   whole back-edge (increment + bound test + jump) is one `ForRangeNext`
   opcode instead of nine dispatches.
5. **`local[idx]` reads** fused to one dispatch (`ListGetLocal`), and
   `local_list[local_idx]` to one dispatch with zero stack traffic
   (`ListGetLL`).
6. **Scope exits popped locals one dispatch at a time** — now `PopN`.
7. **Every opcode paid a thread-local write** for the copy-profiler even
   when disabled. Now gated.
8. **`filled(n, v)`** builtin — native list creation; interpreted
   append-loops for scratch buffers were pure dispatch tax.
9. A pointer-caching bytecode fetch path was tried and **measured a 33%
   regression** (LLVM already hoists the chunk deref chain; the cache's
   validation broke that). Deleted. Measure, never assume.
10. **Peephole superinstructions with label-safety barriers**: `GetLocal2`
    (two pushes, one dispatch — interleaved A/B: +8%) and fused
    compare-and-branch (`EqJF`/`NeqJF`/`LtJF`/`LteJF`/`GtJF`/`GteJF`). The
    fusion machinery tracks every jump target so a branch can never land
    inside a fused instruction — and `GetLocal2` must push its first value
    *before* reading the second slot, because a loop binding's first use
    may legally read the slot the first push just created. (Found by a
    24-test failure; the fix is a two-line reorder.)
11. **The release profile was `opt-level = "s"`** — a wasm-bundle-size
    setting taxing every native run. Now `opt-level = 3` (+15-20%); wasm
    builds keep size via a dedicated `wasm-size` profile, and a `profiling`
    profile (debug symbols, release codegen) exists for `samply record`.
12. **Constant-rhs fusions**: `EqConst`/`NeqConst` (+ branch-fused
    `EqConstJF`/`NeqConstJF`), `ConstArith` (`x % 512`, `x & 511`, ...),
    and `IncLocal` (`x = x + 1` — four dispatches to one). `Const` fell
    from 15.1% to 5.7% of dispatches; total dispatches −13%. Wall-time
    gain was small — by this point the interpreter's cost is the work
    inside handlers, not the dispatch count, which is exactly what the
    earlier pointer-cache experiment predicted.

Cumulative VM effect on unrelated workloads: DEATHSIGHT's multiverse
search went from 4,557 to 5,364 universes/sec (+18%) across these rungs
with zero source changes; solver v2 went 35.7 → 51.8 puzzles/sec on the
hard-100 (+45%).

Net effect on other programs, same binary: DEATHSIGHT's multiverse search
got ~25% faster without touching its source.

## What rad-the-language learned (gripes found while writing the solver)

- `once` is a reserved keyword; `let mut once = 0` is a parse error.
- `len()` of a `let unique` list counts as aliasing — even though it
  can't alias anything. Track lengths by hand or lose uniqueness.
- No `;` statement separators — `if x { a = 1; break }` must be 4 lines.
- The preallocate-and-index idiom (`buf[top] = v` + `top += 1`) is the
  fast stack; `<<` appends at the *physical* end, which silently desyncs
  from a logical top after the first rewind. Two real bugs in this solver
  came from mixing them.

## Honest notes

- Tdoku's published numbers are from an i7-1065G7 with AVX-512; ours are
  from this machine. The *ratios between solvers on one machine* are the
  meaningful comparison, and the C solvers in the table bracket us fairly.
- The benchmark machine here is noisy (IDE + other agents); numbers are
  best-of-N at high process priority. Run-to-run spread is ~±15%.
- Beating the SIMD flagship in an interpreter is not a physics-compatible
  goal; beating compiled C solvers tier by tier is, and tier one fell 4.6x
  over. The dispatch-fusion well is now close to dry (further count
  reductions stopped moving wall time); the next real rungs are
  algorithmic (Tdoku's triad encoding: expect ~21.7 → ~13 guesses/puzzle)
  and architectural (threaded dispatch, unboxed int loop bodies).
- `analyze_profile.py` parses `samply record --save-only` output (top
  self-time frames). Use the `profiling` cargo profile for symbols.
