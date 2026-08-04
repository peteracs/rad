# Built-in Functions

All collection builtins take the collection as the first argument, making them
pipeline-friendly: `[1,2,3] |> push(4) |> sort`.

See also: [Language Guarantees](./guarantees.md) for behavioral contracts and
[DX Updates](../guide/dx-updates.md) for match guards and CLI flags.

> **Performance note:** Lists use `Arc<Vec<Value>>`, maps use persistent HAMTs (`im::HashMap`), and strings use `Arc<str>`. Copy-on-write applies: if a value is uniquely owned, updates often reuse backing storage; if it is shared, the runtime clones before mutating to preserve value semantics. ECS reads (`get`/`peek`) deep-copy values across the air gap between persistent storage and the execution stack; string fields are O(1) via `Arc<str>` sharing.

{{#include builtins_parts/host_and_values.md}}
{{#include builtins_parts/collections_and_ecs.md}}
{{#include builtins_parts/speculation.md}}
{{#include builtins_parts/causality_and_merge.md}}
{{#include builtins_parts/wire_and_sessions.md}}
