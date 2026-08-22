# BookCore

BookCore is a deterministic exchange order book using owned order state, atomic fills, a materialized open-order view, and lexicographically ordered composite indexes. Buy priority stores negated signed price plus sequence; sell priority stores price plus sequence, so `first()` selects price-time priority without sorting.

```bash
rad projects/dogfood/bookcore/main.rad --record bookcore.radr
rad replay bookcore.radr
rad query-plan MatchOrders --file projects/dogfood/bookcore/main.rad
rad test projects/dogfood/bookcore/tests
```

The production scenario submits 10,000 orders, rejects duplicate IDs and priority keys at the owner boundary, fills every crossing pair atomically, and proves the `OpenOrders` view drains with the book. `bench.rad` replays one million orders. Signed native values use numeric ordering; composite keys are lexicographic; equal-key buckets and entity traversal remain deterministic across snapshots and replay.

Failure fixtures cover owner bypass, signed/unsigned namespace collapse, amendment outside the owner, and a frame-reachable scan plus sort. The compiler points to `order_book.submit_*`, the nominal type boundary, or the transitive cost path.

Score: learnability 2, ownership clarity 2, error quality 2, observability 2, determinism 2, performance 2, testability 2, refactor safety 2, host safety 1, production realism 2 — **19/20**.
