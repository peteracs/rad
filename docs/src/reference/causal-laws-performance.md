# Causal Laws performance baselines

RAD Causal Laws keeps a dedicated benchmark and growth suite because proposal
fan-in, canonical sorting, candidate adoption, and provenance have different
cost curves. The baseline is a release-engineering gate, not a promise that an
experimental syntax will never change internally.

## What is measured

`core/vm/benches/rad_benchmarks.rs` reports these Criterion groups:

| Group | Boundary |
| --- | --- |
| `causal/phase/proposal_creation` | Typed proposal allocation in the pure oracle. |
| `causal/phase/canonical_sort` | Canonical multiset ordering, including duplicate preservation. |
| `causal/phase/resolver_candidate` | Resolver computation and sparse candidate construction. |
| `causal/phase/candidate_adoption` | Clone-on-write reference adoption for touched subjects. |
| `causal/reference_group_sort_resolve_patch` | Complete unoptimized oracle settlement. |
| `causal/settlement_end_to_end` | Production parser-independent VM call through atomic commit and provenance. |
| `causal/provenance/why_render` | Bounded causal fan-in explanation rendering. |
| `causal/provenance/wire_encode` | Full retained provenance closure encoding. |
| `causal/constraints/accepted` | Candidate selection and two successful validation-only constraints. |
| `causal/constraints/rejected_and_encoded` | Atomic rejection plus exact canonical structured encoding. |

The suite sweeps `1`, `10`, `100`, `1,000`, and `10,000` proposals. Provenance
rendering and wire encoding use `1`, `100`, and `10,000` to keep the regular CI
run focused.

Run only the causal baseline with:

```text
cargo bench -p rad-vm --bench rad_benchmarks -- "causal/"
```

GitHub's **Benchmarks** workflow publishes the full Criterion report as the
`criterion-report` artifact for 30 days. Compare results from the same runner
class; workstation and CI timings are not interchangeable.

An initial short-sample workstation run (Windows GNU, release `rlib`, default
native-WASM feature disabled) measured:

| Proposals | Production settlement | Proposal creation | Canonical sort | Resolver candidate | `why()` | Wire encode |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 9.75 us | 0.42 us | 0.007 us | 0.21 us | 0.84 us | 0.81 us |
| 10 | 36.0 us | 2.13 us | 0.55 us | 0.25 us | - | - |
| 100 | 257 us | 19.5 us | 1.88 us | 0.70 us | 2.25 us | 18.3 us |
| 1,000 | 2.50 ms | 223 us | 19.1 us | 5.84 us | - | - |
| 10,000 | 27.6 ms | 2.50 ms | 213 us | 84.2 us | 4.71 us | 1.95 ms |

These numbers establish scale and phase attribution only. The independently
published Linux CI artifact is the comparison baseline for future commits.

## Memory and explanation growth

The executable test
`provenance_fan_in_wire_growth_is_linear_and_default_rendering_is_bounded`
uses canonical wire bytes as a stable retained-size proxy across the full
`1..10,000` sweep. It also requires default `why()` output to show at most
eight proposal branches and report the omitted count. The ledger retains the
complete fan-in according to its configured retention cap; only presentation
is bounded.

The initial canonical-size baseline is:

| Proposals | Provenance wire | Default `why()` |
| ---: | ---: | ---: |
| 1 | 186 B | 245 B |
| 10 | 881 B | 1,046 B |
| 100 | 8,083 B | 1,054 B |
| 1,000 | 82,785 B | 1,062 B |
| 10,000 | 856,787 B | 1,070 B |

These byte counts are deterministic test outputs, not allocator heap
measurements. They make retained growth and bounded presentation reviewable
without depending on a platform-specific memory profiler.

Run the growth baseline with:

```text
cargo test -p rad-vm provenance_fan_in_wire_growth_is_linear_and_default_rendering_is_bounded -- --nocapture
```

## Interpretation rules

- Optimize only after a repeated same-runner regression identifies a phase.
- Keep the pure reference model deliberately unoptimized.
- Never trade order independence, patch conflict rejection, or atomic abort
  for a lower benchmark number.
- Report wire size alongside render time: bounded `why()` output must not hide
  unbounded retained provenance accidentally.
