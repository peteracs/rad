# Transitive authority dogfood

This workload proves that system signatures enforce the complete synchronous
effect bound: state reads/writes, emitted events, IO, and async execution,
including effects reached through imported helpers, statically resolved
callbacks, and synchronously flushed event handlers. Queued handlers remain
visible in the complete impact report but execute behind a deferred authority
boundary. It also provides a small graph for exercising `rad effects`,
`rad writers`, `rad readers`, and `rad path`.

```bash
rad projects/dogfood/authority-effects/main.rad
rad effects RemoveEntity --json --file projects/dogfood/authority-effects/main.rad
rad writers WireIdentity --file projects/dogfood/authority-effects/main.rad
rad readers LiveMembership --file projects/dogfood/authority-effects/main.rad
rad path mission_frame "->" full_scan --file projects/dogfood/authority-effects/main.rad
```

The system declares query-neutral grants directly in its signature:

```rad
system RemoveEntity(
    live: mut LiveMembership,
    writes WireIdentity,
    emits EntityRetired,
) { ... }
```

Use `io true` or `async true` only when that effect is intentional. Both are
denied when omitted. Synthetic index authorities such as `"$entity_names"` and
`"$entity_identity"` are quoted because they describe runtime-owned indexes,
not user components.
