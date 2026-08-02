# Modules & Imports

Rad supports multi-file projects through the `use` statement. Split your code across files, and the module loader merges everything into a single program before compilation.

## Importing a file

Use a quoted relative path to import another `.rad` file:

```rad,ignore
use "math.rad"
```

All top-level declarations in the imported file (`fn`, `component`, `resource`, `entity`, `state`, `event`, `type`) become available in the importing file. Paths are resolved relative to the directory of the file containing the `use` statement.

## Example — multi-file project

```rad,ignore
// lib/stats.rad
fn square(x) { return x * x }
fn clamp(x, lo, hi) {
    if x < lo { return lo }
    if x > hi { return hi }
    return x
}
```

```rad,ignore
// main.rad
use "lib/stats.rad"

component Health { hp: 100, max: 100 }

fn apply_damage(current, amount) {
    return clamp(current - amount, 0, 100)
}

print(square(5))                  // 25
print(apply_damage(80, 30))       // 50
```

Run with:

```bash
rad main.rad
```

The module loader reads `main.rad`, sees `use "lib/stats.rad"`, loads that file first, then merges all declarations together.

## Import aliasing

To avoid name collisions or to organize code under a namespace, use the `as` keyword:

```rad,ignore
use "math.rad" as math

print(math.square(5))         // 25
print(math.add(3, 4))         // 7
```

Aliased imports are **exclusive** — the imported module's `pub` declarations are only accessible through the alias prefix. They do not enter the flat namespace. Private declarations are not accessible from outside; attempting to use them produces a clear error:

```rad,ignore
  Error: 'secret' is private in module alias 'p'
  hint: Add `pub` to the declaration of 'secret' in the imported module
```

Aliasing resolves the flat-namespace collision problem. Two modules can define identically named declarations without conflict:

```rad,ignore
use "physics.rad" as phys
use "rendering.rad" as gfx

phys.update()    // calls physics update
gfx.update()     // calls rendering update — no collision
```

## Module-level Contracts (Ports & Adapters)

To enforce strict architectural boundaries, you can require an imported module to satisfy a **contract**. This implements the Ports & Adapters pattern directly at the module boundary.

Define a contract as a `struct` containing the required function signatures:

```rad,ignore
struct StoragePort {
    get: fn(str) -> Option<str>,
    set: fn(str, str) -> bool,
}
```

When importing a module, append `: ContractName` to verify it provides the required `pub` function exports with matching signatures:

```rad,ignore
use "redis_storage.rad" as storage : StoragePort

// If redis_storage.rad doesn't export `pub fn get(key: str) -> Option<str>`,
// it's a compile-time error.
```

Contracts are intended for function-port boundaries. Use ordinary aliased imports for components, resources, structs, sum types, state machines, and events.

Aliased modules work with all declaration kinds: functions, components, resources, structs, types, state machines, and events. Type references within the aliased module's own code work correctly — `Color::Red` inside `types.rad` still refers to the module's own `Color` type.

```rad,ignore
// types.rad
pub type Color {
    Red { intensity: int }
    Blue { intensity: int }
}
pub fn make_red(n: int) -> Color {
    return Color::Red { intensity: n }
}

// main.rad
use "types.rad" as t
let c = t.make_red(5)
let r = t.Color::Red { intensity: 42 }
```

## Flat namespace

Bare `use` imports (without `as`) merge all `pub` declarations into a single global namespace. If two files define the same top-level name, the loader rejects the program with an error that names both definition sites:

```rad,ignore
Error: Duplicate top-level declaration 'square' (already defined at lib/stats.rad:1)
  --> utils.rad:3:1
```

This applies across declaration kinds — a function and a type cannot share the same name. Use import aliasing to avoid such collisions.

## Circular imports

Circular imports are safe. The loader tracks which files have already been visited and skips them on re-encounter:

```rad,ignore
// a.rad
use "b.rad"
fn a_label() { return "A" }

// b.rad
use "a.rad"
fn b_label() { return "B" }

// main.rad
use "a.rad"
print(a_label())   // A
print(b_label())   // B
```

Both `a_label` and `b_label` are loaded exactly once regardless of the cycle.

## Transitive dependencies

If `main.rad` imports `hub.rad`, and `hub.rad` imports `leaf.rad`, then `main.rad` gets all three files' declarations. Diamond-shaped dependency graphs (where multiple paths lead to the same file) are handled correctly — each file is loaded only once. The multi-file declaration merge order is a deterministic topological order with path tie-breaking.

## Error reporting with source maps

When a multi-file program produces a syntax or type-check error, the diagnostic points to the original file and line number — not the merged offset. Because the compiler uses **error recovery**, it will collect and report all errors across the entire module graph in a single run:

```rad,ignore
  Error: Expected RBrace, got Eof ('')
  --> lib/stats.rad:4:12

  Error: Unknown variable 'hp'
  --> main.rad:12:5
```

## Lockfile

Pass `--write-lock` to record a `forge.lock` file alongside the entry point:

```bash
rad main.rad --write-lock
```

The lockfile lists every module in the dependency graph with its byte size,
FNV-1a checksum, and SHA-256 digest. This lets CI pipelines detect unexpected
changes to dependencies between runs.

## Remote import limits

Projects that use remote HTTP(S) imports can set fetch limits in `rad.toml`.
The module loader reads the optional `[network]` section next to the entry
program:

```toml
[network]
max_module_size = "4M"
fetch_timeout_secs = 12
```

`max_module_size` accepts raw bytes or `K`, `M`, and `G` suffixes. The timeout
is in seconds and must be greater than zero. When a project has no `[network]`
section, Rad defaults to a 2 MiB remote module limit and a 5 second fetch
timeout.

For production and CI, pair remote imports with a committed `forge.lock` so
remote bytes are size-checked and digest-pinned.

## Current scope

The module system supports local relative files and direct HTTP(S) imports.
There is no package registry, package version resolution, or package manager
yet.

## Visibility and Strict Boundaries

By default, all top-level declarations (`fn`, `component`, `struct`, `entity`, `state`, `event`, `type`) are **private** to the file they are defined in. To make a declaration accessible from other files, it must be prefixed with the `pub` keyword:

```rad,ignore
pub fn public_helper(x: int) -> int { return x }
pub component PublicComp { x: int = 0 }
```

If a file imports another file but attempts to use a private declaration from it, a compile-time error is raised.

**Strict Module Boundaries:** Any declaration marked `pub` requires explicit type annotations, regardless of whether `--strict-types` is enabled. For functions, this means parameter and return types must be specified. For components, resources, and structs, all fields must have type annotations. The compiler enforces this during the lowering phase and performs a reachability analysis to ensure public APIs do not leak private types. This ensures that the public API of a module is always strictly typed, preventing the ecosystem from fracturing into "typed" and "untyped" code while allowing fast, inferred iteration inside private module bodies.

```rad,ignore
struct SecretToken { value: str = "" }

pub fn leak() -> SecretToken {
    return SecretToken { value: "dev-only" }
}
```

That module is rejected because `SecretToken` is private but appears in a
public return type. Either make the type public or keep the function private:

```rad,ignore
pub struct PublicToken { value: str = "" }

pub fn token() -> PublicToken {
    return PublicToken { value: "ok" }
}
```

## Exported constants: `pub let`

Top-level `let` bindings can be exported with `pub let` — the right tool for
shared constants like tick rates, map dimensions, and generated content
tables (instead of wrapping every number in a zero-arg function):

```rad,ignore
// gen_content.rad
pub let TICK_RATE = 30
pub let MS_PER_TICK = 1000 / TICK_RATE
pub let SPAWN_GRID = [12, 34, 56]

// main.rad
use "gen_content.rad"
print(MS_PER_TICK)        // 33
```

Rules:

- **Single immutable name only.** `pub let mut` is rejected (exported mutable
  globals are shared mutable state in disguise — use a resource), and so is
  tuple destructuring.
- **Bare `use` only.** Module aliases (`use "x.rad" as m`) expose `pub` fns
  and types, not lets; `m.CONST` gets a targeted error suggesting a bare
  import or a pub fn. A module's own fns can read its top-level lets either
  way.
- A `pub let` is a module export, so it is exempt from unused-variable
  warnings; private top-level lets still warn when unread.

## What's next

Future additions on the roadmap include:

- **Package registry** (Q4 2026) — package manager and a `[dependencies]` section in `rad.toml` for shared, versioned packages
