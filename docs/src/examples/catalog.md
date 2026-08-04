# Example Catalog

All examples live in the repository [`examples/`](https://github.com/peteracs/rad/tree/main/examples) directory. Run any of them with:

```bash
rad examples/<name>.rad
```

## Speculative Execution examples

These examples demonstrate Rad's first-class world forking feature — fork the ECS, simulate systems in isolation, and use `peek()` to compare futures before committing.

| Example | What it demonstrates |
|---|---|
| `ai_move_selection.rad` | AI evaluates 3 strategies via peek, commits the safest one |
| `transaction_rollback.rad` | Banking system previews 6/12-month projections before applying |

## Starter examples

| Example | What it demonstrates |
|---|---|
| `demo.rad` | Complete showcase of all three laws |
| `pipeline.rad` | Deep dive into map/filter/reduce |
| `traffic_light.rad` | Pure state machine controller |
| `sum_types.rad` | Algebraic data types with match |
| `system_schedule.rad` | System ordering with before/after |
| `calculator.rad` | Simple expression evaluation |
| `todo_app.rad` | Task management with ECS |
| `weather.rad` | Weather data processing |

## Application examples

| Example | What it demonstrates |
|---|---|
| `ecs_benchmark.rad` | ECS performance with 100 entities x 50 ticks |
| `inventory.rad` | Item/equip system with ECS + events |
| `chat_system.rad` | Users, rooms, messages |
| `pure_pipelines.rad` | Pure function enforcement |
| `control_tower.rad` | End-to-end showcase: logistics + analytics |
| `data_pipeline.rad` | CSV-style data transformation |
| `rest_pipeline.rad` | REST API data transformation pipeline |
| `safe_ecs.rad` | Defensive ECS patterns with Option/Result |
| `idempotent_events.rad` | Event deduplication and idempotency |
| `shop.rad` | Shopping system with inventory management |

## Game and simulation examples

| Example | What it demonstrates |
|---|---|
| `platformer.rad` | 2D platformer with physics and collision |
| `dungeon.rad` | Procedural dungeon generation |
| `dialog_tree.rad` | Branching narrative dialog system |
| `animation.rad` | Sprite animation system |
| `particle_system.rad` | Particle effects with ECS |
| `music_player.rad` | Audio playback state machine |
| `elevator.rad` | Elevator controller FSM |
| `vending_machine.rad` | Vending machine state machine |
| `network_protocol.rad` | Network protocol state machine |

## Complex integration examples

| Example | What it demonstrates |
|---|---|
| `complex_dag_ci_orchestrator.rad` | DAG-style CI build orchestrator: typed components/resource, `query { Task }` + `require`, guarded `on` handlers, named `phase`s, `enumerate` / `find` / `max_by` / `min_by`, `update()` sugar, release state machine — stress-tests current DX extensions |

## Computational mathematics dogfood

| Project | What it demonstrates |
|---|---|
| [`frankl-search`](./frankl-search.md) | Native exact Boolean-quotient kernels, forked `simulate_many()` search, Causal Laws/constraints/`why()`/replay, an all-width theorem for at most seven join-generators, exact eight-generator graph/projected-CNF exclusions, and independently verified certificates |
| [`collatz-lab`](./collatz-lab.md) | Pruned affine residue trees, bounded-binary-support and natural-tail certificates, counterexample-guided exact-state frontier portfolios, irrational-slope ballot paths, exact odd-cycle equations, COW universes, Causal Laws/constraints/`why()`/replay, and VM-independent verifiers that isolate the forms of a possible Collatz counterexample |

## "Cursed" examples (pushing the language to its limits)

| Example | What it demonstrates |
|---|---|
| `cursed_bst.rad` | Binary search tree with pure functions |
| `cursed_brainfuck.rad` | Brainfuck interpreter |
| `cursed_graph.rad` | Graph algorithms (BFS, DFS, shortest path) |

## Suggested learning path

1. Start with `demo.rad` to see everything in one file
2. Read `pipeline.rad` to understand data flow
3. Try `traffic_light.rad` for state machines
4. Pick an application or game example closest to your use case
5. Build your own with `rad new myproject --template workflow`

## Quick start with templates

Forge includes opinionated project templates based on the flagship examples:

```bash
rad new my-app --template workflow      # approval pipeline
rad new my-app --template stream        # telemetry processor
rad new my-app --template simulation    # agent-based simulation
rad new my-app --template control-plane # fleet management
rad new my-app                          # minimal hello-world
```

Each template creates a runnable project — `rad main.rad` works immediately.
