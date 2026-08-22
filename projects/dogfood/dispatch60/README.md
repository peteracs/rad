# Dispatch60

Dispatch60 is a fixed-rate fleet scheduler shaped around 50,000 vehicles, 100,000 jobs, 2,000 zones, and a 600-trip active working set. The 60-Hz root traverses the incrementally maintained `ActiveTrips` view and mutates two SoA float cells through the fleet owner. It performs no world scan, list construction, component materialization, sort, or guest-heap allocation.

```bash
rad projects/dogfood/dispatch60/main.rad --record dispatch60.radr
rad replay dispatch60.radr
rad query-plan AdvanceActiveTrips --file projects/dogfood/dispatch60/main.rad
rad cost-path AdvanceActiveTrips --file projects/dogfood/dispatch60/main.rad
rad test projects/dogfood/dispatch60/tests
rad bench projects/dogfood/dispatch60/bench.rad
```

The production run proves active-only movement and deterministic positions. The scale workload runs 10,000 frames with the full fleet/job population. `read_field` and `write_field` preserve exact transitive authority, owner checks, transaction limits, indexes, views, and field provenance; heap-backed guest values remain visible to `@no_guest_allocation`. `rad bench` reports guest objects, VM-runtime allocator calls, direct managed-heap backing, and metered host-boundary allocation as separate non-overlapping contracts.

Negative fixtures expose a helper-hidden full scan, a helper-hidden allocation/sort, direct mutation outside the fleet owner, and a dynamic field name that would make cost and authority imprecise. Each diagnostic identifies the transitive cost path or the public owner boundary.

Score: learnability 2, ownership clarity 2, error quality 2, observability 2, determinism 2, performance 2, testability 2, refactor safety 2, host safety 1, production realism 2 — **19/20**.
