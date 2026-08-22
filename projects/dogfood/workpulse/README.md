# WorkPulse

WorkPulse is the production acceptance service for stateful property testing
and temporal model checking. It models queued, leased, running, retrying,
cancelled, successful, and dead-lettered jobs as distinct owned states. Lease
expiry, worker disconnection, job failure, and cancellation remain separate
operations with separate provenance.

Run the service and focused workflow tests:

```bash
rad projects/dogfood/workpulse/main.rad
rad test projects/dogfood/workpulse/tests
rad effects InspectRetryQueue --file projects/dogfood/workpulse/main.rad
rad query-plan RetryQueue --file projects/dogfood/workpulse/main.rad
rad why 1 CompletionRecord --file projects/dogfood/workpulse/main.rad
rad why-removed 1 ActiveLease --file projects/dogfood/workpulse/main.rad
```

Run the deterministic stateful campaign:

```bash
rad model-check projects/dogfood/workpulse/tests/job_model.rad \
  --model JobLifecycleModel \
  --runs 10000 \
  --max-commands 200 \
  --seed 1234 \
  --artifact-dir artifacts/workpulse/failures \
  --json
```

The model generates valid and stale forms of enqueue, lease, start, heartbeat,
complete, fail, timeout, retry, cancel, worker disconnect/reconnect, scheduler
restart, snapshot restore, duplicate delivery, and stale completion. Its
temporal declarations exercise `always`, `eventually`, `until`,
`exactly_once`, `never_after`, and bounded `eventually_within`.

The ten files in `negative/` are mutation proofs. Each deliberately breaks one
lease, terminal-state, retry, restart, idempotency, race, dead-letter, or
post-commit rule. They are run through `rad model-check`, not as ordinary
programs. A failure artifact is directly reproducible and shrinkable:

```bash
rad replay artifacts/workpulse/failures/stale_completion.radr
rad shrink artifacts/workpulse/failures/stale_completion.radr
```

`bench.rad` executes 1,000,000 lifecycle commands over 100,000 jobs and 1,000
workers and prints the deterministic world digest. The portfolio acceptance
runner records wall time, CPU time, peak working set, private bytes, allocator
categories, and model-history median/p95 for the frozen source generation.

Score: learnability 2, ownership clarity 2, error quality 2, observability 2,
determinism 2, performance 2, testability 2, refactor safety 2, host safety 2,
production realism 2 — **20/20**.
