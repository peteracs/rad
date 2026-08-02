# Events

Events are Rad's communication mechanism. Instead of direct function calls between systems, you **emit** events and **handle** them with `on` blocks. Producers and consumers are fully decoupled.

## Declaring events

An event is a named data bag, just like a component:

```
component Health { hp: 100, max: 100 }
component Name { indexed value: "" }

event Hit { target_id: str, amount: int }
event Defeated { name: str }
event GameOver { }

let hero = spawn()
set(hero, Health { hp: 100, max: 100 })
set(hero, Name { value: "Hero" })
```

Event fields can optionally include **type annotations** for documentation and static checking:

```
event Hit { target_id: str, amount: int }
event Defeated { name: str }
```

When types are declared, the Rust VM checker validates that `emit` statements provide values matching the declared types. `pub` events require explicit types on every field.

> **Best Practice:** Notice how the `Hit` event uses `target_id: str` instead of a raw `entity` ID. Events should use **Domain-Driven IDs** and resolve them via `lookup()`. Passing raw ECS entity IDs in events couples your event bus to memory layout and breaks if the world is forked or serialized.

## Emitting events

Use `emit` to fire an event:

```
emit Hit { target_id: "Hero", amount: 30 }
```

### Delayed delivery: `after`

`emit E { .. } after N` queues the event to fire after N event-flush
cycles (game ticks) instead of on the next flush — timers live in the
event queue, not in hand-rolled countdown fields:

```
// the cast point lands 8 ticks from now
emit CastFired { caster: caster, slot: "q" } after def.cast_ticks
```

Delivery is deterministic: delays age one tick per flush, and events
landing on the same tick fire in emit order. Handlers can re-arm
follow-ups (`emit Chain { hop: hop + 1 } after 2`), and a stale timer is
harmless when its handler guards on current state — the spellwork cast
pipeline interrupts a channel by transitioning the state machine and
simply lets the pending `ChannelDone` fire into a guard that no longer
matches.

Delayed emits are not allowed inside a parallel system batch. If a system needs
to arm a timer, run that system by itself in the schedule or emit an immediate
event and let a handler queue the delayed follow-up.

### Querying the past: `recent_events`

Every dispatched event lands in the deterministic event log (main
timeline, ring-capped). `recent_events(name, window)` returns the
payloads from the last `window` ticks, oldest first — windowed views of
history without hand-rolled ring buffers:

```
// the death recap: damage on the victim in the last 15s,
// grouped per source, ranked by contribution
let hits = recent_events("Damage", 450)
    |> filter(fn(d) { return d.target == victim })
let by_source = hits |> group_by(fn(d) { return name_of(d.source) })
```

`window` counts back from the current tick inclusive (`0` = this tick
only). The log is observational — reading it never changes dispatch —
and carries the `readonly` effect like every world read.

## Handling events

Use `on` to register a handler:

```
on Hit(e) {
    let target = lookup(Name, "value", e.target_id)?
    let h = get(target, Health)?
    let mut new_hp = h.hp - e.amount
    if new_hp < 0 { new_hp = 0 }
    set(target, Health { hp: new_hp, max: h.max })
}
```

The handler receives the event data as its parameter (`e`). Access fields like any other value: `e.target_id`, `e.amount`.

## Guarded handlers

Use `where` or `when` to add a guard expression. The handler body only runs when the guard is truthy:

```
on Hit(e) where e.amount > 10 {
    print("heavy hit:", e.amount)
}

on Hit(e) when e.amount <= 10 {
    print("light hit:", e.amount)
}
```

`where` and `when` are interchangeable — use whichever reads more naturally. The guard is desugared to an `if` wrapper at parse time, so the checker and runtime handle ordinary handlers transparently.

## `once` handlers with guards

`on … once (…) where …` / `when …` retires the handler **only after** an emission where the guard is truthy and the body runs. If the guard is false, the body does not run and the `once` handler remains registered — it can still fire on a later emission when the guard passes.

Handlers that are only `once` (no guard) still run at most once total, as soon as the event is dispatched to them the first time.

## Chaining events and Double-Buffering

Rad uses a **strict double-buffered event architecture** (Data-Oriented Design). When you emit an event, it is not processed immediately. Instead, it is pushed to the next frame's queue.

Events are only processed when the current frame ends (at the end of a `schedule` block) or when you explicitly call `flush_events()`.

This means handlers can emit further events, creating event chains, without any risk of stack overflows or infinite loops:

```
on Hit(e) {
    let h = get(e.target, Health)?
    let mut new_hp = h.hp - e.amount
    if new_hp < 0 { new_hp = 0 }
    set(e.target, Health { hp: new_hp, max: h.max })
    if new_hp == 0 {
        emit Defeated { name: (get(e.target, Name)?).value }
    }
}

on Defeated(e) {
    print(e.name, "has been defeated!")
}
```

If you are writing a simple script without a `schedule` block, you must explicitly call `flush_events()` to process the events you emitted:

```
emit Hit { target: hero, amount: 30 }
flush_events() // Processes Hit, which may emit Defeated
flush_events() // Processes Defeated
```

## Events and speculative execution

When systems run inside `simulate()`, event emissions dispatch inside the fork's own event queue. The live event queue and delayed-event timeline are untouched, so event-driven combat, damage cascades, and cleanup handlers can be simulated without rewriting them as direct function calls.

The type checker walks every handler reachable from events emitted by a simulated system. It rejects IO, `commit()`, `transition`, and unsafe event-effect function calls anywhere in that handler chain.

```rad
event Hit { target: entity, amount: 0 }

system PredictAttack(target: Health) {
    emit Hit { target: self, amount: 3 }
}

on Hit(e) {
    update(e.target, Health) {
        hp = hp - e.amount
    }
}

let future = simulate(fork(), [system::PredictAttack], 1)
```

When `commit(fork)` replaces the live world, **all pending events in the main timeline are discarded**, since they reference pre-commit state that no longer exists.

Global `resource` singletons are included in the forked world snapshot alongside entity-component data. Simulated systems can read and mutate resources within the fork without affecting the main world until `commit`.

## Why events over function calls?

- Handlers don't know who emitted. Emitters don't know who handles.
- Adding new behavior means adding a handler, not modifying existing code.
- Event chains express complex interactions without tangled call graphs.
