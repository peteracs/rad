# RiskBridge

RiskBridge is RAD's native-extension capstone. It adjudicates purchase attempts
with an ABI-checked native model, a pure RAD reference model, and recorded host
responses. The decision transaction owns publication of scores, policy state,
manual-review membership, idempotency, and the decision ledger.

Build and verify the native boundary:

```bash
cargo build --manifest-path projects/dogfood/riskbridge/plugins/risk-model/Cargo.toml --target-dir target/riskbridge-plugin --release -j 1
rad ffi verify \
  target/riskbridge-plugin/release/riskbridge_model_plugin.dll \
  --contract projects/dogfood/riskbridge/ffi-contract.json
```

The verifier checks extension ABI v3, the C calling convention, export names
and signatures, effect/replay declarations, every struct size/alignment/field
offset, opaque nominal identities, and a same-input/same-generation
determinism probe. Loading a DLL is not considered verification.

Run the service and tests:

```bash
rad projects/dogfood/riskbridge/main.rad
rad test projects/dogfood/riskbridge/tests
rad effects adjudicate_native --file projects/dogfood/riskbridge/main.rad
rad query-plan InspectManualReviewQueue --file projects/dogfood/riskbridge/main.rad
rad why 2 PolicyDecision --file projects/dogfood/riskbridge/main.rad
rad why 2 ModelScore --file projects/dogfood/riskbridge/main.rad
```

Native calls execute in `rad-ffi-worker`. A crash, timeout, malformed result,
or typed host error terminates the worker call, returns `HostCallFailure`, and
leaves the authoritative world untouched. A decision pins one plugin
generation from call through commit. Replay consumes recorded host outputs and
does not invoke the live plugin.

The boundary is not an operating-system security sandbox. A plugin is a native
program with the worker process's OS permissions. RAD verifies declared effects
and determinism behavior, contains process crashes/timeouts, and records calls;
it cannot prove from machine code that a plugin never performs a syscall.
Untrusted plugins therefore require OS-level process isolation and credentials
appropriate to the deployment.

Failure coverage includes wrong ABI version, missing export, calling convention,
size, alignment, field offset, nominal identity, malformed enums/flags/scores,
effect mismatch, nondeterminism, crash, timeout, generation races, rollback,
and attempted snapshotting of a native handle.

The full benchmark is parameterized:

```bash
rad bench projects/dogfood/riskbridge/bench.rad --json -- 1000000 1000 native none
rad bench projects/dogfood/riskbridge/bench.rad --json -- 1000000 1000 reference none
```

Record/replay adds `--record <trace>` to the ordinary run form. The acceptance
runner compares native, reference, and replay outcomes for one million
adjudications and retains throughput, latency, memory, allocation, transaction,
recording, and replay evidence under `artifacts/portfolio/`.

Score: learnability 2, ownership clarity 2, error quality 2, observability 2,
determinism 2, performance 2, testability 2, refactor safety 2, host safety 2,
production realism 2 — **20/20**.
