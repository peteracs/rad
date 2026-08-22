<a id="effects"></a>
## 8. Effects, authority, cost, and errors

<a id="effect-kinds"></a>
### 8.1 Effect kinds

Every callable has a transitive semantic summary:

```text
reads, writes, emits, io, async, allocation, query cost, unknown calls
```

The graph follows ordinary calls, imported helpers, statically bounded
callbacks, captures, transitions, synchronous signals, and handlers reached by
an explicit flush. Queued event/schedule boundaries remain visible in impact
reports but do not borrow the emitter's synchronous authority. Generic helper
reports may conservatively union callback targets; system-root enforcement and
scheduling specialize the graph for each statically resolved callback tuple.

`rad effects`, `rad readers`, `rad writers`, and `rad path` expose this same
compiler-owned graph. They are not separate source scanners.

<a id="effect-purity"></a>
### 8.2 Pure, readonly, and effectful functions

`pure fn` cannot read or mutate shared state, emit, perform IO, start async
execution, or reach an unknown callable. `readonly fn` may read declared ECS,
resource, identity-index, view, and relation state but cannot mutate or perform
external effects. An ordinary function receives its inferred effects.

Purity is part of function-value types. A pure callback satisfies pure,
readonly, or unrestricted parameters. A readonly callback satisfies readonly
or unrestricted parameters. An unrestricted/effectful callback does not
satisfy pure or readonly parameters. Calls through function values are checked
with the same rule, preventing higher-order effect laundering.

<a id="effect-authority"></a>
### 8.3 Enforced system upper bounds

A system's bound query parameters and explicit authority clauses form an upper
bound on its specialized synchronous effects:

```text
system Dispatch(
    mission: Mission,
    reads [Asset, "$entity_names"],
    writes [Mission, DispatchLedger],
    emits MissionAssigned,
    io false,
    async false,
) { }
```

Every inferred state read/write and emitted event must be declared. Reachable IO
requires `io true`; async execution requires `async true`; an unknown or
unbounded call is rejected. Diagnostics include the exact transitive path and
the missing grant. Synthetic authorities such as `$entity_names` and
`$entity_identity` model narrow runtime indexes rather than pretending indexed
lookups are stateless.

Authority clauses do not grant semantic ownership. They say what the system may
exercise; ownership says which module may ever acquire a write capability.

<a id="effect-ownership"></a>
### 8.4 Component, resource, and field ownership

An owned component/resource has one canonical owner module and only explicit
canonical co-owners. An owned field applies the same rule to that field while
other fields retain their declared policy. Direct `set`, `remove`, `update`,
mutable query writeback, typed replacement values, helpers, callbacks, and
transactions all require the capability; moving the write behind another
callable does not conceal it.

`writes owned [...]` is a scoped capability grant on an owner operation or
test. Test grants cannot be imported by production modules. Ownership transfer
is an explicit declaration tied to canonical module identity. Alias spelling
and relative-path spelling do not create another owner.

<a id="effect-regions"></a>
### 8.5 Transaction, post-commit, settlement, and sandbox regions

Opcode and builtin policies are classified once per region:

| Region | Permitted effect shape |
|---|---|
| Transaction body | Staged authoritative writes listed by `changes_only`; no pre-commit emit, IO, or async. |
| Post-commit | Event/IO/async effects; no new authoritative patch. |
| Experimental settlement | Pure/readonly candidate evaluation plus resolver-owned staged facts/components. |
| Sandbox guest | Only effects named by read/write/call and resource limits. |

Entering, leaving, returning, throwing, or aborting a region checks balanced
ownership. A failure discards all private state owned by that region. The inner
frame runner intentionally leaves a settlement alive long enough for its
kernel to classify the failure; the public host boundary enforces final balance.

<a id="effect-cost"></a>
### 8.6 Query-cost and allocation contracts

Cost analysis follows the same transitive/callback-specialized graph as
authority. It distinguishes constant indexed lookup, logarithmic ordered-index
seek, output-linear traversal, population-linear scan, and population
`n log n` sort. `@frame`, `@tick`, and `@render` roots reject full scans unless
the callable declares a nonempty `@allow_full_scan(reason: "...")`.

Allocation contracts are deliberately separate:

| Contract | Metered domain |
|---|---|
| `@no_guest_allocation` | RAD values allocated in the guest GC/value domain. |
| `@no_runtime_allocation` | Native allocations owned by VM execution outside explicit host calls. |
| `@no_host_allocation` | Native allocations while crossing a metered host/FFI operation. |

`@budget(instructions: N)` limits deterministic VM semantic work, not CPU
instructions. Runtime and host allocation contracts require the process
allocation meter; if that evidence is unavailable the contract fails instead
of reporting zero. Setup allocation is not silently subtracted from a
steady-state contract: the measured callable boundary defines the interval.

<a id="effect-determinism"></a>
### 8.7 Determinism and host effects

Clock, random, filesystem, network, and extension calls are observable effects.
Deterministic execution requires a fixed seed or an exact host record. A host
function marked pure/deterministic must return the same validated bytes for the
same input and plugin generation and must not perform undeclared IO, clock,
random, global mutation, or external-state mutation. Replay consumes recorded
results and never invokes the live host operation.

<a id="error-model"></a>
### 8.8 Error model

RAD reports four classes of failure:

1. lexer/parser diagnostics with source span and recovery context;
2. checker diagnostics for types, authority, ownership, cost, visibility,
   exhaustiveness, feature gates, and semantic-product integrity;
3. compile diagnostics for lowering constraints that require checked structure;
4. typed runtime failures carrying stack, region, transaction/settlement,
   replay, host, and provenance context.

No invalid operation silently substitutes a default value. APIs whose absence
is ordinary return `Option`; recoverable domain failure returns `Result`;
contract violations, invalid bytecode, corrupt persistence, and violated host
descriptors fail the operation. A transaction or load failure leaves the live
world unchanged.

`panic(message)` creates a runtime failure. `assert(condition, message)` fails
when the condition is false. `?` propagates only `Option`/`Result` values and
does not intercept unrelated runtime failures.

<a id="error-diagnostics"></a>
### 8.9 Diagnostic contract

A semantic diagnostic identifies the attempted operation, violated rule,
canonical owner/authority/type, transitive path when applicable, and a repair
that uses the intended architectural boundary. Imported diagnostics resolve to
the defining source file. Machine-readable CLI output preserves stable fields
for tools; presentation text is not used as semantic authority.

Parser recovery may create internal `Error` nodes so more diagnostics can be
reported in one pass. A program containing any lexer, parser, checker, or
compiler error is never executable.
