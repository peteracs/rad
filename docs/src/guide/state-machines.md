# State Machines

Rad has first-class state machine syntax. Define states and transitions declaratively, and the runtime enforces them at runtime.

## Declaring a state machine

```
state DoorState {
    Locked { on unlock -> Closed }
    Closed { on open -> Open, on lock -> Locked }
    Open   { on close -> Closed }
}
```

Each state lists the transitions it accepts: `on <event> -> <target state>`.
Transitions can be separated by newlines, optional commas, or both.

## Using state machines

Create a state value and transition it:

```
let mut door = DoorState::Locked

door = transition(door, "unlock")?
print(door)   // Closed

door = transition(door, "open")?
print(door)   // Open
```

You can also use `match` as an expression:

```
let label = match door {
    Locked => { "locked" }
    Closed => { "closed" }
    Open => { "open" }
}
print(label)
```

If you only need to check if a state machine is currently in a specific state, you can use the `is` operator, which evaluates to a boolean:

```
if door is Locked {
    print("Cannot open, the door is locked!")
}
```

`transition` returns `Ok(new_state)` if the transition is valid, or `Err(message)` if it isn't. Use postfix `?` in a function that can propagate `Result` (for example `fn main() -> any`), or check manually:

```
let result = transition(door, "fly")
if result is Err {
    print("invalid transition")
}
```

## State machines + ECS

Attach a state machine as a component to an entity:

```
component DoorComponent { state: DoorState::Locked }

let door_entity = spawn()
set(door_entity, DoorComponent { state: DoorState::Locked })
```

## Example: traffic light

```
state TrafficLight {
    Red    { on timer -> Green }
    Green  { on timer -> Yellow }
    Yellow { on timer -> Red }
}

let mut light = TrafficLight::Red
let cycle = ["timer", "timer", "timer", "timer"]

let mut i = 0
while i < len(cycle) {
    light = transition(light, cycle[i])?
    print(light)
    i = i + 1
}
```
