# MarketLens

MarketLens is a production-shaped commerce projection service. Five owned source facts feed `SellableProducts` and `LowStockProducts`; the runtime maintains both views in the same commit as source writes. Readers perform keyed lookups and incremental change reads instead of rebuilding the catalog.

```bash
rad projects/dogfood/marketlens/main.rad --record marketlens.radr
rad replay marketlens.radr
rad test projects/dogfood/marketlens/tests
rad projects/dogfood/marketlens/bench.rad
```

The walkthrough seeds 20,000 products, proves that a description edit does not advance the sellable revision, removes a product immediately when stock reaches zero, inspects the exact failed `Inventory.available > 0` predicate, and restores membership with one revision change. `bench.rad` scales the same architecture to one million products and 50,000 transactional source updates.

Failure fixtures reject manual view mutation, undeclared predicate dependencies, unknown source fields, and a frame-reachable reader-side rebuild. Repair them by mutating source facts through `catalog_owner`, declaring the exact dependency set, and reading the maintained view.

Operational sequence:

```bash
rad projects/dogfood/marketlens/main.rad
rad projects/dogfood/marketlens/negative/hidden_dependency.rad
rad why-not-in-view SellableProducts 1 --file projects/dogfood/marketlens/main.rad
rad effects InspectSellable --file projects/dogfood/marketlens/main.rad
```

Score: learnability 2, ownership clarity 2, error quality 2, observability 2, determinism 2, performance 2, testability 2, refactor safety 2, host safety 1, production realism 2 — **19/20**.
