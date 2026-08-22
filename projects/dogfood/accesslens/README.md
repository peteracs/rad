# AccessLens

AccessLens is a zero-trust entitlement service that answers both “why allowed?” and “why denied?” without temporary logging. It publishes a 10,000-user directory, repairs Alice's device posture, revokes her production entitlement after an HR event, and preserves the complete explanation through a wire snapshot and restoration.

Run it:

```bash
rad projects/dogfood/accesslens/main.rad --record artifacts/accesslens.radr
```

The service uses opaque user IDs, owner-only policy facts, contract transactions, a transactionally maintained `ProductionDeployers` view, ordered directory identity, explicit event delivery, field provenance, removal tombstones, missing-index explanations, and revision explanations. `systems/policy.rad` deliberately imports its owner under an alias while the scenario imports it bare; path-canonical module identity guarantees both names address one state instance.

Operational inspection:

```bash
rad replay artifacts/accesslens.radr
rad effects expire_contractor --file projects/dogfood/accesslens/main.rad
rad writers Entitlement --file projects/dogfood/accesslens/main.rad
rad readers DeviceTrust --file projects/dogfood/accesslens/main.rad
```

The `negative/` fixtures demonstrate direct owner bypass, direct trust mutation, attempted view mutation, and an incorrect revision expectation. The diagnostics direct developers to `RevokeProductionAccess`, `SetDeviceTrust`, or the materialized-view boundary.

`bench.rad` publishes 100,000 users and revokes 10,000 entitlements. Identical input and seed produce identical state, explanations, event order, wire bytes, and replay.

Portfolio score: learnability 2, ownership 2, errors 2, observability 2, determinism 2, performance 1, testability 2, refactor safety 2, host safety 1, production realism 2. Total: 18/20.
