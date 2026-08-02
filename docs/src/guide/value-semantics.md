# Value Semantics

Containers in Rad (lists, maps, components) use value semantics. Assignment makes an independent copy — mutations to the copy never affect the original.

## Lists

```rad
let mut a = [1, 2, 3]
let mut b = a
b[0] = 99
print(a[0])   // 1 (unchanged)
```

## Transforms return new values

List operations return new lists instead of mutating in place:

```rad
let mut xs = [3, 1, 2]
xs = push(xs, 4)     // [3, 1, 2, 4]
xs = sort(xs)         // [1, 2, 3, 4]
```

The original list is never mutated. You explicitly reassign to `xs` if you want to keep the result.

For list appending, use the `push` builtin and rebind the result:

```rad
let mut _xs2 = [1, 2]
_xs2 = push(_xs2, 3)
```

## Components

Component values behave the same way:

```rad
component Health { hp: 100, max: 100 }
let hero = spawn(Health { hp: 100, max: 100 })

let h = get(hero, Health)?
let _h2 = h
// _h2 is an independent copy — modifying _h2 does not affect the entity's Health
```

To update an entity's component, you must call `set`:

```rad
set(hero, Health { hp: 50, max: 100 })
```

### Plain-data rule (Law 1 gate)

Component and struct fields must be plain data. Function/closure-typed fields are rejected by the checker.

This keeps ECS storage data-only and prevents closure/capture-cell cycles from entering world columns.

## Unique bindings (`let unique`)

When performance is critical and you want to guarantee that a value is never aliased (and therefore never deep-cloned), use `let unique`:

```rad
let unique mut xs = []
for i in range(100000) {
    xs << i  // always in-place — no Arc clone ever
}
```

The compiler rejects any code that would create an alias:

```rad,ignore
let unique data = [1, 2, 3]
let copy = data      // ← compile error: cannot alias unique binding 'data'
some_fn(data)        // ← compile error: cannot pass unique binding as argument
let f = fn() { print(data) }  // ← compile error: cannot capture unique binding
```

Reassignment to the same name is allowed, enabling transform-and-rebind patterns:

```rad
let unique mut xs = [3, 1, 2]
xs = sort(xs)    // OK — result is assigned back to xs
```

Use `unique` for hot-path accumulators, large lists built in loops, or any binding where an accidental alias would cause a performance cliff.

## Diagnosing hidden copies (`--profile-copies`)

Run your program with `--profile-copies` to find unexpected deep clones:

```bash
rad main.rad --profile-copies
```

When a list mutation triggers `Arc::make_mut` on a shared backing buffer, the VM prints a diagnostic to stderr:

```
[copy-profile] line 42: deep clone of 10000-element list (Arc refcount was 2)
```

This helps identify accidental aliasing in hot loops. If you see a clone you didn't expect, consider using `let unique` or restructuring the code to avoid sharing.

## Why value semantics?

- No aliasing bugs — two variables never point to the same mutable state.
- Predictable behavior in pipelines — transforms always produce new values.
- Thread-safe by default — no shared mutable state means no data races.

## Performance: lists, maps, strings, and ECS

- **Lists (`Arc<Vec<Value>>`):** Contiguous element storage behind an `Arc`. When you assign a list `let b = a`, it is a cheap pointer copy. If you modify `b` and the list is still shared, the runtime clones the backing vector once (`Arc::make_mut`); if `b` is the unique owner, mutations happen in place. The compiler further optimizes common patterns like `list = push(list, item)` and `list << item` to mutate the vector directly in its local stack slot (`Op::ListPushLocal`), completely avoiding the overhead of pushing and popping the list to the stack.
- **Maps (persistent HAMT):** Maps use `im::HashMap` (Hash Array Mapped Trie) for $O(\log N)$-style structural sharing on updates when shared.
- **Strings (`Arc<str>`):** Heap strings are stored as `Arc<str>` inside `Object::Str`. When a string crosses the air gap between persistent ECS storage and the execution stack (e.g. via `get` / `peek`), only the `Arc` shell is cloned — the underlying bytes are shared, making string reads O(1) regardless of length.
- **Structure-of-Arrays (SoA) ECS:** When components are stored in the ECS world, they are stripped apart and stored in flat, contiguous arrays for maximum CPU cache utilization during system execution.
- **Air-gap isolation (ECS reads/writes):** The runtime enforces strict isolation between ECS (persistent storage) and the execution stack (arena/GC). Values written through `set` / `spawn` are deep-copied into persistent storage; values read through `get` / `peek` are deep-copied out. String fields are O(1) per field via `Arc<str>` sharing. System-local temporaries are allocated in a per-system arena (`BumpArena`) and reset between systems.
- **NaN-Boxing:** The core `Value` type is exactly 64 bits. Integers up to 48 bits are stored unboxed directly inside the NaN payload, meaning most math requires zero heap allocation.
