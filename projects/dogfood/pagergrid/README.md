# PagerGrid

PagerGrid is a production-shaped incident router and the acceptance project for transitive effects. It ingests named services and incidents, specializes a shared higher-order router per call site, pages responders through an explicit synchronous flush, and writes an inspectable incident timeline.

Run the workflow:

```bash
rad projects/dogfood/pagergrid/main.rad
rad test projects/dogfood/pagergrid/tests/workflow.rad
rad effects RouteCritical --file projects/dogfood/pagergrid/main.rad
rad path RouteCritical '->' audit.append_timeline --file projects/dogfood/pagergrid/main.rad
rad writers IncidentTimeline --file projects/dogfood/pagergrid/main.rad
```

Introduce and diagnose a defect:

```bash
rad projects/dogfood/pagergrid/negative/hidden_write.rad
rad projects/dogfood/pagergrid/negative/callback_io.rad
rad projects/dogfood/pagergrid/negative/flush_handler.rad
```

The diagnostics identify the undeclared write or IO effect, print the transitive path, and name the required system grant. The two valid systems share `invoke` without inheriting each other's `EscalationState` and `NotificationState` writes.

Scale workload:

```bash
rad projects/dogfood/pagergrid/bench.rad
```

This routes 50,000 alerts across 50,000 named services with deterministic 10/90 critical/routine partitioning.

Score: learnability 2, ownership clarity 1, error quality 2, observability 2, determinism 2, performance 2, testability 2, refactor safety 2, host safety 1, production realism 2 — **18/20**.
