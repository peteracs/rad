# Production dogfood portfolio

The eleven services under `projects/dogfood/` are cumulative acceptance tests,
not isolated syntax samples.

| Project | Primary contract | Scale acceptance | Score |
|---|---|---:|---:|
| PagerGrid | Transitive effects | 50,000 alerts | 18/20 |
| FulfillOS | Write ownership | 10,000 orders | 18/20 |
| ClearPay | Atomic transactions | 10,000 exact batch captures + 100 scalar captures | 18/20 |
| ForgeLink | Native-fidelity types | 200,000 scalar round trips | 19/20 |
| MarketLens | Materialized views | 1,000,000 products | 19/20 |
| BookCore | Ordered indexes | 1,000,000 orders | 19/20 |
| Dispatch60 | Cost contracts | 50k vehicles, 100k jobs, 10k frames | 19/20 |
| MatchFlow | Phases and delivery | 50k players, 10k inputs | 18/20 |
| AccessLens | Negative provenance | 100k users, 10k revocations | 18/20 |
| WorkPulse | Stateful model checking | 10k histories; 2m model commands; 1m production commands | 20/20 |
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

The full run then checks formatting, workspace compilation, syntax/VM/CLI/LSP
suites, strict Clippy, warning-free rustdoc, native math kernels and packaging,
architecture/folder-tree/line-limit policy, ordinary and causal snapshots,
generated language/host API surfaces, executable documentation examples,
documentation links, mdBook, warning-free WASM compilation, and diff hygiene.

This is a release campaign, not a quick smoke test. Each RAD child is hard
limited to less than 1,000,000,000 ns end-to-end, while the orchestrator and
Rust/native build gates are timed separately. RiskBridge alone executes
one million adjudications in native benchmark, record, replay, and pure-reference
modes, so a full run can take tens of minutes. Its online decision window and
trace reader/writer are bounded; each million-case process has a 512 MiB private
memory ratchet. The 2026-08-23 release-profile receipt measured 97,464,320 bytes
for recording, 13,291,520 for replay, and 4,513,792 for the pure reference; the
final clean-commit campaign reissues these metrics from its own source-bound
report rather than inheriting them.
Do not rerun the complete project merely to validate an acceptance-regex or
report-format edit: execute the affected command directly, then reserve the
complete matrix for one final clean source generation.

`-Project` and `-AllowDirty` exist for development diagnosis only. Such reports
are explicitly `releaseEligible: false`; they cannot be presented as release
evidence. `-Project riskbridge` still runs RiskBridge's complete scale matrix;
it is not a lightweight check. `-SkipRepositoryGates` has the same status.

The JSON report is the source of truth. Human summaries must not combine
numbers from different source trees, binaries, profiles, machines, or cached
traces. Every RAD outcome records and enforces `elapsedNs < limitNs`; semantic
work, digests, allocation categories, intended diagnostics, and replay
equality remain the deterministic ratchets behind that machine-local deadline.
