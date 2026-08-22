# Cost, phases, provenance, models, and FFI

This page is normative for production-boundary features 7–11. Their cumulative
acceptance services are Dispatch60, MatchFlow, AccessLens, WorkPulse, and
RiskBridge.

## 7. Cost contracts and query plans

### Guarantee

Hot roots declare semantic work and allocation contracts:

```rad
@frame
@no_full_scan
@no_guest_allocation
@budget(instructions: 80000)
system AdvanceActiveTrips(...) {
    visit_view(ActiveTrips, advance_one)
}
```

The graph follows imported helpers and statically bounded callbacks. It
classifies indexed lookup as `O(1)`, ordered range as `O(log n + k)`, view
iteration as `O(k)`, world/component scans as `O(n)`, and sort as
`O(n log n)`. A helper-hidden scan still violates `@no_full_scan`.

Allocation contracts are precise and non-overlapping:

- `@no_guest_allocation`: no guest object or managed guest backing allocation;
- `@no_runtime_allocation`: no VM/runtime allocator call during the system;
- `@no_host_allocation`: no metered host-boundary allocation.

`rad bench` reports each category separately. A contract only passes when the
native allocator meter is installed; absence of measurement is not success.

```bash
rad query-plan AdvanceActiveTrips --file projects/dogfood/dispatch60/main.rad
rad cost-path AdvanceActiveTrips --file projects/dogfood/dispatch60/main.rad
rad bench projects/dogfood/dispatch60/bench.rad --json
```

### Non-guarantees and migration

Instruction counts are deterministic semantic work, not CPU cycles. Wall time
depends on the host and is reported rather than treated as a portable budget.
Move frame populations into materialized views/indexes and declare intentional
teardown scans with a reason; do not hide a scan behind an owner helper.

## 8. Explicit lifecycle phases and delivery

### Guarantee

Programs declare an ordered phase graph and choose delivery at each emission:

```rad
phase Input
phase Validate after Input
phase Resolve after Validate
phase Commit after Resolve
phase Publish after Commit

emit next DamageRequested { ... }
emit phase(Resolve) DamageRequested { ... }
signal sync SnapshotReady { ... }
emit RespawnWindowOpened { ... } after 2
```

`next` uses the next event buffer, `phase(P)` cannot target a completed phase,
`sync` runs immediately, and delayed events use deterministic frame delivery.
Handler contracts include `@no_nested_flush`, `@non_reentrant`,
`@must_complete_before(Phase)`, and `@exactly_once`. `assert_trace` validates a
subject's lifecycle trace.

Diagnostics name the source event, requested phase, completed barrier, or
reentrancy contract. Deferred authority remains separate unless a synchronous
signal or nested flush makes the handler reachable now.

### Non-guarantees and migration

The runtime does not infer whether a native transition was synchronous. Encode
the recovered contract explicitly. MatchFlow proves byte-identical state and
event order for `Input -> Validate -> Simulate -> Resolve -> Commit -> Publish`.

## 9. Negative and field provenance

### Guarantee

Provenance covers writes, individual fields, removals, missing index keys,
materialized-view membership/exclusion, and view revision movement:

```rad
why(user, DeviceTrust)
why_field(user, DeviceTrust, "level")
why_removed(user, Entitlement)
why_missing(Entitlement, user_id)
why_not_in_view(ProductionDeployers, user)
why_revision_changed(ProductionDeployers)
why_revision_did_not_change(ProductionDeployers, revision)
```

The same operations are available without adding logging:

```bash
rad why 0 DeviceTrust --file projects/dogfood/accesslens/main.rad
rad why-field 0 DeviceTrust level --file projects/dogfood/accesslens/main.rad
rad why-removed 0 Entitlement --file projects/dogfood/accesslens/main.rad
rad why-not-in-view ProductionDeployers 0 --file projects/dogfood/accesslens/main.rad
```

Deletion tombstones and field causes survive snapshot, wire transfer, and
replay. When a bounded ledger evicts history, the explanation includes a
truncation marker with the exact omitted-record count and a BLAKE3 digest of
the omitted prefix. It never presents a truncated chain as complete.

### Non-guarantees and migration

The digest proves which omitted prefix the runtime summarized; it does not
recover evicted text. Configure retention for the operational horizon. Remove
parallel ad-hoc audit caches after migrating to runtime provenance.

## 10. Stateful model checking

### Guarantee

A `model` declares commands, state invariants, temporal contracts, campaign
size, and a deterministic seed:

```rad
model JobLifecycleModel {
    commands [enqueue, lease, start, heartbeat, complete, timeout, retry, cancel]
    invariant { return active_lease_count(job) <= 1 }
    temporal {
        always BlockingReason,
        JobCompleted eventually Succeeded,
        CancellationRequested eventually_within JobCancelled 1,
        Running never_after Succeeded,
    }
    runs 10000
    max_commands 200
    seed 1234
}
```

Supported temporal forms are `always`, `eventually`, `until`, `exactly_once`,
`never_after`, and bounded `eventually_within`. The generator varies commands
and flush boundaries. On failure it deterministically shrinks the generated
history, records the failed property/reason/seed, embeds the merged source and
module layout, and writes a directly replayable `.radr` artifact.

```bash
rad model-check projects/dogfood/workpulse/tests/job_model.rad \
  --runs 10000 --max-commands 200 --seed 1234 \
  --artifact-dir artifacts/workpulse/failures --json
rad replay artifacts/workpulse/failures/failure.radr
rad shrink artifacts/workpulse/failures/failure.radr
```

Reports include histories, commands, counterexamples, minimum trace length,
shrink time, and median/p95 history time. Tests are isolated by default: every
test receives a fresh world. Suites that intentionally test shared state must
declare that mode explicitly.

### Non-guarantees and migration

Finite model checking does not prove an unbounded system. Bounds and seed are
part of the report. Commands must encode the real transition boundary; a model
cannot detect a business rule omitted from both model and implementation.

WorkPulse mutation-proves ten lifecycle failures, runs 10,000 histories of up
to 200 commands, and benchmarks 1,000,000 commands over 100,000 jobs.

## 11. Effect-declared native extensions

### Guarantee

The extension ABI carries a versioned descriptor, C calling convention,
exports, signatures, effects, deterministic/replayable flags, and exact native
type layouts. `rad ffi verify` loads the plugin in an isolated worker and checks
the descriptor against a JSON contract before production use.

```bash
rad ffi verify plugins/risk_model.dll --contract ffi-contract.json --json
```

Verification rejects ABI/version/export/calling-convention mismatches, wrong
size/alignment/offsets, overlapping fields, opaque nominal substitution,
effect/replay mismatch, and a failed same-input/same-generation determinism
probe. Host calls record plugin identity/digest, generation, input/output
digests and bytes, effect class, and outcome in the causal chain.

Crashes and timeouts terminate the worker operation and return typed
`HostCallFailure`; they do not corrupt the RAD process/world. A transaction
that fails after a successful host result still rolls back its authoritative
patch. Host handles cannot enter snapshots. Replay consumes the recorded host
result and must have no leftover or live host calls.

### Trust boundary and non-guarantees

Native machine code is not statically proven pure. RAD verifies declarations,
behavioral determinism probes, process containment, and replay records. The
worker still has its operating-system permissions; deploy untrusted plugins in
an OS sandbox with restricted credentials/network/filesystem access. In-process
native loading is a trusted low-latency choice with explicitly weaker crash
containment.

RiskBridge compares one million native decisions with the pure RAD reference
and replay, pins plugin generations across decisions, and retains cross-FFI
provenance.

## Portfolio acceptance

Run the exact-generation gate from a clean commit:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File devtools/accept-portfolio.ps1
```

The generated JSON report binds every result to the commit/tree, `Cargo.lock`,
release binary SHA-256, toolchain, target machine, command, log, CPU time, wall
time, peak working set/private bytes, replay result, benchmark digest, negative
diagnostic, and repository gate. A dirty or partial run is marked
`releaseEligible: false`.
