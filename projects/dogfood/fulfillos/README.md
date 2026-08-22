# FulfillOS

FulfillOS is a warehouse fulfillment service and the ownership acceptance project. Inventory, its revision resource, and the reorder threshold are semantically write-owned. Order and shipping systems can read stock and call the owner's operations, but direct writes are unrepresentable. A named migration module is the only co-owner.

```bash
rad projects/dogfood/fulfillos/main.rad
rad test projects/dogfood/fulfillos/tests
rad effects ReserveOrders --file projects/dogfood/fulfillos/main.rad
rad writers Inventory --file projects/dogfood/fulfillos/main.rad
```

The three negative fixtures demonstrate direct mutation, a typed replacement used to conceal a write, and an owned-field bypass:

```bash
rad projects/dogfood/fulfillos/negative/shipping_direct_write.rad
rad projects/dogfood/fulfillos/negative/concealed_replacement.rad
rad projects/dogfood/fulfillos/negative/owned_field.rad
```

Run `bench.rad` for 10,000 orders. Every stock transition passes through `inventory_owner`, while a test-only capability remains scoped to one test body.

Score: learnability 2, ownership clarity 2, error quality 2, observability 2, determinism 2, performance 1, testability 2, refactor safety 2, host safety 1, production realism 2 — **18/20**.
