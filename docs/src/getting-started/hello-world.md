# Hello World

Create a file called `hello.rad`:

```
print("Hello, Rad!")
```

Run it:

```bash
rad hello.rad
```

Output:

```
Hello, Rad!
```

## A more interesting example

Rad shines when you use its three core features together. Here's a minimal ECS + events example:

```
component Health { hp: 100, max: 100 }
component Name { indexed value: "" }

event Hit { target_name: str, amount: int }

on Hit(e) {
    let target = lookup(Name, "value", e.target_name)?
    let h = get(target, Health)?
    let mut new_hp = h.hp - e.amount
    if new_hp < 0 { new_hp = 0 }
    set(target, Health { hp: new_hp, max: h.max })
}

fn main() -> any {
    let hero = spawn()
    set(hero, Name { value: "Hero" })
    set(hero, Health { hp: 100, max: 100 })

    emit Hit { target_name: "Hero", amount: 30 }
    flush_events()
    let h = get(hero, Health)?
    print("HP remaining:", h.hp)
}
```

Output:

```
HP remaining: 70
```

## What just happened?

1. **`component`** declared a pure data type (`Health`)
2. **`spawn`** / **`set`** created an entity and attached data to it
3. **`event`** + **`on`** defined a message and a handler
4. **`emit`** fired the event, which was then processed by **`flush_events()`**, triggering the handler
5. **`?`** took the `Health` value out of the `Option` returned by `get`, or would have exited `main` early if the component were missing

These are Rad's three laws in action: data separated from logic (ECS), data flowing through pipelines, and communication via events. Each one is covered in depth in the [Language Guide](../guide/three-laws.md).
