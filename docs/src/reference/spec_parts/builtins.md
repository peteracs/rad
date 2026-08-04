## 6. Built-in Functions

### General
| Function | Signature | Description |
|---|---|---|
| `print` | `print(args...)` | Print values to stdout |
| `str` | `str(val) -> str` | Convert to string |
| `int` | `int(val) -> int` | Convert to integer |
| `float` | `float(val) -> float` | Convert to float |
| `len` | `len(list\|str\|map) -> int` | Length of list, string, or map |
| `range` | `range(n)`, `range(a, b)`, `range(a, b, step)` | Generate list of integers |
| `typeof` | `typeof(val) -> str` | Type name as string |
| `variant_of` | `variant_of(val) -> str` | Returns the variant name if `val` is a sum type or state, else `nil` |
| `abs` | `abs(num) -> num` | Absolute value |
| `min` | `min(a, b) -> num` | Minimum |
| `max` | `max(a, b) -> num` | Maximum |
| `int_div` | `int_div(a: int, b: int) -> int` | Truncating integer division (rounds toward zero) |
| `rand_int` | `rand_int(min: int, max: int) -> int` | Random integer in inclusive range `[min, max]` |
| `rand_float` | `rand_float() -> float` | Random float in range `[0.0, 1.0)` |
| `rand_bool` | `rand_bool() -> bool` | Random boolean |
| `rand_seed` | `rand_seed(seed: int) -> nil` | Set PRNG seed for deterministic pseudo-random sequences |

For a small replayable game-style example, see `tests/conformance/rng_seeded_dungeon_reproducible.rad`.

### I/O, File, HTTP, and Networking
| Function | Signature | Description |
|---|---|---|
| `print` | `print(...) -> nil` | Print values to stdout (with newline) |
| `eprint` | `eprint(...) -> nil` | Print values to stderr (with newline) |
| `log` | `log(level: str, data: map) -> nil` | Print structured JSON log with trace context; only string-keyed map entries are emitted |
| `metric` | `metric(type: str, name: str, value: float, tags: map) -> nil` | Print structured JSON metric with trace context; only string-keyed tags are emitted |
| `trace_id` | `trace_id() -> int \| nil` | Get the current distributed trace ID inside event handling, or `nil` outside an event context |
| `write_stdout` | `write_stdout(str) -> nil` | Write string to stdout without newline |
| `write_stderr` | `write_stderr(str) -> nil` | Write string to stderr without newline |
| `flush_stdout` | `flush_stdout() -> nil` | Explicitly flush stdout |
| `input` | `input([prompt]) -> str` | Print optional prompt and read a line from stdin |
| `readline` | `readline() -> str` | Read a line from stdin |
| `read_stdin_all` | `read_stdin_all() -> str` | Read all of stdin until EOF |
| `read_file` | `read_file(path: str) -> str` | Read UTF-8 text file contents |
| `write_file` | `write_file(path: str, content: str) -> nil` | Write UTF-8 text file contents (overwrite) |
| `append_file` | `append_file(path: str, content: str) -> nil` | Append text to file |
| `read_file_bytes` | `read_file_bytes(path: str) -> list<int>` | Read file as bytes |
| `write_file_bytes` | `write_file_bytes(path: str, bytes: list<int>) -> nil` | Write bytes to file |
| `file_exists` | `file_exists(path: str) -> bool` | Check if a file or directory exists |
| `remove_file` | `remove_file(path: str) -> nil` | Delete a file |
| `list_dir` | `list_dir(path: str) -> list<str>` | List directory entries |
| `create_dir` | `create_dir(path: str) -> nil` | Create a directory (including parents) |
| `remove_dir` | `remove_dir(path: str) -> nil` | Recursively remove a directory |
| `http_get` | `http_get(url: str) -> str` | Blocking HTTP GET, returns response body text |
| `http_post` | `http_post(url: str, body: str) -> str` | Blocking HTTP POST, returns response body text |
| `http_post_json` | `http_post_json(url: str, body: str) -> str` | HTTP POST with JSON content-type |
| `http_request` | `http_request(method: str, url: str, headers: map, body: str) -> map` | Full HTTP request, returns map with status, headers, body |
| `tcp_connect` | `tcp_connect(host: str, port: int) -> int` | Open TCP connection, return handle |
| `tcp_listen` | `tcp_listen(host: str, port: int) -> int` | Bind TCP listener, return handle |
| `tcp_accept` | `tcp_accept(handle: int) -> int` | Accept TCP connection, return handle |
| `tcp_accept_timeout` | `tcp_accept_timeout(handle: int, timeout_ms: int) -> Option<int>` | Accept TCP connection with a deadline; `timeout_ms = 0` polls |
| `tcp_read` | `tcp_read(handle: int, max_bytes: int) -> str` | Read from TCP stream |
| `tcp_write` | `tcp_write(handle: int, data: str) -> nil` | Write to TCP stream |
| `tcp_close` | `tcp_close(handle: int) -> nil` | Close TCP handle |
| `udp_bind` | `udp_bind(host: str, port: int) -> int` | Bind UDP socket, return handle |
| `udp_recv_from` | `udp_recv_from(handle: int, max_bytes: int) -> (str, str, int)` | Receive one UDP datagram as `(data, host, port)` |
| `udp_recv_from_timeout` | `udp_recv_from_timeout(handle: int, max_bytes: int, timeout_ms: int) -> Option<(str, str, int)>` | Receive one UDP datagram with a deadline; `timeout_ms = 0` polls |
| `udp_recv_from_bytes` | `udp_recv_from_bytes(handle: int, max_bytes: int) -> (list<int>, str, int)` | Receive one UDP datagram as `(data_bytes, host, port)` |
| `udp_recv_from_bytes_timeout` | `udp_recv_from_bytes_timeout(handle: int, max_bytes: int, timeout_ms: int) -> Option<(list<int>, str, int)>` | Receive one UDP datagram as bytes with a deadline; `timeout_ms = 0` polls |
| `udp_recv_bytebuf` | `udp_recv_bytebuf(handle: int, max_bytes: int) -> (any, str, int)` | Receive one UDP datagram as `(bytebuf, host, port)` |
| `udp_recv_bytebuf_timeout` | `udp_recv_bytebuf_timeout(handle: int, max_bytes: int, timeout_ms: int) -> Option<(any, str, int)>` | Receive one UDP datagram as a native byte buffer with a deadline; `timeout_ms = 0` polls |
| `udp_send_to` | `udp_send_to(handle: int, host: str, port: int, data: str) -> int` | Send one UDP datagram, return bytes sent |
| `udp_send_to_bytes` | `udp_send_to_bytes(handle: int, host: str, port: int, data_bytes: list<int>) -> int` | Send one UDP byte datagram, return bytes sent |
| `udp_send_bytebuf` | `udp_send_bytebuf(handle: int, host: str, port: int, data: any) -> int` | Send one native byte-buffer UDP datagram, return bytes sent |
| `udp_close` | `udp_close(handle: int) -> nil` | Close UDP socket handle |

Browser clients cannot call these native socket builtins from WASM. Use a
host-owned WebTransport edge process for browser datagrams and keep RAD as the
UDP authority; see [WebTransport Edge Networking](../guide/webtransport-networking.md).

### Byte Buffers
| Function | Signature | Description |
|---|---|---|
| `bytebuf_new` | `bytebuf_new(size: int) -> any` | Create a zero-filled native byte buffer |
| `bytebuf_len` | `bytebuf_len(buf: any) -> int` | Return byte length |
| `bytebuf_get` | `bytebuf_get(buf: any, index: int) -> int` | Read one byte as `0..255` |
| `bytebuf_set_u8` | `bytebuf_set_u8(buf: any, index: int, value: int) -> any` | Write one byte |
| `bytebuf_set_u32_le` | `bytebuf_set_u32_le(buf: any, offset: int, value: int) -> any` | Write a little-endian unsigned 32-bit int |
| `bytebuf_set_i32_le` | `bytebuf_set_i32_le(buf: any, offset: int, value: int) -> any` | Write a little-endian signed 32-bit int |
| `bytebuf_get_u32_le` | `bytebuf_get_u32_le(buf: any, offset: int) -> int` | Read a little-endian unsigned 32-bit int |
| `bytebuf_get_i32_le` | `bytebuf_get_i32_le(buf: any, offset: int) -> int` | Read a little-endian signed 32-bit int |
| `bytebuf_to_list` | `bytebuf_to_list(buf: any) -> list<int>` | Convert to a byte list |
| `bytebuf_from_list` | `bytebuf_from_list(bytes: list<int>) -> any` | Convert a byte list to a native byte buffer |

### ECS
| Function | Signature | Description |
|---|---|---|
| `get` | `get(entity, ComponentName) -> Option` | `Some(component)` or `None` |
| `require` | `require(entity, ComponentName) -> component` | Returns component directly; runtime error if missing |
| `require_all` | `require_all(entity, ComponentName...) -> list` | Returns components in requested order; runtime error on first missing |
| `set` | `set(entity, component_value)` | Set/replace component on entity |
| `has` | `has(entity, ComponentName) -> bool` | Check if entity has component |
| `spawn` | `spawn([name], components...) -> entity_id` | Create a new entity |
| `remove` | `remove(entity, ComponentName) -> bool` | Remove a component from an entity |
| `despawn` | `despawn(entity) -> bool` | Destroy an entity |
| `entities` | `entities([ComponentName...]) -> list` | Return all entity IDs, or filter by entities having all listed components |
| `query_where` | `query_where(ComponentName..., fn) -> list` | Filter entities having components using a predicate function on entity ID |
| `query_map` | `query_map(ComponentName..., fn) -> list` | Map over entities having components using a function on entity ID |
| `query_count` | `query_count(ComponentName...) -> int` | Return the number of entities having the given components |
| `with_field` | `with_field(entities, ComponentName, FieldName, fn) -> list` | Filter a list of entities by evaluating a predicate function on a specific component field |
| `lookup` | `lookup(ComponentName, field_name: str, value) -> Option<entity>` | O(1) lookup: returns `Some(entity_id)` for the first entity whose `indexed` field matches `value`, or `None` |

> **Note:** `lookup()` requires the field to be declared as `indexed` in the component declaration. Non-indexed fields produce a runtime error.

> **Note:** The `query { ... } select ... where ...` expression is the preferred, fastest way to query entities and project component data. The builtin functions above are maintained for dynamic use cases.

> **See also:** Entity literal expressions (§5.9) provide a declarative alternative to `spawn()` + `set()` when creating entities with known components inline. Named entity literals (`entity "name" { ... }`) also replace the `spawn("name") + set()` pattern.

### World Forking (Speculative Execution)
| Function | Signature | Description |
|---|---|---|
| `fork` | `fork() -> world_fork` | Copy-on-write snapshot of the entire ECS world (entities, components, archetypes). O(A) shallow `Arc` refcount bumps on column handles/maps; actual data cloning deferred to first mutation via `Arc::make_mut` (see [Memory Model](memory-model.md)). |
| `simulate` | `simulate(fork, systems, ticks) -> world_fork` | Run the listed systems on a fork for N ticks, returning the updated fork. Ground-truth semantics live in the Rust VM. |
| `commit` | `commit(fork) -> nil` | Atomically replace the live ECS world with the fork's state. **Clears all pending events.** |
| `peek` | `peek(fork, entity, Component) -> Option<Component>` | Read a component from the fork without committing. Returns `None` if the entity or component does not exist in the fork. Values are deep-copied across the air gap (O(F) for F fields; string fields are O(1) via `Arc<str>`). |

**Static system list.** The second argument to `simulate` must be a **list literal** whose elements are **`system::…` references** naming declared `system`s — for example `simulate(f, [system::Decay, system::Physics], 3)`. String literals in that list are rejected at compile time (use `system::Name` instead). Multi-segment paths use additional `::` (for example `system::alias::Sys` → the same qualified-name rules as `schedule [alias.Sys, …]`). Unknown system names are compile-time errors.

As a convenience for sharing one schedule across call sites, a reference to a **top-level immutable binding** whose value is such a list literal is also accepted and const-folds to it — `let ROLLOUT = [system::Decay, system::Physics]` then `simulate(f, ROLLOUT, 3)`. The binding must be top-level, immutable (`let`, not `let mut`), and a plain list literal of `system::…` references; an arbitrary variable, a `let mut`, or a computed expression (a function call, concatenation, etc.) is still rejected, because the checker resolves the schedule statically with no dataflow analysis.

**Syntax.** `system::Name` is an expression of type `system`. At runtime, values in the schedule list are **`system` references** (`Value`/`Object::SystemRef`), not strings.

**Design note.** `simulate` is exposed as a builtin call for parser and composability, but the second argument is checked like a dedicated syntactic form (as if it were a macro or keyword), not like a normal function parameter.

`fork()` creates a copy-on-write snapshot of the ECS world (O(A) `Arc` refcount bumps, not a full deep copy). `simulate()` runs systems on the fork in isolation: IO, `commit()`, unsafe event-effect calls such as `transition`, and unsafe handler chains are statically forbidden. `emit` statements are allowed; emitted events dispatch on the fork's own event queue and any events still pending at the end stay with the returned fork. `commit()` replaces the real world (O(1) pointer swap) and **discards all pending events** in the main timeline, since they reference pre-commit state that no longer exists. `peek()` reads from a fork without modifying any state; values are deep-copied across the air gap into the caller's heap (string fields are O(1) via shared `Arc<str>`).

### State Machines
| Function | Signature | Description |
|---|---|---|
| `transition` | `transition(state_inst, event_name) -> Result` | `Ok(state_inst)` or `Err(message)`; transition guards are declared in `state` with `when ...` |

### Option / Result helpers
| Function | Signature | Description |
|---|---|---|
| `unwrap` | `unwrap(option_or_result) -> value` | Unwraps `Some` / `Ok`; errors on `None` / `Err` |
| `unwrap_or` | `unwrap_or(option_or_result, default) -> value` | Unwraps `Some` / `Ok`; returns `default` on `None` / `Err` |
| `expect` | `expect(option_or_result, [msg]) -> value` | Like `unwrap` with a custom failure message |
| `map_or` | `map_or(option_or_result, default, fn) -> value` | Returns `fn(inner)` for `Some/Ok`, otherwise `default` |
| `is_some` | `is_some(option_or_result) -> bool` | Returns `true` if `Some` / `Ok` |
| `is_none` | `is_none(option_or_result) -> bool` | Returns `true` if `None` / `Err` |

### List & Pipeline
| Function | Signature | Description |
|---|---|---|
| `map` | `map(list, fn) -> list` | Transform each element |
| `filter` | `filter(list, fn) -> list` | Keep elements where fn returns truthy |
| `reduce` | `reduce(list, init, fn) -> value` | Fold list to single value |
| `flat_map` | `flat_map(list, fn) -> list` | Map then flatten (callback returns list) |
| `group_by` | `group_by(list, fn) -> map` | Group elements by key function |
| `push` | `push(list, val) -> list` | Return new list with val appended (use `list << val` for in-place mutation) |
| `pop` | `pop(list) -> value` | Return the last element (same as `pop_last`) |
| `pop_last` | `pop_last(list) -> value` | Return the last element |
| `drop_last` | `drop_last(list) -> list` | Return list without the last element |
| `sort` | `sort(list) -> list` | Return sorted copy |
| `sort_by` | `sort_by(list, fn) -> list` | Return sorted copy using key function |
| `reverse` | `reverse(list\|str) -> list\|str` | Return reversed copy |
| `slice` | `slice(list, start, end) -> list` | Return sub-list (also works on strings) |
| `append` | `append(list, list) -> list` | Concatenate two lists |
| `extend` | `extend(list, list) -> list` | Alias for `append` |
| `zip` | `zip(list, list) -> list` | Pair elements into `[[a, b], ...]` |
| `enumerate` | `enumerate(list) -> list` | Return `[[0, elem₀], [1, elem₁], ...]` index-element pairs |
| `find` | `find(list, fn) -> Option` | First element where `fn` returns truthy, or `None` |
| `max_by` | `max_by(list, fn) -> Option` | Element with largest key from `fn`, or `None` if empty |
| `min_by` | `min_by(list, fn) -> Option` | Element with smallest key from `fn`, or `None` if empty |
| `contains` | `contains(list\|str\|map, val) -> bool` | Check membership |

### Map / Collection
| Function | Signature | Description |
|---|---|---|
| `keys` | `keys(map\|component) -> list` | Sorted key names (deterministic) |
| `values` | `values(map) -> list` | List of map values (sorted deterministically by key) |
| `entries` | `entries(map) -> list` | List of `[key, value]` pairs (sorted deterministically by key) |
| `merge` | `merge(map, map) -> map` | Merge maps (second wins) |
| `remove_key` | `remove_key(map, key) -> map` | Return new map with key removed |

### String
| Function | Signature | Description |
|---|---|---|
| `split` | `split(str, delim) -> list<str>` | Split string by delimiter |
| `join` | `join(list, sep) -> str` | Join list elements with separator |
| `trim` | `trim(str) -> str` | Strip whitespace |
| `replace` | `replace(str, old, new) -> str` | Replace all occurrences |
| `starts_with` | `starts_with(str, prefix) -> bool` | Prefix check |
| `ends_with` | `ends_with(str, suffix) -> bool` | Suffix check |
| `regex_is_match` | `regex_is_match(pattern: str, text: str) -> bool` | True when regex pattern matches text |
| `regex_find` | `regex_find(pattern: str, text: str) -> Option[str]` | First regex match as `Some(value)` or `None` |
| `chr` | `chr(int) -> str` | Unicode code point to character |
| `ord` | `ord(str) -> int` | First character to code point |
| `chars` | `chars(str) -> list<str>` | Split into character list |
| `to_upper` | `to_upper(str) -> str` | Uppercase conversion |
| `to_lower` | `to_lower(str) -> str` | Lowercase conversion |
| `format` | `format(template, args...) -> str` | Replace `{}` placeholders with arguments in order |
| `format_value` | `format_value(value, spec: str) -> str` | Format a single value using a Python-style format specifier (see §5.6) |

### Date / Time
| Function | Signature | Description |
|---|---|---|
| `now_unix_s` | `now_unix_s() -> int` | Current UNIX timestamp (seconds) |
| `now_unix_ms` | `now_unix_ms() -> int` | Current UNIX timestamp (milliseconds) |

### Safe Conversion
| Function | Signature | Description |
|---|---|---|
| `try_int` | `try_int(val) -> Option` | Safe int conversion (no error) |
| `try_float` | `try_float(val) -> Option` | Safe float conversion (no error) |

### Testing
| Function | Signature | Description |
|---|---|---|
| `assert` | `assert(condition: bool, msg: str)` | Assert condition is true; runtime error with `msg` on failure |
| `assert_eq` | `assert_eq(a, b)` | Assert two values are equal; runtime error on mismatch |

### Test Data Generation
| Function | Signature | Description |
|---|---|---|
| `gen_int` | `gen_int() -> list<int>` | Generate a list of test integers |
| `gen_float` | `gen_float() -> list<float>` | Generate a list of test floats |
| `gen_str` | `gen_str() -> list<str>` | Generate a list of test strings |
| `gen_bool` | `gen_bool() -> list<bool>` | Generate a list of test booleans |
| `gen_list` | `gen_list(list) -> list<list<any>>` | Generate a list of test lists |

`gen_*` builtins are deterministic test generators, not runtime random-number APIs.
Use `rand_*` for pseudo-random values.

List-transforming builtins return new lists instead of mutating in place.
`pop`/`pop_last` return the removed element; use `drop_last` to get the remaining list.
Rebind explicitly:

```rad
let mut xs = [1, 2, 3]
xs = push(xs, 4)
xs = sort(xs)
let popped = pop(xs)
xs = drop_last(xs)
```

Calling `push(xs, v)` as a standalone statement has no effect on `xs` unless the returned list is assigned/rebound.

---
