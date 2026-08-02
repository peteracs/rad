# The Three Laws of Great Software Architecture
## Why We Built a Language Around Them

Large codebases that stay workable — games, distributed systems, data pipelines — tend to converge on the same three patterns, and they converge from very different starting points. Separate data from logic. Move data through pipelines. Let components talk via events.

In most languages these stay conventions: you can follow them, but the compiler has no idea whether you did. So we asked the obvious question. What if they weren't patterns you _apply_ to a language, but properties the language could actually check?

That's Rad. The three laws below describe the design goal; each section ends with a precise account of how much of it the compiler really enforces, because the gap between the two is the part worth being honest about.

---

## Law 1: Separate Data from Logic

**The Problem:** Object-Oriented Programming bundles data and behavior together. A `Player` class has `hp`, `position`, `inventory`, AND the methods to modify all of them. Change the combat system and you break the inventory. Change the movement code and you break the rendering. Everything touches everything.

**The Pattern:** Entity Component System (ECS). Entities are just IDs. Components are pure data. Systems are pure logic that operates on components. Nothing is coupled.

```
component Position { x: 0.0, y: 0.0 }
component Velocity { dx: 1.0, dy: 0.5 }

system Physics(pos: mut Position, vel: Velocity) {
    pos.x = pos.x + vel.dx
    pos.y = pos.y + vel.dy
}
```

The signature is the contract. `Physics` declares that it reads `Velocity` and writes `Position`, so the blast radius of a change to it is written at the top of the block instead of discovered by grepping. That declaration is load-bearing: the scheduler reads it to order systems and to run non-conflicting ones in parallel, treating `mut` parameters as writes and everything else as reads (see [System scheduling](../reference/spec.md#72-system-scheduling)).

Be precise about what the signature is, though. It is a **declared query**, not a sandbox. It selects the entities the system iterates and the components written back when it returns — but the body can still call `get`/`set` on any component type through the general ECS API. Compilation still rejects none of that; what exists today is an opt-in lint: `rad lint --preset=strict` (or `enterprise`) warns when a system body directly reads (`RAD-L016`) or writes (`RAD-L015`) a component or resource type that is not in its signature. "Directly" is a real limitation — it catches the ECS builtins, `update` sugar, component literals, and `query { }` expressions written in the body itself, but it does not follow calls into helper functions, so an access buried in a `fn` the system calls still gets through. A system that reaches outside its signature is also stepping outside what the scheduler's conflict analysis knows about, which is the practical reason to keep systems honest.

**In Rad:** ECS isn't a library, it's part of the language. There is no `class` keyword, and you genuinely cannot bundle data with behavior — `component` and `struct` fields are restricted to plain data, and the checker rejects any field whose type is a function or closure.

What Rad does *not* claim is that ECS is the only way to write anything. You also get plain `fn` functions, closures, `struct` records, sum types, state machines, and top-level statements, and plenty of good Rad programs are mostly those. The narrower, real property is about **shared world state**: it lives in components on entities or in globals declared with the `resource` keyword, and nowhere else. There is no implicit mutable global, so two systems that share no component or resource type cannot interfere with each other.

---

## Law 2: Move Data Through Pipelines

**The Problem:** Shared mutable state is hard to reason about. When data changes in place and several references point at the same object, no single piece of code owns the current value, and answering "who set this, and when?" means reconstructing an aliasing graph at runtime.

**The Pattern:** Pipeline Pattern. Data flows through a chain of transformations. Each step produces a new value rather than editing its input. The output of one step is the input to the next.

```
let result = scores
    |> filter(fn(s) { return s > 0 })
    |> map(fn(s) { return s * 2 })
    |> reduce(0, fn(a, b) { return a + b })
```

Every intermediate value is a new list. The original `scores` is never modified. You can read the pipeline top-to-bottom and understand the transformation without tracking any state.

**In Rad:** The `|>` pipe operator is a first-class language feature, and the checker keeps pipeline stages clean: side-effecting builtins (`set`, `spawn`, `despawn`, `remove`, `set_resource`, `transition`, `flush_events`) and `emit` are **rejected at compile time** in `|>` position. Only pure functions and `readonly` ones — the ECS reads like `get`, `has`, `entities`, `lookup` — may appear there. Two further guarantees hold everywhere, not just in pipelines, and both are enforced by the checker:

- **Immutable by default.** A `let` binding cannot be reassigned; you must write `let mut` to opt in. Writing into an element of an immutable container is a compile error too.
- **Value semantics.** Every assignment, argument pass, and return produces an independent copy, so two variables never alias the same mutable state and no function can modify a value you hand it.

**One honest caveat.** Rad is immutable *by default*; it is not a language where nothing is mutated. Law 1's `Physics` system assigns to `pos.x` directly, and that write is real — a system parameter marked `mut` is written back to the component when the system returns. Mutation is a supported tool here.

The property worth defending is narrower and more useful than "nothing changes": **every mutation is declared at the point that permits it.** You write `mut` on the binding, or `mut` on the system parameter, and nowhere else can the value change. Combined with value semantics, that means you find every writer by reading declarations rather than by chasing references at runtime — which is the actual debugging cost the pattern is meant to remove.

---

## Law 3: Components Talk Via Events

**The Problem:** Direct function calls create coupling. If module A calls module B, A depends on B's interface. Change B and A breaks. Now multiply this by every module calling every other module. The dependency graph becomes a hairball.

**The Pattern:** Event-Driven Architecture. Modules don't call each other. They emit events. Other modules subscribe to events they care about. The emitter doesn't know who's listening. The listener doesn't know who emitted.

```
event Hit { target_name: str, amount: int }

on Hit(e) {
    let target = lookup(Name, "value", e.target_name)?
    let h = get(target, Health)?
    let mut new_hp = h.hp - e.amount
    if new_hp < 0 { new_hp = 0 }
    set(target, Health { hp: new_hp, max: h.max })
}
```

Adding a particle effect on hit? Add a handler. Removing the sound system? Remove a handler. Nothing else changes. The blast radius is zero.

**In Rad:** `emit` and `on` are keywords. Events are declared at the top level and handlers are registered automatically. Event queues are **double-buffered**: `emit` appends to the *next* flush's queue, and `flush_events()` swaps the queues before it starts dispatching. An event emitted by a handler is therefore never delivered during the flush that is already running (see [Event ordering](../reference/spec.md#73-event-ordering)).

Be precise about what that does and doesn't buy you.

**It does eliminate implicit re-entrancy.** A handler cannot re-enter itself, or any other handler, through `emit` alone. An `emit` chain — `A` emits `B`, `B` emits `A` — advances exactly one hop per flush instead of recursing, so a chain of events cannot grow the call stack no matter how long it runs.

**It does not guarantee termination.** Two things still loop:

- A handler may call `flush_events()` itself. That starts a genuinely nested dispatch — the runtime is written to *survive* re-entrancy, not to forbid it — and a handler that flushes without a base case will overflow the stack and abort the process.
- A driver loop that keeps flushing two handlers which emit each other runs forever, exactly like any other `while` loop with no exit condition.

Rad only bounds this where it has a budget to enforce: guest code running in the sandbox is fuel-limited, so a runaway event chain there is terminated instead of hanging. Ordinary programs get no such backstop. Double buffering makes event flow *predictable and stack-safe*; giving your event loop a termination condition is still your job.

---

## Bonus Law: Make Illegal States Unrepresentable

Finite State Machines are the oldest and most reliable pattern for managing control flow. A door is Locked, Closed, or Open. A connection is Connecting, Connected, or Disconnected. There is no fourth state. The machine enforces this.

```
state DoorState {
    Locked { on unlock -> Closed }
    Closed { on open -> Open, on lock -> Locked }
    Open   { on close -> Closed }
}

match door {
    Locked => { print("locked") }
    Closed => { print("closed") }
    Open   => { print("open") }
}
```

The `match` must be exhaustive, and this is checked at **compile time** by the type checker — not caught at runtime. Drop the `Open` arm and the program never starts:

```text
Error: Non-exhaustive match: state 'Open' of machine 'DoorState' is not covered
  --> door.rad:9:5
   |
>>  9 |     match door {
           ^
hint: Add a case: Open => { ... }
```

The check applies when the subject's type is statically known and the match has no `_` arm; a wildcard is an explicit opt-out. For open sets that cannot be enumerated — `str`, `int`, `float`, `bool` — the checker inverts the rule and *requires* a wildcard instead.

This is the stronger version of the guarantee, and it is what kills the bug class where code assumes "it can only be A or B" and then C happens: add a fourth state to the machine and every match that ignores it stops compiling, so the compiler hands you the list of places to update.

---

## Why a Language?

You can apply these patterns in any language. Bevy does ECS in Rust. RxJS does pipelines in JavaScript. Redux does events in React. But they are all libraries negotiating with a host language that has never heard of them, which means the host cannot check whether you actually followed the pattern. Conformance is a code-review problem.

Rad puts the patterns in the grammar so the compiler can check them. That is a real difference, and it is worth stating without inflating it: **Rad does not make bad architecture unwritable.** You can write a plain imperative Rad program — top-level statements, a bare `for` loop, direct `set()` calls in an event handler — and several programs in this repository do exactly that, deliberately. Top-level statements are part of the execution model, not a loophole.

What changes is the cost curve, and what moves from "style opinion" to "compile error." Every item below is rejected by the type checker before the program runs — these are the diagnostics you actually get:

- Reassigning an immutable binding, or writing into an immutable container — *"Cannot assign to immutable variable 'x'"*
- Bundling behavior into data, via a function- or closure-typed `component` or `struct` field — *"Component field 'Handler.cb' cannot have a function type. Components must be plain data (Law 1: Separate Data from Logic)"*
- A side-effecting builtin or `emit` in a pipeline stage — *"Cannot call impure builtin 'set' inside a pipeline"*
- A non-exhaustive `match` on a state machine or sum type — *"Non-exhaustive match: state 'Open' of machine 'DoorState' is not covered"*
- Aliasing a `let unique` binding — *"Cannot alias unique binding 'xs' into 'ys'"*
- IO, `commit()`, or an unsafe transitive handler chain inside `simulate()` — *"Systems used in simulate() must not perform IO — directly or in any handler reachable through their emits"*
- A circular `after` / `before` dependency between scheduled systems — *"Circular system dependency detected"*

And these hold for any program that compiles:

- **No aliasing.** Value semantics make every assignment, argument, and return an independent copy.
- **No hidden global state.** Components and `resource` globals are the only shared mutable state, and both are declared.
- **Visible blast radius.** A system's parameter list states its intended reads and writes, which is what lets the scheduler parallelize systems that don't conflict.
- **Illegal states are unrepresentable** for the states and sum types you declare — the exhaustiveness check keeps every `match` honest as those types grow.

So the claim is not that every Rad program has the same architecture. It's that the architectural path is the shortest one, and that a specific, enumerable list of architectural mistakes fails to compile instead of failing in production.

---

**Try it now:** [Rad Playground](https://peteracs.github.io/rad/) — runs the Rust VM as WebAssembly in your browser.

**Source / spec:** [github.com/peteracs/rad](https://github.com/peteracs/rad) · [Language specification](../reference/spec.md) · [Language guarantees](../reference/guarantees.md)

**Implementation:** Rust bytecode VM and `rad` CLI (`rad-vm` crate) are the primary implementation and runtime. The historical Rad-to-C compiler under `core/c-backend/` is frozen legacy code, not part of the shipping language contract. An early Python prototype informed the design but is not part of the shipping toolchain.
