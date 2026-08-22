# MatchFlow

MatchFlow is a deterministic authoritative match-frame service. It accepts player input, validates it, stages phase-targeted damage, atomically resolves combat, and publishes one committed snapshot.

Run the production scenario:

```bash
rad projects/dogfood/matchflow/main.rad
```

The frame progresses through `Input -> Validate -> Simulate -> Resolve -> Commit -> Publish`. `PlayerInput` and `DamageResolved` use next-flush delivery, `DamageRequested` is gated on `Resolve`, `SnapshotReady` is synchronous and exactly once, and `RespawnWindowOpened` is delayed. The lifecycle trace is asserted in the executable scenario.

Inspect the operational story without adding logging:

```bash
rad effects execute_authoritative_frame --file projects/dogfood/matchflow/main.rad
rad path execute_authoritative_frame '->' ApplyDamage --file projects/dogfood/matchflow/main.rad
rad why-not-in-view AlivePlayers 1 --file projects/dogfood/matchflow/main.rad
rad query-plan AlivePlayers --file projects/dogfood/matchflow/main.rad
```

Try the failures in `negative/`: a late phase-targeted emit, duplicate delivery to an exactly-once handler, a nested flush, and a completion-barrier violation. Each failure identifies the event contract and lifecycle boundary that was crossed.

The scale workload in `bench.rad` publishes 50,000 players and resolves 10,000 lethal inputs. Stable opaque IDs, owner-only combat state, an atomic damage transaction, an incrementally maintained alive-player view, and an ordered simulation key carry forward the preceding portfolio features.

Portfolio score: learnability 2, ownership 2, errors 2, observability 2, determinism 2, performance 1, testability 2, refactor safety 2, host safety 1, production realism 2. Total: 18/20.
