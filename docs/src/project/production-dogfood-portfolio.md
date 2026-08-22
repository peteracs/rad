# Production dogfood portfolio

The eleven services under `projects/dogfood/` are cumulative acceptance tests,
not isolated syntax samples.

| Project | Primary contract | Scale acceptance | Score |
|---|---|---:|---:|
| PagerGrid | Transitive effects | 50,000 alerts | 18/20 |
| FulfillOS | Write ownership | 10,000 orders | 18/20 |
| ClearPay | Atomic transactions | 10,000 captures | 18/20 |
| ForgeLink | Native-fidelity types | 200,000 scalar round trips | 19/20 |
| MarketLens | Materialized views | 1,000,000 products | 19/20 |
| BookCore | Ordered indexes | 1,000,000 orders | 19/20 |
| Dispatch60 | Cost contracts | 50k vehicles, 100k jobs, 10k frames | 19/20 |
| MatchFlow | Phases and delivery | 50k players, 10k inputs | 18/20 |
| AccessLens | Negative provenance | 100k users, 10k revocations | 18/20 |
| WorkPulse | Stateful model checking | 10k histories; 1m commands | 20/20 |
| RiskBridge | Native FFI/replay capstone | 1m adjudications per mode | 20/20 |

Every project contains a runnable workflow, focused tests, expected failures,
an operational inspection story, and a scale workload. Later projects reuse
earlier boundaries. A negative passes only when it exits nonzero for its named
contract; parse errors and unrelated earlier failures are acceptance failures.

The release command is:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File devtools/accept-portfolio.ps1
```

It requires a clean commit by default and creates a new immutable directory
under `artifacts/portfolio/`; it never deletes or overwrites a prior campaign.
For every selected project it runs:

1. production main and workflow suite;
2. each negative with its exact diagnostic pattern;
3. record/replay with final world-digest verification;
4. effects, shortest authority path, query plan, and why/why-not inspection;
5. scale benchmark with process and runtime metrics;
6. feature-specific model or FFI campaigns.

The full run then checks formatting, workspace compilation, VM/CLI/LSP suites,
strict Clippy, architecture/folder-tree/line-limit policy, ordinary and causal
snapshots, documentation links, mdBook, release build, and WASM compilation.

`-Project` and `-AllowDirty` exist for development diagnosis only. Such reports
are explicitly `releaseEligible: false`; they cannot be presented as release
evidence. `-SkipRepositoryGates` has the same status.

The JSON report is the source of truth. Human summaries must not combine
numbers from different source trees, binaries, profiles, machines, or cached
traces. Wall time is reported, not portability-gated; semantic work, digests,
allocation categories, intended diagnostics, and replay equality are the
deterministic ratchets.
