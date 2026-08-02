# Playground

The browser playground runs the full Rust VM compiled to WebAssembly. No install needed — just open the link and type.

**[Open the Playground](https://peteracs.github.io/rad/)**

## What you can try

Paste any of these snippets and hit Run:

**Pipeline flow:**

```
let total = [10, -5, 20, 0, 15, -3, 30]
    |> filter(fn(s) { return s > 0 })
    |> map(fn(s) { return s * 2 })
    |> reduce(0, fn(a, b) { return a + b })

print(total)
```

**ECS system:**

```
component Position { x: 0.0, y: 0.0 }
component Velocity { dx: 1.0, dy: 0.5 }

system Physics(pos: mut Position, vel: Velocity) {
    pos.x = pos.x + vel.dx
    pos.y = pos.y + vel.dy
}
```

**State machine:**

```
state DoorState {
    Locked { on unlock -> Closed }
    Closed { on open -> Open, on lock -> Locked }
    Open   { on close -> Closed }
}

let mut door = DoorState::Locked
door = transition(door, "unlock")?
print(door)
```

## Local playground

You can also launch the playground from a Forge project:

```bash
rad play
```
