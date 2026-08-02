# Sum Types & Pattern Matching

Rad supports algebraic data types (sum types) with exhaustive pattern matching.
See also: [DX Updates (v0.5 Ergonomics)](./dx-updates.md) for guards and nested destructuring patterns.

## Defining sum types

```
type Shape {
    Circle { radius: 0.0 }
    Rect { w: 0.0, h: 0.0 }
}
```

Each variant can carry different fields.

> **Syntax note:** Variant fields use `field: default_value` — the default value doubles as the type hint. This is different from `component` / `struct` / `resource` declarations, which use `field: Type = default_value`. If you accidentally write the component-style syntax inside a variant, the parser will tell you exactly what to fix.
>
> ```
> // Component — type and default separated by =
> component Health { hp: int = 100 }
>
> // Sum type variant — default value only (type is inferred)
> type Shape {
>     Circle { radius: 0.0 }
> }
> ```

## Constructing values

```
let c = Shape::Circle { radius: 5.0 }
let r = Shape::Rect { w: 10.0, h: 3.0 }
```

## Pattern matching

Use `match` to destructure and branch on variants:

```
fn area(s) {
    match s {
        Circle { radius } => { return 3.14159 * radius * radius }
        Rect { w, h } => { return w * h }
    }
}

print(area(c))
print(area(r))
```

Match is exhaustive — every variant must be covered.

If you only care about the variant tag and not its fields, you can match the variant by name only (a bare variant match):

```
match s {
    Circle => { print("It's a circle") }
    Rect => { print("It's a rectangle") }
}
```

However, if you open braces to destructure a variant, you must bind all of its fields exhaustively (or use the `..` rest operator in v0.5 compat mode to ignore the remaining fields).

When `match` is used as an expression (e.g., `let x = match s { ... }`), all match arms must return the same type. Any match arms appearing after an unconditional wildcard (`_`) are flagged as unreachable errors.

Arms may be **bare expressions** — `pat => expr` is sugar for `pat => { expr }` — and `when` guards compose with them, turning if/else ladders into guard chains:

```
let order = match brain.cc {
    Cc::Taunted { by: t } when alive(t) => Order::Attack { target: t }
    Cc::Stunned {} => Order::Hold {}
    Cc::Free {} when len(queue) == 0 => Order::Hold {}
    Cc::Free {} => queue[0]
}
```

Block arms yield their **last expression** when a value is needed.

> **Pro Tip:** If you have a massive sum type but only care about a few variants in a specific function, using `_` will cause you to lose exhaustiveness checking if you add new variants later. To retain exhaustiveness, split the domain into smaller nested sum types and match the relevant inner type explicitly.

## Matching on Primitives

You can also use `match` on primitive types like `str`, `int`, `float`, and `bool`. When matching on an open set like strings or integers, you **must** provide an unconditional wildcard arm (`_ => { ... }`) to ensure the match is exhaustive.

```
fn handle_command(cmd: str) {
    match cmd {
        "open" => { print("Opening file...") }
        "save" => { print("Saving file...") }
        "quit" => { print("Exiting...") }
        _ => { print("Unknown command: " + cmd) }
    }
}
```

## Nested matching

Match expressions work inside event handlers, systems, and pipelines:

```
let shapes = [
    Shape::Circle { radius: 1.0 },
    Shape::Rect { w: 2.0, h: 3.0 },
    Shape::Circle { radius: 4.0 }
]

let areas = shapes |> map(fn(s) {
    match s {
        Circle { radius } => { return 3.14159 * radius * radius }
        Rect { w, h } => { return w * h }
    }
})

print(areas)
```

## Checking variants with `is`

If you just want to check whether a value is a specific variant without destructuring its fields, you can use the `is` operator. This evaluates to a boolean and is especially useful in `if` conditions or filter pipelines:

```
let c = Shape::Circle { radius: 5.0 }

if c is Circle {
    print("It's a circle!")
}

let circles_only = shapes |> filter(fn(s) { return s is Circle })
```

## Zero-field variants

Variants with no fields still require braces by default:

```
type Option {
    Some { value: nil }
    None { }
}

let x = Option::None { }
```

In [v0.5 compatibility mode](../reference/compat-v05.md), the shorthand `Option::None` (without braces) is accepted.

For the built-in `Option` and `Result` types specifically, the language provides a special tuple/unit shorthand. You can write `Some(x)`, `None()`, `None`, `Ok(x)`, and `Err(x)` instead of the verbose braced syntax.
