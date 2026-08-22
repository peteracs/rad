<a id="statements"></a>
## 4. Statements

A block is `{` followed by zero or more statements and `}`. RAD does not use
semicolons. Statements execute in source order unless control flow transfers.

<a id="stmt-bindings"></a>
### 4.1 Bindings and let-else

```text
let answer = 42
let mut retries: int = 0
let unique mut bytes: list<int> = []
let rec factorial: fn(int) -> int = fn(value: int) -> int { ... }
let (left, right): (int, int) = pair
let [index, value]: (int, int) = entry
let Some { value } = maybe else { return }
```

`let` is immutable unless `mut` is present. `unique` statically prohibits
aliases and enables guaranteed in-place mutation. `rec` makes a closure binding
visible inside its initializer. Tuple/list patterns require exact arity.
Refutable sum patterns require `else`; that block must transfer control away
from the following statements.

<a id="stmt-assignment"></a>
### 4.2 Assignment and writeback

```text
count = count + 1
record.field = value
values[index] = value
matrix[row][column] = value
buckets[key] << value
```

Assignment requires a mutable root binding. Field/index chains are evaluated
once, updated from the leaf outward, and written back to that root. `<<` on an
indexed list bucket appends one element and creates an empty bucket when the map
key is absent. ECS component/resource mutation is never implied by local record
assignment; use `set`, `set_resource`, or `update`.

<a id="stmt-control"></a>
### 4.3 Conditional and loop statements

```text
if ready { run() } else if waiting { poll() } else { stop() }

while retries < 3 {
    retries = retries + 1
}

for item in items where item.active {
    if item.skip { continue }
    if item.done { break }
}

for (key, value) in entries(table) { }
for [index, value] in enumerate(items) { }
```

Conditions and `where` filters must be boolean. `for` accepts lists, maps,
strings, and explicit iterator values according to their type contract.
`break` and `continue` target the innermost loop. A loop binding is freshly
scoped for each iteration.

<a id="stmt-return"></a>
### 4.4 Return and expression statements

`return expression` leaves the current callable. `return` returns `nil`.
Every path of a function with a concrete non-`nil` return type must return a
compatible value. Any expression may be a statement; its result is discarded
after effects and errors are evaluated.

<a id="stmt-events"></a>
### 4.5 Event emission

```text
emit EventName { field: value }
emit next EventName { field: value }
emit phase(Resolve) EventName { field: value }
emit next EventName { field: value } after 2
signal sync EventName { field: value }
```

Bare `emit` and `emit next` enqueue for the next event buffer. `phase(P)` queues
for the named not-yet-completed phase. `after N` is valid only with next-buffer
delivery and waits `N` flush cycles. `signal sync` invokes handlers before the
statement returns and may therefore be reentrant. Payload fields are complete,
typed, and evaluated before enqueue/delivery.

<a id="stmt-schedule"></a>
### 4.6 Scheduling

```text
LocalSystem()
schedule [InputPhase, local.System, system::module::Qualified]
schedule serial [A, B, C]
```

A direct system call executes that system. `schedule` expands phases, validates
ordering edges, topologically orders systems, forms conflict-free batches, and
merges worker results deterministically. `serial` executes one system at a time
in topological order.

<a id="stmt-update"></a>
### 4.7 Component and resource update

```text
update(subject, Health) {
    hp = 90,
    resistances["fire"] = 25,
}

update(GlobalClock) { tick = 42 }
```

The entity expression is evaluated once. The checker resolves every field and
nested index type, ownership grant, and system authority. The resulting record
is one component/resource write. Updating an indexed field updates its index in
the same authoritative operation.

<a id="stmt-match"></a>
### 4.8 Match statements

```text
match outcome {
    Assigned { mission, asset } => { publish(mission, asset) }
    Rejected { reason } when len(reason) > 0 => { audit(reason) }
    Rejected { reason: _reason } => { }
}

match subject {
    has Health(component) => { print(component.hp) }
    0 => { print("zero") }
    _ => { print("other") }
}
```

Patterns and guards are evaluated top to bottom. Sum/state matches must be
exhaustive through unguarded arms. Primitive/entity matches require a wildcard
unless another rule proves complete coverage. Named variant fields support
shorthand and explicit `field: binding`. Built-in sums use the same canonical
named-field form: `Some { value }`, `Ok { value }`, `Err { error }`, and
`None {}`.

<a id="stmt-transaction"></a>
### 4.9 Transaction region

The `Transaction` statement node is the body of a named `transaction`
declaration (Section 3.6). Its source sections are `requires`, `changes_only`, staged
ordinary statements, `ensures`, and final `post_commit`. It is not a free-form
anonymous block: every transaction has a stable name and declared contract.

<a id="stmt-causal"></a>
### 4.10 Causal settlement statements (experimental)

```text
settle {
    ForecastDemand(target)
    ReserveMargin(target)
}

propose Adjustment { target: target, amount: 5 }
next(target, Load { value: proposed_value })
require proposed.value <= limit else "capacity.exceeded"
```

`settle` gathers law proposals and resolves one candidate. `propose` is legal in
a law, `next` in the matching resolver, and constraint `require ... else CODE`
in a candidate constraint. These statements are feature-gated and cannot be
used to bypass the stable transaction region.

`OnceGuardPass` and `Error` are compiler/parser internal statement nodes and
have no source spelling.
