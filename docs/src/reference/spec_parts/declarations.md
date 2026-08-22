<a id="declarations"></a>
## 3. Declarations

Declarations install nominal schemas, callable code, execution topology, test
contracts, or top-level behavior. Unless stated otherwise, a named declaration
is file-private; prefix it with `pub` to export it. Public fields, parameters,
and returns require explicit types.

<a id="decl-components"></a>
### 3.1 Components and resources

```text
pub component AssetIdentity owned with [migration_tools] {
    indexed id: AssetId = AssetId(u64(0)),
    ordered indexed priority: (u16, u64) = (u16(0), u64(0)),
    zone: ZoneId = ZoneId(u16(0)),
}

pub component Profile {
    name: str = "",
    owned rank: int = 0,
}

pub resource Ledger owned { revision: u64 = u64(0) }
pub transient resource Scratch { probes: int = 0 }
```

A component may be attached to entities. A resource is one runtime-owned
singleton. `transient resource` participates in execution/forks but is omitted
from persistent world encoding and semantic world digests.

`indexed` maintains exact-key lookup. `ordered indexed` additionally maintains
ascending deterministic traversal. An index declaration is part of the schema
and is rebuilt/validated on load.

`owned` grants semantic writes to the declaring canonical module. `with [...]`
names co-owner module identities. `transfer to module` moves ownership and
revokes the declaring module. Field-level `owned` restricts writes to that
field while leaving other fields writable. Owner/co-owner/test code must still
declare `writes owned [Type]` or `writes owned [Type.field]` on the callable;
outside modules cannot forge that grant.

Schema versions use `component Name v2 { ... }` and `resource Name v3 { ... }`.
The version is persisted and supplied to the owning migration.

<a id="decl-structs-native"></a>
### 3.2 Structs and native declarations

```text
pub struct Point { x: float = 0.0, y: float = 0.0 }
pub opaque type MissionId = u64
pub enum MissionKind: u8 { Medical = 1, Power = 2 }
pub bitflags Flags: u16 { Active = 1, Priority = 8 }
pub repr(C) packed struct Packet { id: MissionId, flags: Flags }
```

Ordinary structs are immutable-shaped values and cannot be attached as ECS
components. `opaque type`, fixed-representation `enum`, `bitflags`, and
`repr(C)` structs are represented by the `NativeType` declaration node and obey
the nominal/layout rules in §2.6.

<a id="decl-entities"></a>
### 3.3 Entity declarations

```text
pub entity control_room {
    Point { x: 1.0, y: 2.0 },
    Health { hp: 100 },
}
```

An entity declaration creates one named entity during program initialization.
Its component entries are validated against component schemas. Named identity
participates in `$entity_names`/`$entity_identity`, snapshots, and replay.

<a id="decl-sums-aliases"></a>
### 3.4 Sum types and aliases

```text
pub type Outcome {
    Assigned { mission: MissionId, asset: AssetId }
    Rejected { mission: MissionId, reason: str }
}

pub type Pair<T> = (T, T)
pub type Buckets<K, V> = map<K, list<V>>
```

A sum variant is nominal and constructed as `Outcome::Assigned { ... }`.
Variant fields are type declarations, not component defaults. A type alias is
transparent and may be parameterized; substitution is recursive through its
complete type expression.

<a id="decl-states"></a>
### 3.5 State machines

```text
pub state MissionFlow {
    Queued { on reserve -> Reserved }
    Reserved {
        on launch -> Active
        on cancel -> Cancelled when cancellation_allowed()
    }
    Active { on complete -> Completed }
    Completed {}
    Cancelled {}
}
```

State names and transition names are local to the machine. A transition is
valid only from its declared source and only when its guard is true.
`transition(value, "name")` returns an `Option` containing the next state; the
caller explicitly stages that value.

<a id="decl-functions"></a>
### 3.6 Functions and transactions

```ebnf
function_decl = [ "pub" ] [ "pure" | "readonly" | "async" | effect { effect } ]
                "fn" IDENT [ "<" IDENT { "," IDENT } ">" ]
                "(" [ parameter { "," parameter } ] ")"
                [ "->" type_expr ] [ ownership_grant ] block ;
parameter     = [ "mut" ] IDENT [ ":" type_expr ] ;
effect        = "io" | "ecs" | "event" | "readonly" ;
ownership_grant = "writes" "owned" "[" ownership_target { "," ownership_target } "]" ;
```

`pure`, `readonly`, and `async` are mutually meaningful effect declarations.
Ordinary effects are inferred transitively and checked against restricted
callers. A `mut` parameter permits rebinding that parameter; it does not grant
component ownership.

Stable atomic transactions are named function declarations:

```text
pub transaction Reserve(subject: entity)
writes owned [Mission, Capacity]
{
    requires has(subject, Mission)
    changes_only [Mission, Capacity, ActiveMission]

    set(subject, ActiveMission {})

    ensures has(subject, ActiveMission)
    post_commit { emit next MissionReserved { mission: subject } }
}
```

A transaction requires at least one `requires`, exactly one nonempty
`changes_only`, and at least one `ensures`. It stages one patch against one base
snapshot and commits all or none. `post_commit`, when present, is final and may
emit/perform allowed external effects but cannot mutate authoritative state.
Transactions are not generic or async and return `nil`.

<a id="decl-systems"></a>
### 3.7 Systems and authority signatures

```text
system Route(
    mission: mut Mission,
    available: AvailableAsset,
    ledger: accum Ledger,
    reads AuditTrail,
    writes Projection,
    emits MissionRouted,
    io true,
    async false,
) after Intake before Publish {
    // `self` is the current entity for a component query row.
}
```

Component parameters determine query cardinality. A mutable component/resource
parameter grants writeback; `accum` values merge through the accumulator
contract. Type-only filters constrain membership without binding a value.
Authority-only `reads`/`writes`/`emits` entries do not alter cardinality and may
be single names, qualified fields, a bracketed list, a quoted synthetic
authority, or `*` where a language rule explicitly allows it. `io` and `async`
are opt-in booleans.

The declared signature is an enforced upper bound on call-site-specialized
synchronous effects. `after` and `before` add scheduler edges; cycles are
errors. Scheduler conflicts consume the same specialized graph.

<a id="callable-contracts"></a>
### 3.8 Callable contracts

Contracts prefix systems or handlers and apply transitively:

| Contract | Meaning |
|---|---|
| `@frame`, `@tick`, `@render` | Marks a latency-sensitive root. |
| `@no_full_scan` | Rejects reachable population scans. |
| `@allow_full_scan(reason: "...")` | Documents one intentional scan. |
| `@no_guest_allocation` | Rejects RAD-managed guest allocation. |
| `@no_runtime_allocation` | Measures/rejects VM runtime allocation. |
| `@no_host_allocation` | Measures/rejects host-boundary allocation. |
| `@budget(instructions: N)` | Enforces deterministic semantic work. |
| `@no_nested_flush` | Rejects nested `flush_events()`. |
| `@non_reentrant` | Rejects recursive callable entry. |
| `@exactly_once` | Rejects duplicate handler delivery. |
| `@must_complete_before(Phase)` | Rejects delivery after a phase barrier. |

The compiler reports the transitive effect/cost path. A measurement contract
fails if its required native meter is unavailable.

<a id="decl-events"></a>
### 3.9 Events and handlers

```text
pub event MissionReserved { mission: entity, asset: entity }

@non_reentrant
on MissionReserved(evt) when has(evt.mission, Mission) { }

on MaintenanceWindow once (evt) where has(evt.asset, AssetIdentity) { }
async on RemoteReply(evt) { await process(evt) }
```

Event fields are typed. A handler receives one payload binding. `when` and
`where` are the guard positions. `once` runs the handler at most once for the
VM lifetime; `@exactly_once` additionally diagnoses a duplicate delivery.
Handlers cannot be `pub`.

<a id="decl-phases"></a>
### 3.10 Phases

```text
phase Input { ReadInput }
phase Simulate { Integrate, Resolve }
serial phase Commit { CommitWorld }
phase Publish {}
```

A phase names a system group and may participate in `after`/`before` ordering.
Brace and bracket member forms are equivalent. `serial` forbids parallel
batches inside the phase. A schedule may reference a phase directly or through
an import alias.

<a id="decl-views"></a>
### 3.11 Materialized views

```text
pub materialized view ActiveMissions {
    depends [Mission, ActiveMission]
    key Mission.id
}
```

A view declares its exact source dependencies and optional key projection.
The runtime owns membership, key indexes, revisions, change history, and
provenance; user code cannot mutate a view. Maintenance is atomic with the
source write and is restored/reinstalled when a compatible world is loaded.

<a id="decl-migrations"></a>
### 3.12 Migrations

```text
migrate AuditTrail(old, from_version) {
    return AuditTrail {
        revision: get_or(old, "revision", 0),
        notes: get_or(old, "notes", []),
    }
}
```

Loading invokes the matching migration when persisted shape/version differs.
The old row is data, and the function must return the current declared type.
Migration runs before adoption; any error rejects the load without partial
world mutation.

<a id="decl-modules"></a>
### 3.13 Module imports and visibility

```text
use "schema.rad"
use "owners/grid_owner.rad" as owner
```

Paths resolve relative to the importing file and normalize to one canonical
module identity. Bare imports expose public names in the flat namespace;
aliased imports expose public names as `owner.name`. Multiple aliases,
bare-plus-aliased access, and normalized relative spellings share one semantic
module/runtime identity. Different canonical paths remain different modules
even when their bytes match. Cycles and duplicate flat symbols are diagnosed.

`pub let` exports an immutable, explicitly typed value. Private declarations
cannot be reached through an alias. Source maps preserve the defining file and
span for diagnostics, authority, ownership, fingerprints, and replay.

<a id="decl-tests"></a>
### 3.14 Tests and stateful models

```text
test "isolated workflow" { }
shared test "intentional shared producer" { }
test property for value in gen_int(), flag in gen_bool() { }
test "owned fixture" writes owned [Inventory] { }
```

Tests are isolated by default: each starts from the file's post-initialization
fixture snapshot. Adjacent `shared test` declarations intentionally form one
shared chain; isolation resumes afterward. Property generators execute the
body once per generated tuple. Test ownership grants are scoped to that test
body and cannot be exported.

```text
model Lifecycle {
    commands [enqueue, lease, complete, timeout]
    invariant { return active_lease_count(job) <= 1 }
    temporal {
        always Observable,
        Requested eventually Completed,
        Active until Completed,
        exactly_once CompletionObserved,
        Running never_after Succeeded,
        Requested eventually_within Completed 2,
    }
    runs 10000
    max_commands 200
    seed 1234
}
```

Models run private simulation forks, vary command and flush boundaries, check
invariants after each boundary, observe component/event signals, and produce a
deterministically shrunk replay artifact on failure.

<a id="decl-causal"></a>
### 3.15 Causal declarations (experimental)

```text
intent Adjustment { key target: entity, amount: int }
law Forecast(target: entity) { propose Adjustment { target: target, amount: 5 } }
resolver Resolve for Adjustment(target, proposals) { next(target, Load { value: sum(proposals) }) }
constraint Capacity for Load(subject, proposed) watches Limit {
    require proposed.value <= candidate(subject, Limit).value else "capacity.exceeded"
}
```

`intent`, `law`, `resolver`, and `constraint` require the causal feature flag.
Laws propose immutable intents; one resolver owns candidate writes;
constraints observe one base and complete candidate and may only require or
reject. Their settlement is atomic and distinct from stable transactions.

<a id="decl-top-level"></a>
### 3.16 Top-level statements

Any ordinary statement may be a `Decl::Stmt` and executes in source order
during initialization. A private `fn main() -> nil` is invoked after top-level
initialization. Recovery-only `Decl::Error` is not source syntax.
