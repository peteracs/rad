<a id="execution-model"></a>
## 7. Execution model

<a id="execution-program"></a>
### 7.1 Program initialization and entry

A loaded program is one canonical module graph. Module declarations are
registered before checking bodies, then top-level statements execute in source
order. If present, private `fn main() -> nil` runs after initialization. Public
entry points require explicit parameter and return types in strict mode.

The runtime owns the world: entities, components, resources, indexes, views,
events, timers, provenance, transaction metadata, tasks, and replay state.
Application code cannot replace those stores or retain a mutable reference into
them. Values crossing the stack/world boundary retain value semantics.

<a id="execution-systems"></a>
### 7.2 Systems, phases, and deterministic scheduling

A system query is derived only from bound component/resource parameters and
type-only filters. Authority-only `reads`, `writes`, and `emits` entries do not
change cardinality. The scheduler expands phases, adds `after`/`before` edges,
rejects cycles, and creates batches from call-site-specialized synchronous
effects. Systems whose effects conflict do not share a batch.

Each parallel system runs against the same batch base. Worker patches are
adopted in deterministic schedule order. Accumulator parameters use their
declared merge rule. Conflicting ordinary writes are prevented by batching;
conflicts discovered at adoption are runtime failures rather than last-writer
wins. `schedule serial` and `serial phase` disable batching for their scope.

Determinism covers semantic ordering and final state for one program, input,
seed, runtime feature set, host record, and worker-count-independent schedule.
It does not make unrecorded clock, random, network, filesystem, or native-host
behavior deterministic.

<a id="execution-events"></a>
### 7.3 Event delivery and reentrancy

RAD has distinct delivery boundaries:

| Form | Delivery |
|---|---|
| `emit E { ... }` / `emit next E { ... }` | Next event buffer. |
| `emit next E { ... } after N` | After `N` flush cycles. |
| `emit phase(P) E { ... }` | When phase `P` is entered. |
| `signal sync E { ... }` | Current call stack before the statement returns. |

`flush_events()` swaps buffers and drains the selected handlers. Events emitted
while draining normally enter the following buffer. Calling `flush_events()`
inside a handler creates explicit nested delivery; `@no_nested_flush` and
`@non_reentrant` make that forbidden where required. The synchronous authority
graph stops at queued delivery boundaries and follows synchronous signals and
explicit flushes.

Handler ordering is declaration order after canonical module ordering. A
payload is immutable after emission. Exactly-once and phase-barrier contracts
are checked at compile time where reachability proves a violation and at
runtime for duplicate/dynamic delivery.

<a id="execution-transactions"></a>
### 7.4 Stable transactions

A transaction evaluates `requires` against one immutable base snapshot. Its
body stages component/resource writes, spawn, remove, state transition,
despawn, and other permitted authoritative operations in a private journal.
`changes_only` is checked transitively. `ensures` observes the complete staged
candidate. Any failure discards the journal and leaves the base unchanged.

Successful adoption is one atomic world revision and one provenance cause.
Observers cannot see intermediate rows. `post_commit` executes only after
adoption and cannot extend or replace the committed patch. Event, IO, and async
effects that must happen after state adoption belong there. A failed
post-commit effect does not retroactively undo the already committed state and
is represented as its own typed failure/cause.

Transactions cannot nest and cannot commit or merge speculative forks. The
experimental settlement kernel is a separate region with separate candidate
and resolver rules.

<a id="execution-speculation"></a>
### 7.5 Forks, simulation, sandbox, and merge

`fork()` creates a copy-on-write world snapshot including execution-relevant
event/runtime state. Simulation executes on an isolated fork; no output, host
effect, live-world write, task, timer, or provenance node escapes unless an
explicit operation adopts an allowed result. `simulate_many` and
`simulate_seeded` derive deterministic per-candidate seeds.

`commit()` adopts one fork after base/delta validation. `merge_forks` requires
compatible bases and resolves only the declared merge domain. `diff` and
`assert_only_changed` compare semantic world changes, not host side effects.

`sandbox_run` adds read/write/call/fuel/memory capability checks. Guest source,
input, output, and errors cross data-only serialization boundaries. A denied or
malformed operation fails closed; no partial guest state reaches the host.

<a id="execution-persistence"></a>
### 7.6 Snapshots, migration, recording, and replay

World snapshots encode schema identity, declared versions, runtime-owned world
state, indexes/views required for restoration, and deterministic metadata.
Transient resources, tasks, sockets, file handles, and opaque native handles
are not persistent values. Loading validates the whole payload, runs required
migrations in isolation, reinstalls declared indexes/views, and adopts all or
nothing.

Record/replay binds input to program and semantic-configuration fingerprints.
Recorded host calls include the identity needed to prevent a replay from
silently invoking live external state. A successful replay consumes the exact
record, reproduces ordered externally visible events, and ends at the same
world digest. Extra, missing, or mismatched records are errors.

<a id="execution-async"></a>
### 7.7 Tasks and async execution

`async call(...)` creates a task; `await` yields its value or propagates its
failure. Tasks are runtime-owned, process-local values and cannot be persisted.
Async reachability is included in authority and transaction checks. Scheduling
order remains deterministic only for effects represented through RAD's ordered
state/event/record boundaries; arbitrary host completion timing is not a state
ordering contract.

<a id="execution-views"></a>
### 7.8 Indexes and materialized views

Hash and ordered indexes are maintained in the same authoritative operation as
their source component. Ordered traversal follows the declared native key
ordering, deterministic entity-ID tie-breaking, and explicit insert/replace/
erase semantics. Mutating membership invalidates a live traversal where the API
contract says so; snapshot-returning APIs remain stable.

Materialized views are transactionally maintained from their exact dependency
set. A dependency write may change membership, key publication, revision,
change history, and provenance in the same commit. A nondependency write does
not move that view revision. Readers never rebuild the view.

<a id="execution-models"></a>
### 7.9 Stateful models and temporal checking

A model trial starts from an isolated fixture world and applies generated
commands to one evolving private world. Invariants run after each command and
flush boundary. Temporal clauses observe component membership and captured
events: `always`, `eventually`, `until`, `exactly_once`, `never_after`, and
bounded `eventually_within`.

On failure the runner deterministically shrinks command history while
preserving the failed property and emits a replayable `.radr` artifact bound to
the same semantic product. Model execution cannot publish trial state or event
logs into the production world.

<a id="execution-relations"></a>
### 7.10 Experimental relations and settlements

Relation schemas declare tuple shape, key/unique constraints, symmetry, and
delete behavior. Facts are changed only by relation operations permitted in a
resolver candidate. Derived rules and aggregates evaluate from one base plus
candidate relation state; constraint rejection discards every proposed fact
and component change. Relation manifests and rule identities participate in
verification and replay fingerprints.
