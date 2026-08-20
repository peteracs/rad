# Transitive authority dogfood

This workload proves that system signatures are enforced authority bounds,
including accesses reached through imported helpers and synchronously flushed
event handlers. It also provides a small graph for exercising `rad effects`,
`rad writers`, `rad readers`, and `rad path`.

```bash
rad projects/dogfood/authority-effects/main.rad
rad effects RemoveEntity --json --file projects/dogfood/authority-effects/main.rad
rad writers WireIdentity --file projects/dogfood/authority-effects/main.rad
rad readers LiveMembership --file projects/dogfood/authority-effects/main.rad
rad path mission_frame "->" full_scan --file projects/dogfood/authority-effects/main.rad
```
