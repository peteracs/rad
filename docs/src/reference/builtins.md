# Built-in Functions

All collection builtins take the collection as the first argument, making them
pipeline-friendly: `[1,2,3] |> push(4) |> sort`.

See also: [Language Guarantees](./guarantees.md) for behavioral contracts and
[DX Updates](../guide/dx-updates.md) for match guards and CLI flags.

> **Performance note:** Lists use `Arc<Vec<Value>>`, maps use persistent HAMTs (`im::HashMap`), and strings use `Arc<str>`. Copy-on-write applies: if a value is uniquely owned, updates often reuse backing storage; if it is shared, the runtime clones before mutating to preserve value semantics. ECS reads (`get`/`peek`) deep-copy values across the air gap between persistent storage and the execution stack; string fields are O(1) via `Arc<str>` sharing.

## Standard I/O

| Function | Description |
|---|---|
| `print(...)` | Print one or more values to stdout, separated by spaces (with trailing newline) |
| `eprint(...)` | Print one or more values to stderr, separated by spaces (with trailing newline) |
| `log(level, map)` | Print a structured JSON log with trace context. Only string-keyed map entries are emitted as JSON fields. |
| `metric(type, name, value, tags)` | Print a structured JSON metric with trace context. `tags` is a map; only string-keyed entries are emitted. |
| `trace_id()` | Current distributed trace ID inside event handling, or `nil` outside an event context |
| `flush_events()` | Process all pending emitted events (automatically called at end of `schedule`) |
| `write_stdout(str)` | Write string to stdout without trailing newline |
| `write_stderr(str)` | Write string to stderr without trailing newline |
| `flush_stdout()` | Explicitly flush the stdout buffer |
| `sleep_ms(ms)` | Sleep the current thread for `ms` milliseconds; non-positive values return immediately |
| `debug_trace(value)` | Log `value` to stderr and return it unchanged. Treated as pure for pipeline analysis; is a **ghost effect** — may be stripped in `--release` (VM) or when C is compiled with `-DRAD_RELEASE`. See [Language Guarantees](./guarantees.md#8-ghost-effects-observational-intrinsics). **Stripping:** only **direct** calls of the form `debug_trace(...)` where the callee is the builtin name are elided; aliased or indirect calls (e.g. via a variable) are not guaranteed to strip. **Inside a `sandbox_run` guest** the line does not reach stderr: like all guest output it is buffered and surfaced to the host tagged `[sandbox] DEBUG: …`, in execution order, so untrusted text can never impersonate the host's own diagnostics. |
| `input([prompt])` | Print optional prompt and read a line from stdin (returns task in async context) |
| `readline()` | Read a line from stdin (returns task in async context) |
| `read_stdin_all()` | Read all of stdin until EOF and return as string (returns task in async context) |

Structured observability output follows the current event trace when one exists:

```rad
on Damage {
    log("info", { "event": "Damage", "trace": trace_id() })
    metric("counter", "damage_events", 1, { "source": name_of(event.source) })
}
```

## File I/O

| Function | Description |
|---|---|
| `read_file(path)` | Read UTF-8 text file contents from `path` (returns task in async context) |
| `write_file(path, content)` | Write UTF-8 text `content` to `path` (returns task in async context) |
| `append_file(path, content)` | Append UTF-8 text `content` to file at `path`, creating if needed (returns task in async context) |
| `read_file_bytes(path)` | Read file as a list of byte integers (0–255) (returns task in async context) |
| `write_file_bytes(path, bytes)` | Write a list of byte integers (0–255) to file at `path` (returns task in async context) |
| `file_exists(path)` | Return `true` if a file or directory exists at `path` (returns task in async context) |
| `remove_file(path)` | Delete the file at `path` (returns task in async context) |
| `list_dir(path)` | Return a list of entry names in the directory at `path` (returns task in async context) |
| `create_dir(path)` | Create directory at `path`, including any missing parents (returns task in async context) |
| `remove_dir(path)` | Recursively remove directory at `path` (returns task in async context) |

## HTTP

| Function | Description |
|---|---|
| `http_get(url)` | Perform HTTP GET and return response body as string (returns task in async context) |
| `http_post(url, body)` | Perform HTTP POST with string body and return response body as string (returns task in async context) |
| `http_post_json(url, body)` | Perform HTTP POST with `Content-Type: application/json` and return response body as string (returns task in async context) |
| `http_request(method, url, headers, body)` | General-purpose HTTP request. `method` is `"GET"`, `"POST"`, etc. `headers` is a `map<str, str>`. Returns `{"status": int, "body": str, "headers": map}` (returns task in async context) |

## TCP Networking

| Function | Description |
|---|---|
| `tcp_connect(host, port)` | Open a TCP connection and return a handle (int) |
| `tcp_listen(host, port)` | Bind and listen on a TCP address, return a listener handle (int) |
| `tcp_accept(listener_handle)` | Accept an incoming connection on a listener, return a stream handle (int) |
| `tcp_accept_timeout(listener_handle, timeout_ms)` | Try to accept an incoming connection until `timeout_ms` elapses. Returns `Some(stream_handle)` or `None`; `timeout_ms = 0` is a nonblocking poll. |
| `tcp_read(handle, max_bytes)` | Read up to `max_bytes` from a TCP stream, return data as string |
| `tcp_write(handle, data)` | Write string data to a TCP stream |
| `tcp_close(handle)` | Close a TCP handle (stream or listener) |

## UDP Networking

UDP builtins operate on string, byte-list, or native `bytebuf` datagrams and
explicit `(data, host, port)` receive tuples. Prefer `bytebuf` variants for
compact binary game protocols and other high-frequency packet paths; use
byte-list variants for compatibility and tests, and string variants for
diagnostics or simple text protocols.
For browser clients, use a host WebTransport edge process that forwards
WebTransport datagrams to a RAD UDP authority; see
[WebTransport Edge Networking](../guide/webtransport-networking.md).

| Function | Description |
|---|---|
| `udp_bind(host, port)` | Bind a UDP socket and return a socket handle (int) |
| `udp_recv_from(socket, max_bytes)` | Block until one datagram arrives, returning `(data, host, port)` |
| `udp_recv_from_timeout(socket, max_bytes, timeout_ms)` | Try to receive one datagram until `timeout_ms` elapses. Returns `Some((data, host, port))` or `None`; `timeout_ms = 0` is a nonblocking poll. |
| `udp_recv_from_bytes(socket, max_bytes)` | Block until one datagram arrives, returning `(data_bytes, host, port)` where `data_bytes` is `list<int>` with byte values `0..255` |
| `udp_recv_from_bytes_timeout(socket, max_bytes, timeout_ms)` | Try to receive one datagram as bytes until `timeout_ms` elapses. Returns `Some((data_bytes, host, port))` or `None`; `timeout_ms = 0` is a nonblocking poll. |
| `udp_recv_bytebuf(socket, max_bytes)` | Block until one datagram arrives, returning `(data, host, port)` where `data` is a native byte buffer |
| `udp_recv_bytebuf_timeout(socket, max_bytes, timeout_ms)` | Try to receive one datagram as a native byte buffer until `timeout_ms` elapses. Returns `Some((data, host, port))` or `None`; `timeout_ms = 0` is a nonblocking poll. |
| `udp_send_to(socket, host, port, data)` | Send one string datagram and return the number of bytes sent |
| `udp_send_to_bytes(socket, host, port, data_bytes)` | Send one byte-list datagram and return the number of bytes sent. Each list element must be an int in `0..255`. |
| `udp_send_bytebuf(socket, host, port, data)` | Send one native byte-buffer datagram and return the number of bytes sent |
| `udp_close(socket)` | Close a UDP socket handle |

Use timeout receives in fixed-tick servers so simulation keeps advancing when no packets arrive:

```rad
let socket = udp_bind("127.0.0.1", 8788)
match udp_recv_from_timeout(socket, 2048, 0) {
    Some(packet) => {
        let data = packet[0]
        let host = packet[1]
        let port = packet[2]
        udp_send_to(socket, host, port, "ack:" + data)
    }
    None => nil
}
```

For binary protocols, keep decode/encode in a protocol module and make the
transport loop pass opaque byte buffers through:

```rad
let socket = udp_bind("127.0.0.1", 8788)
match udp_recv_bytebuf_timeout(socket, 1200, 0) {
    Some(packet) => {
        let bytes = packet[0]
        let host = packet[1]
        let port = packet[2]
        let reply = encode_snapshot_bytes(decode_input_bytes(bytes))
        udp_send_bytebuf(socket, host, port, reply)
    }
    None => nil
}
```

> **Platform note:** All I/O, file, HTTP, TCP, and UDP builtins are disabled in the WASM runtime and will return an error if called. In async contexts, blocking operations (file I/O, HTTP, stdin) automatically return a task that can be `await`-ed.

## Runtime / Host

| Function | Description |
|---|---|
| `load_extension(path)` | Native-only plugin bridge. Loads a dynamic library and returns a map of exported native functions; unsupported in WASM. |
| `gc_collect()` | Run the VM backup cycle collector and return the number of swept objects. This does not manage ECS world storage, which is handled by `Arc` reference counts. |

`load_extension()` is the generic boundary for project-owned acceleration.
Domain algorithms do not become VM builtins: an extension registers named
scalar functions, and a project adapter may exchange canonical JSON when it
needs structured inputs or outputs. Values constructed through the extension
ABI are adopted by the calling VM before the native call returns. Native calls
remain forbidden inside causal regions and observational attempt replay.

## Type Conversion

| Function | Description |
|---|---|
| `str(val)` | Convert any value to its string representation |
| `int(val)` | Convert to integer (accepts int, float→truncate, string, bool→0/1) |
| `float(val)` | Convert to float (accepts float, int, string) |
| `typeof(val)` | Return the type name as a string (`"int"`, `"float"`, `"str"`, `"bool"`, `"list"`, `"nil"`, or the name of a component/sum type like `"Result"`) |
| `variant_of(val)` | Return the variant name as a string if `val` is a sum type or state (e.g., `"Ok"`, `"Err"`), otherwise returns `nil`. The Rust VM returns canonical short names (never fully-qualified like `"Type::Variant"`). |
| `try_int(val)` | Safe int conversion — returns `Some(value)` or `None` (never errors) |
| `try_float(val)` | Safe float conversion — returns `Some(value)` or `None` (never errors) |

## Numeric

| Function | Description |
|---|---|
| `abs(n)` | Absolute value — returns `int` for int input, `float` for float input |
| `sign(n)` | -1/0/1 by sign (Math.sign semantics: 0 and NaN map to 0) — int-preserving like `abs` |
| `min(a, b)` | Smaller of two numeric values (promotes to `float` when types differ) |
| `max(a, b)` | Larger of two numeric values (promotes to `float` when types differ) |
| `clamp(x, lo, hi)` | Pin `x` to `[lo, hi]` (inclusive). Ints stay int when all three are ints; errors when `lo > hi`. Pure. |
| `int_div(a, b)` | Truncating integer division — always returns `int`, rounds toward zero. Both arguments must be `int`. Pure. |
| `round(x)` | Round to nearest integer, halves away from zero — `round(2.5)` is `3`, `round(-2.5)` is `-3`. Returns `int`. Pure. |
| `floor(x)` | Largest integer ≤ `x`. Returns `int`. Pure. |
| `ceil(x)` | Smallest integer ≥ `x`. Returns `int`. Pure. |
| `sqrt(x)` | Square root as `float`. Errors on negative input. Pure. |
| `pow(base, exp)` | Power. `int ^ non-negative int` stays `int` (overflow-checked); any float operand returns `float`. Pure. |
| `to_fixed(x, digits)` | Format a number with exactly `digits` decimal places, correct for negatives — `to_fixed(-143.9, 2)` is `"-143.90"`. Returns `str`. Pure. |
| `json_stringify(v)` | Serialize nil/bool/int/float/str, lists, maps, structs, components, and sum types (`$variant` field) to a JSON string. Errors on non-finite floats. Pure. |
| `json_parse(s)` | Parse a JSON string. Returns `Some(value)` (objects become maps, arrays become lists) or `None` on invalid JSON. Pure. |
| `rand_int(min, max)` | Random integer in the inclusive range `[min, max]` |
| `rand_float()` | Random float in the half-open range `[0.0, 1.0)` |
| `rand_bool()` | Random boolean (`true` or `false`) |
| `rand_seed(seed)` | Reseed RNG for reproducible pseudo-random streams |

Use `rand_seed(...)` when you want replayable simulation/game behavior (for example, deterministic dungeon rolls in `tests/conformance/rng_seeded_dungeon_reproducible.rad`).

> **Division (`/`) semantics:** `int / int` always returns `int` via truncation toward zero (e.g. `10 / 3 → 3`, `-7 / 2 → -3`). When either operand is `float`, the result is `float`. `int_div(a, b)` provides the same truncating behavior as a named function, useful in `map` and pipeline contexts where an operator cannot appear directly.

> **Tuple arithmetic (the vector dialect):** `+`, `-`, `*`, `/` work
> element-wise on tuples of matching arity, and `*`/`/` broadcast a scalar:
> `pos + dir * speed * dt`, `(3.0, 4.0) * 2.0`, `-v`. Promotion is per
> element; arity mismatches are errors, never truncation. Positions,
> velocities, and directions read like the math they are.

## Bits

Int-only bit operators, added for bitboard workloads (sudoku/chess-style
candidate masks, grid walkability words):

| Operator | Description |
|---|---|
| `a & b`, `a \| b`, `a ^ b` | Bitwise AND / OR / XOR. Bind tighter than comparisons, looser than arithmetic: `mask & bit == 0` means `(mask & bit) == 0`. |
| `a << n`, `a >> n` | Left / right shift, C-style precedence (looser than `+`, tighter than `&`): `base + off << 3` is `(base + off) << 3`. Logical shifts on the 64-bit pattern; a count outside `0..64` yields `0`. |
| `~a` | Bitwise NOT — flip all 64 bits. The revoke-mask idiom: `allowed & ~revoked`. |

Statement-position `xs << v` is **list append** (chainable: `xs << a << b`);
`<<` only means shift inside expressions, and the checker points you at
`push()` if a list ends up on its left there.

| Function | Description |
|---|---|
| `shl(x, n)` / `shr(x, n)` | Named forms of `<<` / `>>` — identical semantics, for pipeline/`map` contexts |
| `popcount(x)` | Number of set bits (counts all 64 for negative ints) |
| `ctz(x)` | Count trailing zeros; `ctz(0)` is 64 |

## String Operations

`reverse`, `slice`, and `contains` also work on strings (see Collections and List Operations below). Indexing into a string (`s[i]`) returns the integer byte value at that index.

Interpolation is supported directly in strings:

```
let city = "Neo Arcadia"
let pop = 1200
print("city=${city}, pop=${pop}")
```

`f"..."` strings are still supported and allow `{...}` and `${...}` placeholders.

`f"""..."""` triple-quoted f-strings support multi-line content where only `${expr}` interpolates. Bare `{` and `}` are literal text and inner `"` do not need escaping. Use `\$` to produce a literal `$` when followed by `{`.

F-string interpolations support Python-style **format specifiers** after a colon:

```
let pi = 3.14159
print(f"{pi:.2f}")           // "3.14"
print(f"{42:06d}")           // "000042"
print(f"{255:#x}")           // "0xff"
print(f"{'hello':>10}")      // "     hello"
print(f"{0.75:.1%}")         // "75.0%"
```

The format spec follows Python's mini-language: `[[fill]align][sign][#][0][width][.precision][type]`. Supported types include `d` (decimal), `f` (fixed-point), `e`/`E` (scientific), `b` (binary), `o` (octal), `x`/`X` (hex), `s` (string), and `%` (percentage). Numbers default to right-alignment; strings default to left-alignment.

### String Functions

| Function | Description |
|---|---|
| `split(str, delimiter)` | Split string by delimiter, return list of strings |
| `join(list, separator)` | Join list elements into a string with separator |
| `trim(str)` | Return string with leading/trailing whitespace removed |
| `replace(str, old, new)` | Replace all occurrences of `old` with `new` |
| `starts_with(str, prefix)` | Check if string starts with prefix |
| `ends_with(str, suffix)` | Check if string ends with suffix |
| `regex_is_match(pattern, text)` | Check whether regex `pattern` matches `text` |
| `regex_find(pattern, text)` | Return first regex match as `Some(value)` or `None` |
| `chr(code)` | Convert Unicode code point (int) to single-character string |
| `ord(str)` | Convert first character of string to its Unicode code point (int) |
| `chars(str)` | Split string into list of individual character strings |
| `to_upper(str)` | Convert string to uppercase |
| `to_lower(str)` | Convert string to lowercase |
| `format(template, ...)` | Replace `{}` placeholders with arguments in order |
| `format_value(value, spec)` | Format a single value using a Python-style format specifier string (e.g., `format_value(3.14, ".1f")` → `"3.1"`) |
| `byte_len(str)` | Return the number of UTF-8 bytes in a string |
| `byte_at(str, index)` | Return the byte value at a zero-based UTF-8 byte index |
| `substring_bytes(str, start, end)` | Slice by UTF-8 byte offsets. The range must be in bounds and must form valid UTF-8. |

## Date / Time

| Function | Description |
|---|---|
| `now_unix_s()` | Current UNIX timestamp in seconds |
| `now_unix_ms()` | Current UNIX timestamp in milliseconds |

## Collections

| Function | Description |
|---|---|
| `len(col)` | Length of a list, string, tuple, or map |
| `range(end)` | List of integers `[0, 1, …, end-1]` |
| `range(start, end)` | List of integers `[start, start+1, …, end-1]` |
| `range(start, end, step)` | List of integers from `start` to `end` (exclusive) with `step` (can be negative) |
| `contains(col, val)` | Membership test — works on lists, strings (substring), and maps (key lookup) |
| `keys(map)` | Return list of map keys (sorted deterministically) |
| `values(map)` | Return list of map values (sorted deterministically by key) |
| `entries(map)` | Return list of `[key, value]` pairs (sorted deterministically by key) |
| `merge(map1, map2)` | Return new map where keys in `map2` override keys in `map1` |
| `remove_key(map, key)` | Return a new map with the specified key removed. If the map is uniquely owned, this performs an O(1) in-place deletion. |

## List Operations

| Function | Description |
|---|---|
| `push(list, val)` | Return new list with `val` appended. Statement form: `xs << val` (chains: `xs << a << b`). |
| `set_at(coll, key, val)` | Return new list/map with one element replaced. Lists bounds-check (no silent growth); maps insert-or-replace. The expression dual of `coll[key] = val`, and what `update(e, C) { field[i] = v }` lowers to. |
| `pop(list)` | Return the last element (errors on empty list). Alias for `pop_last`. |
| `pop_last(list)` | Return the last element (errors on empty list) |
| `drop_last(list)` | Return list without the last element (errors on empty list) |
| `drop_first(list)` | Return list without the first element (errors on empty list) — the queue-advance idiom: `queue = drop_first(queue)` |
| `sort(list)` | Return sorted copy (numbers, strings, bools, tuples — tuples compare lexicographically, same order as `sort_by`) |
| `sort_by(list, key_fn)` | Return sorted copy using `key_fn(element)` to extract comparison keys. **Tuple keys compare lexicographically** — multi-key sorting is `sort_by(fn(t) { return (-t.rung, t.dist) })`. Pure, pipeline-friendly. |
| `reverse(list)` | Return reversed copy (also works on strings) |
| `slice(list, start, end)` | Return sub-list from `start` to `end` (exclusive); also works on strings |
| `append(list1, list2)` | Concatenate two lists into a new list |
| `extend(list1, list2)` | Alias for `append` |
| `zip(list1, list2)` | Pair elements from two lists into `[[a₀, b₀], [a₁, b₁], …]` (stops at shorter). Pairs naturally with destructuring: `zip(xs, ys) \|> map(fn([a, b]) { ... })` |
| `enumerate(list)` | Return a list of `[index, element]` pairs: `enumerate(["a","b"])` → `[[0,"a"],[1,"b"]]`. Use with destructuring: `for [i, val] in enumerate(items) { ... }` |

## Functional

| Function | Description |
|---|---|
| `map(list, fn)` | Transform each element, return new list |
| `filter(list, fn)` | Keep elements where `fn` returns truthy, return new list |
| `reduce(list, init, fn)` | Fold list to single value: `fn(accumulator, element)` |
| `flat_map(list, fn)` | Map then flatten — `fn` must return a list per element |
| `group_by(list, fn)` | Group elements by the key returned from `fn(item)` (str, int, bool, entity, tuple — kept as real keys), return map of lists |
| `find(list, fn)` | Return `Some(element)` for the first element where `fn` returns truthy, or `None` |
| `any(list, fn)` | True if `fn` is truthy for at least one element; short-circuits. `any([])` is `false` |
| `all(list, fn)` | True if `fn` is truthy for every element; short-circuits. `all([])` is `true` |
| `max_by(list, fn)` | Return `Some(element)` with the largest key from `fn(element)`, or `None` for empty lists |
| `min_by(list, fn)` | Return `Some(element)` with the smallest key from `fn(element)`, or `None` for empty lists |
| `sum(list)` | Numeric fold: total of all elements. Ints stay int, any float promotes; `sum([])` is `0` |
| `product(list)` | Numeric fold: product of all elements; `product([])` is `1` |
| `get_or(coll, key, default)` | Map lookup or list index with a fallback instead of nil/bounds-error — the cooldown-table read: `cds \|> get_or("q", 0)` |
| `index_of(list, v)` | First index holding `v` (structural equality), or `-1`. An int rather than an Option because the consumer is slot arithmetic: `if at >= 0 { set_at(slots, at, nil) }` |

**Accessor shorthand:** anywhere a one-argument closure is expected, `.field`
projects that field — `mods |> map(.flat) |> sum` instead of
`mods |> map(fn(m) { return m.flat }) |> sum`. Chains reach through nesting:
`units |> map(.stats.hp)`.

**Readonly callbacks:** pipeline callbacks may READ the world — closures and
unannotated functions whose bodies only call readonly builtins
(`get`/`has`/`require`/`name_of`/queries) infer the readonly effect and
compose into `map`/`filter`/`sort_by`/`min_by` without a `readonly fn`
annotation: `units |> sort_by(fn(u) { return (-points_of(u), name_of(u)) })`.

**Filtered loops:** `for m in mods where m.stat == "ad" { ... }` is sugar for
wrapping the body in `if` — same truthy condition rules, reads like the query
it is.

## BitSet

A dynamically-growing bit set for O(1) integer membership testing. Ideal for
bookmarks, line flags, visited-node tracking, or any use case where you need
fast `contains` on integer keys without the overhead of a hash map.

Memory usage is ~1 bit per index: an 80,000-line file's bookmark set uses ~10 KB.

| Function | Description |
|---|---|
| `bitset_new()` | Create a new empty bit set |
| `bitset_set(bs, index)` | Return a new bitset with bit at `index` set (grows automatically) |
| `bitset_has(bs, index)` | Return `true` if bit at `index` is set, `false` otherwise — O(1) |
| `bitset_clear(bs, index)` | Return a new bitset with bit at `index` cleared |

```
let mut bookmarks = bitset_new()
bookmarks = bitset_set(bookmarks, 42)
bookmarks = bitset_set(bookmarks, 1337)

print(bitset_has(bookmarks, 42))    // true
print(bitset_has(bookmarks, 100))   // false

bookmarks = bitset_clear(bookmarks, 42)
print(bitset_has(bookmarks, 42))    // false
```

> **When to use BitSet vs list `contains`:** Use `bitset_has` when checking
> integer membership repeatedly — it is O(1) per lookup. `contains(list, val)`
> performs a linear scan and is O(n). For keyword sets or string membership,
> consider using a `map` with dummy values, which provides O(1) string-key lookup.
>
> **Note on Mutability:** `bitset` uses strict value semantics like `list` and `map`. `bitset_set` and `bitset_clear` are pure functions that return a new bitset. However, the compiler performs static escape analysis: if your bitset is uniquely owned (e.g. a local variable that never escapes), mutations are compiled to $O(1)$ in-place updates.

## String Buffers

Buffers are value-semantic string builders for tight append loops. The compiler
can optimize a non-escaping local reassignment pattern, but the surface model
stays functional: each append returns the next buffer value.

| Function | Description |
|---|---|
| `buffer_new()` | Create an empty string buffer |
| `buffer_append(buffer, str)` | Return a buffer with `str` appended |
| `buffer_to_str(buffer)` | Convert a buffer to a string |

```rad
let mut b = buffer_new()
b = buffer_append(b, "hp=")
b = buffer_append(b, str(42))
print(buffer_to_str(b))    // "hp=42"
```

## Byte Buffers

`bytebuf` is a native byte buffer for binary packet encode/decode. It has value
semantics at the language surface, and the compiler lowers non-escaping local
reassignment patterns to in-place writes.

| Function | Description |
|---|---|
| `bytebuf_new(size)` | Create a zero-filled byte buffer |
| `bytebuf_len(buf)` | Return the byte length |
| `bytebuf_get(buf, index)` | Read one byte as an int `0..255` |
| `bytebuf_set_u8(buf, index, value)` | Return a buffer with one byte written |
| `bytebuf_set_u32_le(buf, offset, value)` | Return a buffer with a little-endian unsigned 32-bit int written |
| `bytebuf_set_i32_le(buf, offset, value)` | Return a buffer with a little-endian signed 32-bit int written |
| `bytebuf_get_u32_le(buf, offset)` | Read a little-endian unsigned 32-bit int |
| `bytebuf_get_i32_le(buf, offset)` | Read a little-endian signed 32-bit int |
| `bytebuf_to_list(buf)` | Convert to `list<int>` for compatibility/tests |
| `bytebuf_from_list(bytes)` | Convert `list<int>` byte values into a byte buffer |

```rad
fn encode_move(client_seq: int, target_x: float, target_y: float) -> any {
    let mut packet = bytebuf_new(15)
    packet = bytebuf_set_u8(packet, 0, 77)
    packet = bytebuf_set_u8(packet, 1, 4)
    packet = bytebuf_set_u8(packet, 2, 2)
    packet = bytebuf_set_u32_le(packet, 3, client_seq)
    packet = bytebuf_set_i32_le(packet, 7, round(target_x * 1000.0))
    packet = bytebuf_set_i32_le(packet, 11, round(target_y * 1000.0))
    return packet
}
```

## ECS

| Function | Description |
|---|---|
| `spawn([name], components...)` | Create a new entity with optional name and components, return its ID |
| `despawn(id)` | Destroy an entity, clear its data, and recycle its ID |
| `get(id, Component)` | Get component — returns `Some(value)` or `None` |
| `require(id, Component)` | Get component and fail fast if missing (returns component directly) |
| `require_all(id, Component...)` | Get multiple required components, fail fast on first missing component |
| `set(id, Component{...})` | Set component on entity (use `..base` spread to avoid retyping unchanged fields) |
| `has(id, Component)` | Check if entity has component |
| `remove(id, Component)` | Remove component from entity |
| `entities([ComponentName...])` | Return all entity IDs, or only entities that have all listed component types |
| `name_of(id)` | Entity's declared name (empty string if unnamed). Readonly. |
| `get_entity(name)` | Lookup by name — returns `entity \| nil`; narrow with a guard (`if e == nil { return }`). Readonly. |
| `require_entity(name)` | Fail-fast lookup by name — returns `entity`, errors if missing (the get/require pairing, extended to names). Readonly. |
| `id_of(id)` | Entity's stable integer id. Pure — usable in `pure fn`. Entities also sort by ascending id: `query { C } \|> sort` is the canonical deterministic order. |
| `query_where(ComponentName..., fn)` | Filter entities having the given components using a predicate evaluated on the entity ID. The predicate may be **pure or read-only** — `get`/`res`/`has`/`readonly fn` calls are allowed (the entity list is snapshotted before the predicate runs), so filtering by component values is direct: `query_where(Hero, fn(id) { return (get(id, Hero) \|> unwrap).level >= 3 })`. Writes, IO, and events in the predicate are compile errors |
| `query_map(ComponentName..., fn)` | Map over entities having the given components using a function evaluated on the entity ID. Same contract as `query_where`: the mapper may be **pure or read-only** (world reads and `readonly fn` calls allowed); writes, IO, and events in the mapper are compile errors |
| `query_count(ComponentName...)` | Return the number of entities having the given components |
| `with_field(entities, ComponentName, FieldName, fn)` | Filter a list of entities by evaluating a predicate function on a specific component field |
| `lookup(ComponentName, field_name, value)` | O(1) indexed lookup: returns `Some(entity_id)` for the **lowest-id** entity whose `indexed` field matches `value`, or `None`. The field must be declared `indexed` in the component. |
| `lookup_all(ComponentName, field_name, value)` | Every entity whose `indexed` field matches `value`, ids ascending — the multi-match query ("all open tickets") as one hash probe instead of an O(world) scan. |
| `get_resource(ResourceType)` | Get global resource — returns `Some(value)` or `None`. Readonly. |
| `res(ResourceType)` | Direct resource access — returns the value itself, no Option. Declared resources auto-initialize from their field defaults, so `res(R)` never misses; the checker types `res(R).field` precisely and rejects components/unknown names. Readonly. |
| `set_resource(ResourceType, value)` | Set global resource value. Mutating. |

### `update` statement

**Component form:** `update(entity, Component) { field = expr, ... }` is syntactic sugar for reading the current component, overriding the listed fields, and writing back. The entity expression is evaluated exactly once. Field types are validated against the component's declaration.

```
component Score { points: 0, level: 1 }
let e = spawn(Score { points: 0, level: 1 })

update(e, Score) {
    points = 50,
    level = 2
}
```

This is equivalent to `set(e, Score { points: 50, level: 2, ..unwrap(get(e, Score)) })`, but shorter and less error-prone.

**Resource form:** `update(ResourceType) { field = expr, ... }` works the same way but for global resources declared with the `resource` keyword. No entity is needed.

```
resource Config { gravity: 9.81, debug: false }

update(Config) {
    debug = true
}
```

The checker rejects `update(entity, Resource)` (resources are not per-entity) and `update(Resource)` inside a system that holds the same resource as a `mut` parameter (the writeback would overwrite the update).

### Indexed lookup semantics

Declare a field `indexed` to maintain a runtime hash index
(`component Ticket { indexed status: "" }`). The index is maintained
through `spawn`/`set`/`update`/`remove`/`despawn`, survives `fork`/`commit`
rewinds, the wire codec (`fork_from_bytes` + `commit`), `save_world`/
`load_world`, `fork_apply`, `merge_forks`, and schema migration — the
program's `indexed` declarations are the source of truth, and `commit()`
reconciles any snapshot that arrived without index data (old saves,
foreign forks). Pinned semantics, chosen for determinism:

- With duplicate keys, `lookup` returns the **lowest entity id** and
  `lookup_all` returns ids **ascending** — both stable across save/load
  round trips and record/replay.
- Float keys are **bit-pattern** keys: `0.0` and `-0.0` are distinct
  buckets, and an int probe never matches a float key. Hashability costs
  exactness; the trade-off is documented rather than hidden.
- `lookup`/`lookup_all` on a field not declared `indexed` is a loud
  runtime error, never a silent scan.

**Effect classification:** The ECS read builtins (`get`, `has`, `entities`, `query_where`, `query_map`, `query_count`, `with_field`, `peek`, `lookup`, `lookup_all`, `get_resource`, `res`) carry the `readonly` effect — they read world state but never mutate it. This means they are **allowed inside pipeline expressions** (`|>`), unlike mutating builtins (`set`, `spawn`, `set_resource`, `load_world`, …) and IO builtins (`print`, `log`, `sleep_ms`, file/network access), which are rejected in pipelines — as direct stages and inside callbacks alike. User-defined functions that only call `readonly` builtins can be declared as `readonly fn` and also used in pipelines.

### Entity literal expressions

When all components are known up front, an **entity literal expression** can replace `spawn()` + multiple `set()` calls:

```
let hero = entity {
    Name { value: "Hero" },
    Health { hp: 100, max: 100 },
    Position { x: 0.0, y: 0.0 }
}
```

An optional **name expression** between `entity` and `{` creates a named entity, replacing the `spawn("name") + set()` pattern:

```
let e = entity "player" { Health { hp: 100 }, Position { x: 0, y: 0 } }
let found = require_entity("player")   // fail-fast lookup (entity)
let maybe = get_entity("player")       // fallible lookup (entity | nil)

// The name can be any string expression:
let file = entity path { FilePath { path: path }, Unparsed {} }
let npc = entity f"npc_{id}" { Name { value: n } }
```

The expression spawns an entity (named or anonymous), attaches every listed component, and evaluates to the entity ID (type `entity`). It works anywhere an expression is expected — let-bindings, function arguments, return values.

Component entries can be traditional initializers (`Component { field: value }`) or arbitrary **expressions** — variables, function calls, or any expression evaluating to a component value:

```
let pos = Position { x: 1.0, y: 2.0 }
let hero = entity { Name { value: "Hero" }, pos, make_health(100) }
```

See the [ECS guide](../guide/ecs.md#entities) and [language spec](spec.md) for details.

Use `spawn` + `set` when you need to add components conditionally or incrementally over time.

### Component spread syntax

When updating a component, use `..base` to copy unchanged fields from an existing value:

```
component Health { hp: 100 }
let hero = spawn(Health { hp: 100 })

let h = get(hero, Health)?
set(hero, Health { hp: h.hp - 10, ..h })
```

Explicit fields override the spread base. The type checker ensures the base is the same component type.

### Default-fill in literals

Fields whose declaration carries a usable default — `field: value` or
`field: Type = value` — may be **omitted from literals**; the constructor
fills them from the declaration:

```rad
component Incident { title: "", priority: 0, status: "open" }

let i = spawn(Incident { title: "disk full" })   // priority: 0, status: "open"
```

Bare type annotations (`x: float`) have no default and stay required. This
is *constructor* semantics: `Incident { title: "t" }` is always a complete
value with defaults filled — to change one field of an **existing** component
use `update(e, Incident) { status = "closed" }` or spread syntax, not `set`
with a partial literal (which would reset the other fields to defaults).

## State Machines

| Function | Description |
|---|---|
| `transition(state, event)` | Attempt transition — returns `Ok(value)` or `Err(message)` |

## Error Handling

**Postfix `?` (try):** After an expression of type `Option<T>` or `Result<T, str>`, `expr?` unwraps the success value or returns early from the **current function** with `None` / `Err`. The function’s return type must allow that propagation (for example `fn main() -> any`, `fn main() -> nil`, or an explicit `Option<...>` / `Result<...>` return type). `fn main() -> nil` is special-cased: `?` propagation exits the program cleanly instead of producing a type error. Prefer `?` over `unwrap` when missing data should propagate rather than panic.

| Function | Description |
|---|---|
| `unwrap(val)` | Extract value from `Some` or `Ok`; runtime error on `None` or `Err` |
| `unwrap_or(val, default)` | Extract value from `Some` or `Ok`; return `default` on `None`/`Err` (no runtime error) |
| `map_or(val, default, fn)` | If `val` is `Some`/`Ok`, return `fn(inner)`; otherwise return `default` |
| `expect(val, msg)` | Same as `unwrap` but uses `msg` in the error message on failure |
| `is_some(val)` | Return `true` if `Some` or `Ok`, `false` otherwise. Pure. |
| `is_none(val)` | Return `true` if `None` or `Err`, `false` otherwise. Pure. |

## Testing

| Function | Description |
|---|---|
| `assert(condition, msg)` | Assert condition is true; runtime error with `msg` on failure |
| `assert_eq(a, b)` | Assert two values are equal; runtime error on mismatch |

## Test Data Generation

| Function | Description |
|---|---|
| `gen_int()` | Generate a deterministic list of test integers (property-style generator) |
| `gen_float()` | Generate a deterministic list of test floats (property-style generator) |
| `gen_str()` | Generate a deterministic list of test strings (property-style generator) |
| `gen_bool()` | Generate a deterministic list of test booleans (property-style generator) |
| `gen_list(list)` | Generate a list of test lists from a seed list |

`gen_*` functions are for deterministic test input generation, **not randomness**.
Use `rand_*` functions when you need pseudo-random values at runtime.

## World Forking (Speculative Execution)

| Function | Description |
|---|---|
| `fork()` | Snapshot the **full program state** — ECS world *and* the in-flight event queue — returning a `world_fork` value |
| `simulate(fork, systems, ticks)` | Run `systems` on the forked world for `ticks` iterations, returning the updated fork. The fork's pending events fire inside the simulation; whatever it leaves in flight travels with the result |
| `simulate_par(fork, systems, ticks, n, seed, overrides?)` | Run `n` independent simulations of the same fork in parallel, returning a list of result forks. Deterministic: each fork's RNG seed is derived from `seed` and its index, so results are identical regardless of thread count. The optional 6th argument is a **list of resource values** applied to the fork before the rollouts (`[Policy { tax: 8 }]`) — seed a candidate at the call site without `commit()` |
| `simulate_many(forks, systems, ticks, seed)` | The heterogeneous sibling of `simulate_par`: run each of the **distinct** forks in the list in parallel under the same schedule, returning a list of result forks (one per input, same order). Per-fork seeds derive from `(seed, index)`, so results are deterministic regardless of thread count. This is the axis a search wants — evaluate B×K candidate worlds at once |
| `simulate_seeded(fork, systems, ticks, raw_seed)` | ONE rollout under an **exact** RNG seed — no per-index derivation. Feed it `fork_seed(f)` of a `simulate_par`/`simulate_many` result and it reproduces that single rollout bit-identically, without paying for the others |
| `fork_with(fork, resource_value)` | Return a copy of `fork` with one resource overridden (e.g. `fork_with(f, Policy { rate: 8 })`) — seed a speculative candidate **without** `commit()`ing to the live world. Events, timers, and entities ride through untouched, so it composes straight into `simulate`/`simulate_par`/`simulate_many` |
| `fork_seed(fork)` | The effective RNG seed the rollout that produced this fork ran under, or `0` for any fork that is not a simulate-family result (derived seeds are never 0). Local debug metadata: not serialized to the wire, and cleared by `fork_with` (an overridden copy is a new candidate, not a rollout's output) |
| `commit(fork)` | Replace the live program state with the fork's — world **and** pending events, exactly as captured |
| `peek(fork, entity, Component)` | Read a component value from a fork without committing |
| `peek_resource(fork, Resource)` | Read a resource value from a fork without committing — `Some(value)` or `None`. Reads simulated scores/clocks straight off result forks. |
| `sandbox_run(source, fork, caps_json, input?)` | Compile and run untrusted RAD source against a fork inside a capability-bounded guest VM. Returns `Result<world_fork, str>`. The optional 4th argument is data-only input the guest reads back with `sandbox_input()` |
| `sandbox_input()` | (Inside a sandboxed guest) the data-only input provided by the host, or `nil` |
| `sandbox_output(v)` | (Inside a sandboxed guest) report a structured, data-only result to the host. Serialized to JSON immediately; last call wins |
| `sandbox_last_output()` | (In the host) the structured value the most recent `sandbox_run` guest reported via `sandbox_output(v)`, parsed back onto the host heap, or `nil` if it reported none |
| `sandbox_last_fuel()` | (In the host) fuel consumed by the most recent `sandbox_run` (0 before any run) — the metering signal for billing or rate-limiting a plugin |
| `diff(fork_a, fork_b)` | Per-component changed-row counts between two forks, as a `map<str, int>`. O(archetypes) `Arc` pointer comparisons, not a world scan |
| `assert_only_changed(fork_a, fork_b, allowed)` | Runtime error unless every difference between the forks is in the `allowed` component list. Accepts component type refs (`Health`) or name strings (`"Health"`) |

`fork()` captures a copy-on-write snapshot of the ECS (entities, components, resources, archetypes) in O(A)
`Arc` refcount bumps. Mutation after a fork pays for copy only on touched data: `Arc::make_mut`
clones the `ValueColumn`, running an O(E) retain scan for persistent `Arc<Object>` refs (see [Architecture](./architecture.md)).

**Events are program state**, so they fork like everything else: `fork()`
captures the pending event queue alongside the world (payloads persisted,
causality ids included), and `commit()` restores it — events emitted after
the fork are rewound with the world, events pending at the fork fire when
you next `flush_events()`. A snapshot that dropped them would not be a
snapshot.

`simulate()` temporarily swaps in the fork — world *and* event queue — runs
the listed systems, and produces a new fork without touching the real world.
Events emitted inside the simulation enqueue on the simulation's own
timeline: they fire on later simulated ticks, and anything still in flight
at the end travels with the result fork (peekable, committable, mergeable).
You can chain `simulate()` on its result for multi-phase prediction:
`simulate(simulate(f, [system::A], 3), [system::B], 2)`.

`commit()` atomically replaces the live program state with the fork's. This
is not reversible — if you need to inspect a fork before committing, use
`peek()`. After a commit, `why()` honestly discloses the seam: writes made
*inside* a fork are not in the causality ledger, so explanations note when
the current value may originate from the committed timeline.

`peek()` reads a single component from the forked world without committing or modifying
any state. Returns an `Option` — `None` if the entity or component doesn't exist in the fork.
Values are deep-copied across the air gap (O(F) for F fields; string fields are O(1) via `Arc<str>`).

The type checker statically prevents systems that perform IO, call `commit()`, call unsafe
event-effect operations such as `transition`, or reach unsafe handler chains from being used
inside `simulate()`. `emit` statements are allowed: their handlers dispatch on the fork's
event queue, isolated from the live timeline. This includes `rand_*`: a plain fork carries no explicit
seed, so randomness inside `simulate()` would not be reproducible. If a speculated system
needs randomness, use `simulate_par()` — its forks are explicitly seeded, so the checker
permits `rand_int`/`rand_float`/`rand_bool` there (and only there; `rand_seed` stays banned,
since re-seeding would collapse the per-fork divergence).

### Parallel exploration: `simulate_par`

`simulate_par(fork, [system::A, system::B], ticks, n, seed)` explores `n` futures of the same
starting fork on a worker-VM pool (one VM per thread, snapshots restored via CoW `Arc` bumps).
Each fork gets an RNG seed derived from `(seed, fork_index)` with a SplitMix64 finalizer, so
runs are **bit-identical for the same inputs at any thread count**. Use it to score
alternative strategies and `commit()` the winner:

```rad
let futures = simulate_par(fork(), [system::Economy], 10, 8, 42)
let best = max_by(futures, fn(f) { return (peek(f, kingdom, Treasury) |> unwrap).gold })
commit(best)
```

For **search** — where the interesting axis is different *candidate policies*, not repeated
rollouts of one — pair `fork_with` with `simulate_many`. `fork_with` seeds each candidate off a
shared root fork without mutating the live world, and `simulate_many` evaluates them all in
parallel:

```rad
let root = fork()
let candidates = [
    fork_with(root, Policy { rate: 1 }),
    fork_with(root, Policy { rate: 5 }),
    fork_with(root, Policy { rate: 10 }),
]
let futures = simulate_many(candidates, [system::Economy], 10, 42)
let best = max_by(futures, fn(f) { return (peek(f, kingdom, Treasury) |> unwrap).gold })
commit(best)   // the ONLY write to the live world in the whole search
```

Because `fork_with` never commits, a purely speculative tree search leaves the live world
bit-identical to where it started — no `commit`/mutate/fork dance, and nothing stranded if the
search is abandoned midway.

When the candidates only differ in resource values, `simulate_par`'s optional override list
does the seeding inline — `simulate_par(root, SYSTEMS, 10, 6, 42, [Policy { rate: 5 }])` runs
six rollouts of the root with `Policy` overridden, and the live world is never written.

**Reproducing one rollout.** Every simulate-family result knows which effective RNG seed
produced it. When rollout 4 of 6 is the outlier that decides a candidate's worst-case score,
pull its seed and re-run exactly that future in isolation:

```rad
let outs = simulate_par(root, SYSTEMS, 10, 6, 42)
let outlier_seed = fork_seed(outs[4])              // never 0 for a rollout result
let again = simulate_seeded(root, SYSTEMS, 10, outlier_seed)
// `again` is bit-identical to outs[4] — one rollout's cost, not six.
```

The seed answers "which future is this" (`fork_seed(fork())` is `0` — only simulate-family
results carry one), and `simulate_seeded` is the consumer that makes it actionable: the pair
turns "the SET of rollouts is reproducible" into "each individual rollout is reproducible".

### Blast-radius assertions: `diff` and `assert_only_changed`

Tests normally assert what changed — never what *didn't*. Because forks are CoW snapshots of
100% of program state, RAD can check the negative space cheaply:

```rad
let before = fork()
emit Hit { amount: 25 }
flush_events()
assert_only_changed(before, fork(), [Health])   // error if ANYTHING else changed
```

`diff(fork_a, fork_b)` returns a `map<str, int>` of component/resource type name → number of
changed rows (an upper bound: a freshly cloned column counts all its rows). Comparison is
O(archetypes) `Arc::ptr_eq` checks on CoW columns — untouched data is never scanned, so
diffing two forks of a million-entity world where one component changed costs roughly
the number of archetypes, not a million.

`assert_only_changed(fork_a, fork_b, allowed)` raises a runtime error naming the unexpected
components and their row counts:

```text
assert_only_changed() failed: unexpected changes to [Gold (1 rows)] (allowed: [Health])
```

Spawns, despawns, component removals, and `set_resource` writes all show up in the diff,
since each structurally changes a column or the entity table.

### The speculation sandbox: `sandbox_run`

`sandbox_run(source, fork, caps_json, input?)` runs **untrusted code** (AI-generated plans,
mods, plugins) against a forked world inside a fresh guest VM. The guest never sees the live
world; the host inspects the returned fork with `peek()` and decides whether to `commit()`.

The optional `input` argument is serialized to JSON at the boundary and surfaces inside the
guest as `sandbox_input()` — pass identity and parameters through this typed, data-only
channel instead of splicing host values into the guest's source text:

```rad
match sandbox_run(bot_src, fork(), caps, { "unit": name, "round": n }) {
    Ok(f) => { /* guest read it via sandbox_input()["unit"] */ }
    Err(m) => { print(m) }
}
```

Capability grant format:

```json
{ "read": ["Reactor"], "write": ["Reactor"], "fuel": 1000000, "mem_bytes": 16777216, "seed": 7 }
```

- `write` — component types the guest may write via `set`/`spawn`/`set_resource`/system
  writebacks. Empty (the default) denies all writes; `"*"` grants everything, and is required
  for `despawn`.
- `read` — component/resource types the guest may **read** via `get`/`res`/`require`/`has`/
  `lookup`/`query`/`query_where`/`query_count`/`query_map`/`with_field`/`why`/`entities(C…)`
  and read (non-`mut`) system parameters. **Omitting the key grants read of everything** (`"*"`),
  so pre-existing grants are unchanged; an explicit list is an allowlist and an explicit `[]`
  reads nothing. The **whole-world readers** — `save_world()`, `world_digest()` (no-arg), and
  unfiltered `entities()` — cannot be keyed to one component and require the `"*"` read grant,
  the same way `despawn` requires the `"*"` write grant.
- `fuel` — instruction budget, charged on loop back-edges and calls (default 10M).
- `mem_bytes` — allocation ceiling (default 64 MiB).
- `seed` — guest RNG seed (deterministic by default).

Four enforcement layers apply: a deny-by-default **builtin mask** (no file/network/clock/
process access, no `fork`/`commit`/`sandbox_run` nesting — `commit` is never grantable), the
**component-write ACL** and the symmetric **component-read ACL** above, and the **fuel/memory
budgets**. Module imports are rejected at compile time. Any failure — a malformed capability
grant, guest compile error, capability denial, budget exhaustion — returns `Err(message)`
instead of aborting the host, so a grant computed from an untrusted plugin manifest cannot
crash the host either.

> **Confidentiality vs. integrity.** The `write` ACL together with `diff()`/
> `assert_only_changed()` is an *integrity* boundary — it bounds what a guest can change. The
> `read` ACL is the *confidentiality* boundary — it bounds what a guest can learn. A grant that
> omits `read` (or sets `"read": ["*"]`) is integrity-only: the guest can read every component
> and resource in the forked world and publish it through `print`/`sandbox_output`. If the host
> holds secrets the plugin should not see, name the readable types explicitly, e.g.
> `{ "read": ["Reactor"], "write": ["Reactor"] }`.

Unlike `simulate()`, events emitted by the guest are **not** dropped: the guest VM owns
private double-buffered event queues, so its handlers run normally inside the closed world
(captured-events mode), and pending events are drained after the guest's main completes.
Guest `print` output is surfaced to the host prefixed with `[sandbox]`.

```rad
let proposal = f"""
component Morale { level: 50 }
set(get_entity("kingdom"), Morale { level: 80 })
"""
let caps = f"""{ "write": ["Morale"], "fuel": 1000000 }"""

match sandbox_run(proposal, fork(), caps) {
    Ok(value)  => {
        let m = peek(value, kingdom, Morale) |> unwrap
        if m.level <= 100 { commit(value) }
    }
    Err(message) => { print(f"proposal rejected: {message}") }
}
```

`sandbox_run` returns only `Result<world_fork, str>`, but a guest can report a structured
result with `sandbox_output(v)` and always spends fuel. After the call, the host reads both
back from the calling VM — no need to make the guest WRITE state just to communicate, and no
need to parse `print` text:

```rad
match sandbox_run(bot_src, fork(), caps, { "round": n }) {
    Ok(f)  => {
        let plan = sandbox_last_output()   // the guest's sandbox_output(v), or nil
        let cost = sandbox_last_fuel()     // fuel spent — meter / bill / rate-limit on this
        if cost < budget { /* score `plan` on its own terms, then commit(f) */ }
    }
    Err(m) => { print(f"rejected after {sandbox_last_fuel()} fuel: {m}") }
}
```

Both accessors reflect the **most recent** `sandbox_run` on that VM (fuel is `0` and output is
`nil` before the first). They read host-side state the runtime already held; the same
telemetry is available to JSON-RPC clients as the `out` and `fuel_spent` fields of a `propose`
response.

See `projects/dogfood/speculation/main.rad` for a complete host/guest demo including hostile-proposal
deflection.

### Serving the sandbox to agent frameworks: `rad sandbox serve`

```bash
rad sandbox serve [host.rad] [--caps caps.json]
```

Starts a JSON-RPC 2.0 server over stdio (one JSON object per line) so external processes —
agent frameworks, orchestrators, anything that can pipe JSON — can drive the
speculate-inspect-commit loop against a live RAD world. `host.rad` (trusted) initializes the
world; `--caps` sets the default grant for proposals (overridable per request). Host program
output goes to stderr; stdout carries only protocol lines.

| Method | Params | Result |
|---|---|---|
| `propose` | `{source, input?, caps?}` | `{ok, fork_id, out, diff, fuel_spent, prints}` — or `{ok: false, error, fuel_spent, prints}` on guest failure |
| `peek` | `{fork_id, entity, component}` | `{found, fields}` (`entity` is a name string or id number) |
| `commit` | `{fork_id}` | `{committed: true}` — replaces the live world |
| `drop` | `{fork_id}` | `{dropped: bool}` |
| `shutdown` | — | `{bye: true}` and the server exits |

- `input` crosses a **data-only boundary**: the guest reads it with `sandbox_input()` and
  reports structured results with `sandbox_output(v)` (the `out` field). No closures, no
  heap values — JSON in, JSON out.
- `diff` is a cheap per-component changed-row summary computed by `Arc::ptr_eq` on CoW
  columns — O(archetypes), not O(entities) — so an agent can see the blast radius of its
  proposal (`{"Treasury": 1, "Morale": 1}`) without scanning the world.
- Guest failures (capability denials, budget exhaustion, compile errors) come back as
  `ok: false` with the error message and fuel accounting — the diagnostics an agent needs
  to retry.

Example session (`projects/dogfood/speculation/serve_session.jsonl` piped into the server):

```text
→ {"id":1,"method":"propose","params":{"source":"...","input":{"spend":300,"gain":30}}}
← {"id":1,"result":{"ok":true,"fork_id":1,"out":{"new_gold":700,"new_morale":80},
                    "diff":{"Morale":1,"Treasury":1},"fuel_spent":9,"prints":[]}}
→ {"id":2,"method":"peek","params":{"fork_id":1,"entity":"kingdom","component":"Morale"}}
← {"id":2,"result":{"found":true,"fields":{"level":80}}}
→ {"id":4,"method":"commit","params":{"fork_id":1}}
← {"id":4,"result":{"committed":true}}
```

### Record & replay: `rad run --record`

```text
rad app.rad --record trace.radr
```

Records an execution trace sufficient to reproduce the run bit-for-bit. RAD records
**inputs, not state**: because the interpreter is deterministic (enforced by a permanent
determinism test suite), the trace only needs the values that cross the determinism
boundary —

- the initial RNG seed (header),
- every io builtin result (`read_file`, `http_get`, `input`, `tcp_*`, …) including failures,
- every clock read (`clock`, `now_unix_s`, `now_unix_ms`).

`rand_int` is *not* recorded (pure xorshift off the seed), prints are *not* recorded
(deterministic outputs), and *recordable* io inside `simulate()`/sandboxes cannot exist:
effectful builtins (`read_file`, `http_get`, clocks, …) are statically banned in
simulation schedules and capability-denied in sandboxes, so no value crossing the
determinism boundary originates there. The one thing that *can* still reach the terminal
from inside `simulate()` is a **ghost effect** — `debug_trace()` writes to stderr but is
treated as pure by the typechecker (see §8 of `guarantees.md`). Ghost output is diagnostic
only: it is never recorded, carries no state, and may be elided under `--release`, so it
does not affect reproducibility. A full game session compresses to a few KB of JSONL:

```text
{"t":"header","version":1,"source_hash":"7fdf…","seed":9685449212088958497}
{"t":"io","f":0,"s":1,"b":"read_file","a":"51d1c937a7c98452","r":{"t":"str","v":"goblin:10,…"}}
{"t":"frame","n":0}
```

Each io record carries `f`/`s` (frame/sequence coordinates — frames are main-timeline
`flush_events` flips; speculative flushes inside `simulate()` don't advance the clock) and
`a`, a digest of the arguments. Traces are **self-contained**: the header embeds the full
authenticated module bundle, including resolved import edges, and the final record carries
both a blake3 content digest of the world and the terminal success/error outcome. Traces are
written even when the run crashes — a trace of the crash is the point.

### Replaying: `rad replay`

```text
rad replay trace.radr [--to-frame <n>] [--force]
```

Re-executes the recorded run **bit-for-bit** from nothing but the trace file. Replay-managed
builtins never execute — `read_file` returns the recorded payload even if the file was
deleted, `http_get` replays the recorded response without touching the network, and a
recorded crash is reproduced verbatim. The RNG is rewound to the recorded seed; everything
else replays for free because the interpreter is deterministic.

Three protection layers, all loud:

1. **Integrity** — embedded sources, module identities, resolved import edges, and language
   features are authenticated; a tampered trace is refused (override with `--force`).
2. **Divergence detection** — every replayed io call is checked against the trace (builtin
   name, argument digest, frame coordinate). A mismatch halts with
   `replay divergence at frame N, record #K: …` instead of debugging a timeline that never
   happened.
3. **End-to-end verification** — after replay, both the world content digest and terminal
   success/error outcome are compared. An early crash cannot verify merely because it left
   the same empty world: `Replay verified: world digest matches the recorded run` is printed
   only after the outcome check also succeeds.

`--to-frame N` halts at the start of frame `N` (handlers dispatched by the k-th
`flush_events` belong to frame k), leaving the world exactly as it was mid-history.

### Time travel: `rad replay --serve`

```text
rad replay trace.radr --serve
```

Time-travel debugging as an **API**. On startup the server replays the trace once,
keyframing the world at *every* frame boundary — affordable only because snapshots are
CoW `Arc` bumps, O(archetypes) each. After that single pass there is no re-execution:
`goto_frame` is index movement and every query reads a snapshot. JSON-RPC 2.0 over stdio,
same wire protocol family as `rad sandbox serve`:

| Method | Params | Result |
|---|---|---|
| `info` | — | `{frames, io_records, current, verified, run_error?}` |
| `goto_frame` | `{frame}` | `{frame, digest}` — moves the cursor |
| `peek` | `{entity, component, frame?}` | `{found, fields}` at the cursor or an explicit frame |
| `diff_frames` | `{a, b}` | `{diff: {Component: rows}}` — blast-radius diff pointed backwards in time |
| `why` | `{entity, component, frame?}` or `{resource, frame?}` | `{why}` — the causal chain of the value as of that frame |
| `shutdown` | — | `{bye: true}` |

Frame addressing: index `k` = world at the start of frame `k`; the highest index is the
world at program end. Crashed traces serve their timeline too (`run_error` in `info`) —
the crash state is addressable.

The agent bug-bisection loop (`projects/dogfood/timetravel/bisect_session.jsonl`):

```text
→ {"id":3,"method":"diff_frames","params":{"a":2,"b":3}}
← {"result":{"diff":{"Health":1}}}                          // Gold intact here…
→ {"id":4,"method":"diff_frames","params":{"a":3,"b":4}}
← {"result":{"diff":{"Gold":1,"Health":1}}}                 // …the drain is in frame 3
→ {"id":5,"method":"peek","params":{"frame":3,"entity":"hero","component":"Gold"}}
← {"result":{"found":true,"fields":{"amount":50}}}
→ {"id":6,"method":"peek","params":{"frame":4,"entity":"hero","component":"Gold"}}
← {"result":{"found":true,"fields":{"amount":0}}}           // bad transition confirmed
```

This is where the speculation sandbox (#1), record & replay (#2), and blast-radius diffs
(#3) converge: one wire protocol for proposing futures, replaying pasts, and diffing any
two points on either timeline.

### Causality queries: `why()` and `why_resource()`

```rad
print(why(hero, Gold))          // -> str: the causal chain of the value
print(why_resource(Treasury))
```

"Why does this value exist?" as a runtime primitive. The VM keeps a
provenance ledger of every main-timeline write — who wrote it (top-level
code, a system, or an event handler) — and every event emission. Handler
causes link to the exact emit record of the event *instance* they were
handling, so the chain is causal, not merely correlated:

```text
Gold of hero = { amount: 0 }   (set in frame 4)
  <- by `on Hit` handler
  <- Hit { amount: 10 } emitted in frame 3
  <- by top-level code
```

Chains cross as many event hops as it takes (`set_resource` <- `on Drained`
<- `Drained` emitted by `on Hit` <- `Hit` emitted by top-level), and cover
`set`, `spawn`, `remove`, `despawn`, system writebacks (sequential *and*
parallel batches), and resource writes. Writes inside `simulate()` forks and
sandbox guests are deliberately invisible — speculative values never become
"this value".

The same question works backwards in time: the `why` method on
`rad replay --serve` answers from the ledger rebuilt during the replay pass,
**at any frame** — `why {frame: 3, entity: "hero", component: "Gold"}` says
"spawned, top-level", while frame 4 returns the full drain chain. One call
replaces the whole `diff_frames` bisection loop, and it works on traces
recorded before the feature existed, because provenance is reconstructed
from deterministic re-execution rather than stored in the trace. See
`projects/dogfood/causality/main.rad` and `projects/dogfood/timetravel/why_session.jsonl`.

### Retroactive edits: `rad replay --with`

```text
rad replay trace.radr --with fixed.rad
```

Replay the recorded session's **inputs** against **modified code** — "what would my fix
have done in that exact production session?" Two passes run back to back:

1. **Faithful pass** — the trace's embedded (original) source replays strictly,
   producing the recorded final world.
2. **Retro pass** — the edited file runs against the same trace, with recorded io served
   from an **oracle keyed by `(builtin, args)`**, consumed FIFO per key. Same question →
   the same answer the recorded world gave, regardless of how the edit reordered,
   removed, or duplicated calls.

Oracle semantics, chosen deliberately:

- **Repeatable reads** — a key exhausted by extra calls serves its last recorded value:
  a file re-read returns the same content (it didn't change mid-session), an extra
  `clock()` freezes time at its last reading.
- **Holes are loud** — io the recorded session *never* performed halts the retro pass:
  `retroactive replay hole at frame N: …` — replay cannot fabricate answers from a world
  it never saw. (RNG needs no oracle: the seed travels in the header, so `rand_int`
  replays for free even when the edit consumes it differently.)

The deliverable is the **fix's blast radius** — a value-accurate diff of the two final
worlds:

```text
=== Retroactive replay: fixed.rad against the recorded session ===
Recorded io: 3 consumed, 1 repeated reads, 0 unused
The edit's blast radius (original vs edited final world):
  {Gold: 1}
```

`{Gold: 1}` reads as: *this fix restores the drained gold and touches nothing else* —
Health histories are byte-identical. A fix that reports `changes NOTHING` is equally
informative (e.g. an edit confined to `simulate()` forks never touches the real
timeline). See `projects/dogfood/timetravel/fixed.rad` and `main_v2.rad`.

### Schema migration: `migrate`, `save_world()`, `load_world()`

```rad
component Health { hp: 100, max_hp: 100 }       // v2 shape

migrate Health(old) {                            // v1 saves had only `hp`
    return Health { hp: old["hp"], max_hp: old["hp"] * 2 }
}

write_file("world.radw", save_world())           // persist: schema travels with the data
let n = load_world(read_file("world.radw"))      // replace world; migrate shape drift
```

Schema evolution as grammar. The world — entities, names, components,
resources — is a first-class value, so persisting it is one builtin, not a
serialization framework:

| builtin | type | effect | what it does |
|---|---|---|---|
| `save_world()` | `() -> str` | `read_ecs` | world → JSON, **schema embedded** (per-type field layout), full-fidelity tagged values, wrapped in the `RADWORLD3` integrity envelope (blake3 digest) |
| `load_world(json)` | `(str) -> int` | `ecs` | JSON -> replacement world; returns entities loaded. **Aborts** on malformed/corrupt input |
| `try_load_world(json)` | `(str) -> Result<int, str>` | `ecs` | the fallible sibling: `Ok(entities_loaded)` or `Err(message)` instead of aborting. A failed load leaves the live world untouched, so an app can fall back to a prior backup |

`save_world()` output carries a blake3 **integrity envelope** (`RADWORLD3 <digest> <body>`, or a
compressed `RADPACK1` envelope for large saves), so `load_world`/`try_load_world` refuse a
corrupted or tampered save loudly instead of loading garbage. Older digest-less `RADWORLD2` saves
and the v1 tagged-tree format still load forever. `load_world` is the fail-fast spelling;
`try_load_world` is the handle-it spelling — the same `get`/`require`, `to_int`/`try_int` pairing
used elsewhere.

Serialization is pure; persistence composes with ordinary io
(`write_file`/`read_file`, TCP, HTTP — anywhere a `str` goes). That one
decision means record & replay (#2) needs zero new machinery: the io
boundary is already recorded.

`load_world` replaces the current entity set with the saved one instead of
appending rows into the live world. Declared resources seed the replacement so
transient resources and resources omitted by older saves remain available; saved
resources then overwrite their declared rows. Each persisted shape is compared
against the declared one:

- **Identical field set** → loads as-is. Field *order* is normalized — reordering
  a component declaration is not a schema change.
- **Shape drift + `migrate X(old)` declared** → the block runs per instance. `old`
  binds the persisted fields as `map<str, any>` (the old shape no longer exists
  as a type); the body must `return` the new component. Renames, splits, computed
  defaults — it's ordinary code.
- **Shape drift, no migration** → a loud error naming exactly what drifted:
  `schema of 'Health' changed (added: [max_hp], removed: []) and no migration is
  declared — add migrate Health(old) { return Health { ... } }`. No silent nulls,
  no zero-filled fields, ever.

**Declared schema versions.** A component or resource may carry a version tag —
`component Incident v2 { ... }` — which `save_world()` embeds per type in the
save's schema section. A migrate block that declares a second parameter,
`migrate Incident(old, from_version)`, receives the **save's** version for that
type as an int (`0` for saves written without one), turning generation
detection from a shape sniff into a fact:

```rad
component Incident v3 { severity: 1, source: "" }

migrate Incident(old, from_version) {
    if from_version == 1 { return Incident { severity: old["sev"], source: "" } }
    if from_version == 2 { return Incident { severity: old["severity"], source: "" } }
    return Incident { severity: old["severity"], source: old["source"] }
}
```

Two generations that happen to share a field set are no longer indistinguishable,
and the sniff can no longer silently pick wrong. The tag is **load metadata,
not state**: re-tagging a component does not move `world_digest()`, and saves
from versionless programs are byte-identical to before. One-parameter
`migrate X(old)` blocks keep working, versioned save or not.

Migrations target components *and* resources by name, and compose with the
rest of the list: loaded entities carry spawn provenance, so `why(hero, Health)`
works immediately after a load, and a `load_world` inside a recorded session
replays deterministically. See `projects/dogfood/schema/v1.rad` / `v2.rad` for the full
v1 → v2 story (added field, renamed fields, migrated resource) in 40 lines.

### Convergence receipts: `world_digest()`

```rad
// after applying the server's down-delta and committing:
if world_digest() == rpc("DIGEST") { print("converged") }
```

| builtin | type | effect | what it does |
|---|---|---|---|
| `world_digest()` | `() -> str` | `read_ecs` | blake3 of the canonical **state-only** serialization (the `save_world` body) |
| `world_digest(fork)` | `(world_fork) -> str` | `read_ecs` | the same digest for a fork's state, **without committing it** |
| `schema_digest()` | `() -> str` | `read_ecs` | fingerprint of the program's declared component/resource/event layouts |

Fork bytes (`fork_to_bytes`) include in-flight events, provenance, frame
counters, and id free-lists — all of which legitimately differ between two
machines whose *worlds* agree, so fork digests cannot prove convergence.
`world_digest()` hashes entities (names, components, fields) and resources
only: two processes that merged to the same state print the same digest, no
matter how they got there. Unflushed events do not move it; a real field
change does.

`world_digest()` is an **integrity** receipt (these bytes are what I hashed),
not a **validity** receipt: it certifies whatever world it is handed, including
a type-corrupted one. Validity is enforced separately — the `load_world` field-
type boundary rejects a wrong-typed save, and the `RADWORLD3` envelope rejects a
tampered one — so a digest match means "same state", never "well-formed state".

**Across a schema migration**, raw digest comparison lies: the canonical
body embeds the schema, so a v1 world and its v2-migrated twin digest
differently *by construction* — exactly when a rolling upgrade needs the
receipt most. The protocol that stays honest:

1. Exchange `schema_digest()` first. Equal fingerprints → compare
   `world_digest()` directly, as before.
2. Different fingerprints → the newer side **certifies**: the older peer
   ships its full fork bytes; `fork_from_bytes` migrates them on ingest
   (running the declared `migrate` blocks), and `world_digest(fork)`
   hashes that migrated view. Both sides of the comparison now carry the
   same schema, so equality means *logical* convergence — and a real
   divergence still reports MISMATCH truthfully.

```rad
// the upgraded server's CERTIFY handler:
match fork_from_bytes(client_bytes) {
    Ok(theirs) => {
        if world_digest(theirs) == world_digest() { reply("MATCH") }
        else { reply("MISMATCH") }
    }
    Err(m) => { reply(f"ERROR {m}") }
}
```

See `projects/dogfood/radtrack/demo/run_rolling_demo.ps1` for the live receipt.

### World merge: `merge_forks()`

```rad
let base = fork()
// …branch A mutates the world… let ours = fork()
// …commit(base), branch B mutates… let theirs = fork()

match merge_forks(base, ours, theirs) {
    Ok(merged) => { commit(merged) }       // both futures, one timeline
    Err(conflicts) => {                    // conflicts are data, not prose
        for c in conflicts {
            match c {
                FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                    print(f"{name}: {comp}.{field} ours={ours} theirs={theirs}")
                }
                _ => { print("structural conflict") }
            }
        }
    }
}
```

Git merge for program state — with one move git cannot make, because the
language owns 100% of state and text owns none:

- **Field granularity.** A conflict is the *same field* of the same entity or
  resource diverging from base in both forks — never coarser. Two forks
  editing `Stats.atk` and `Stats.def` of the same component merge cleanly;
  two forks raising `Bank.gold` and upgrading `Bank.vault` both land.
  Convergent edits (both forks writing the same value) are not conflicts.
- **Entity ids are handles, not identity.** Two forks spawning different
  entities that collide on a runtime id is *not* a conflict: theirs is
  remapped to a fresh id and **every `EntityId` reference contributed by
  theirs is deep-rewritten** — through lists, tuples, sum types, nested
  components, and maps (keys included). The remap happens before any
  comparison, so a reference to a colliding spawn can never spuriously
  equal an ours-side reference.
- **Names are identity.** Two forks claiming the same name for different
  entities is a real conflict (`names are identity`). Renames three-way
  merge like any field.
- **Despawn rules.** Despawn vs. untouched → despawn wins. Despawn vs.
  modified → conflict. Component removal follows the same logic.
- **Conflicts are data, not prose.** `Err` carries a `list<Conflict>` — a
  built-in sum type whose variants carry the subject and all three diverging
  values. A resolution policy is a `match` in user code, never string
  parsing. The variants:

  | variant | fields | meaning |
  |---|---|---|
  | `FieldConflict` | `ent, name, comp, field, base, ours, theirs` | same field diverged in both forks (resolvable: a value) |
  | `ResourceFieldConflict` | `res, field, base, ours, theirs` | same resource field diverged (resolvable: a value) |
  | `ComponentConflict` | `ent, name, comp, detail` | removed-vs-modified, added-both, layout drift |
  | `DespawnConflict` | `ent, name, detail` | despawned in one fork, modified in the other |
  | `RenameConflict` | `ent, base, ours, theirs` | renamed differently in both forks (resolvable: the chosen name) |
  | `NameConflict` | `name, entities` | one name claimed by several entities (resolvable: a list of new names) |
  | `ResourceConflict` | `res, detail` | resource initialized in both forks, layout drift |
  | `EventConflict` | `detail, base, ours, theirs` | in-flight events consumed or reordered |

  The `ent` field is a live entity handle — a policy can `get()` other
  components off the conflicting entity to make its decision.
- **In-flight events merge too — never silently dropped.** Emission is
  append-only within a fork, so base's pending queue must be a prefix of
  each branch's queue; the merged fork carries base's events, then ours'
  post-fork emissions, then theirs' (with entity references in theirs'
  payloads rewritten through the id remap). `commit(merged)` +
  `flush_events()` fires all of them. If a branch *consumed* events the
  other still carries (it called `flush_events()` after the fork), there is
  no honest automatic answer to "did those handlers run?" — the merge
  refuses with an `in-flight events` conflict instead of guessing.

The merged world is rebuilt canonically (sorted entity/component order)
through the engine's own operations, so archetype, index, name-map, and
id-allocator invariants hold by construction, and `merge_forks(base, a, b)`
agrees with `merge_forks(base, b, a)` wherever no remap is involved.

**Programmable resolution: `merge_forks_with()`.** Pass a list of
`(conflict, resolution)` pairs and the merge applies them instead of
refusing. What counts as a resolution depends on the conflict:

- `FieldConflict` / `ResourceFieldConflict` — the value the merged world
  should carry.
- `NameConflict` — a list of new names, one per claiming entity (in the
  conflict's `entities` order; `""` unnames). Names are semantic identity,
  so the machine never picks — but "keep both, as `T-5/a` and `T-5/b`" is a
  complete human answer. The merge **re-validates** after renaming: chosen
  names that still collide (with each other, or with an entity the forks
  never touched) come back as conflicts, so a rename can never steal a name
  unnoticed.
- `RenameConflict` — the one name the entity should carry.
- Despawns and event consumption have no honest "pick a side" and stay
  unresolvable.

The sync policy lives in user code:

```rad
fn rank(s: str) -> int {
    if s == "closed" { return 3 }
    if s == "escalated" { return 2 }
    return 1
}

match merge_forks(base, ours, theirs) {
    Ok(m) => { commit(m) }
    Err(conflicts) => {
        let mut decisions = []
        for c in conflicts {
            match c {
                FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                    if comp == "Ticket" and field == "status" {
                        let mut pick = ours              // precedence: closed > escalated > open
                        if rank(theirs) > rank(ours) { pick = theirs }
                        decisions = push(decisions, (c, pick))
                    }
                    if comp == "Ticket" and field == "assignee" {
                        decisions = push(decisions, (c, theirs))   // pusher wins
                    }
                }
                _ => {}
            }
        }
        let m = merge_forks_with(base, ours, theirs, decisions) |> unwrap
        commit(m)
    }
}
```

Name claims resolve the same way — two offline clients both minting `T-5`
is one rename away from a clean merge:

```rad
match c {
    NameConflict { name, entities } => {
        // keep both: first claimant (ours) and second (theirs, remapped)
        decisions = push(decisions, (c, [f"{name}/a", f"{name}/b"]))
    }
    _ => {}
}
```

Unnamed conflicts still come back as `Err` — a policy resolves exactly what
it names, nothing silently.

| builtin | type | effect |
|---|---|---|
| `merge_forks(base, ours, theirs)` | `(world_fork, world_fork, world_fork) -> Result<world_fork, list<Conflict>>` | `ecs` |
| `merge_forks_with(base, ours, theirs, resolutions)` | `(world_fork, world_fork, world_fork, list<(Conflict, any)>) -> Result<world_fork, list<Conflict>>` | `ecs` |

(`merge` remains the map-merge builtin; `merge_forks` is its world-scale
sibling.) This is the convergence point of the whole list: **fork** futures
(#1), **diff** them (#3), **merge** the ones you want (#7), `commit` the
result — speculative execution with reconciliation, as language primitives.
See `projects/dogfood/worldmerge/main.rad`, and `projects/dogfood/opsdesk/` for all seven
features running as one machine in one program (migrate a v1 save, forecast
with simulate, merge two shifts with an in-flight event, fence the merge
with `assert_only_changed`, audit with `why()`, record and replay the whole
session bit-for-bit).

### Distributed world merge: `fork_to_bytes()`, `fork_from_bytes()`

A fork is full program state — world, names, id-allocator, resources, and
in-flight events. The wire codec moves that state between processes and
machines, so two copies of a program can diverge offline and merge one world
on reconnect:

```rad
// machine A
tcp_write(conn, fork_to_bytes(fork()))

// machine B
let theirs = fork_from_bytes(tcp_read(conn, 1048576)) |> unwrap
let merged = merge_forks(base, fork(), theirs) |> unwrap
commit(merged)
flush_events()        // events that were in flight on machine A fire here
```

| builtin | type | effect |
|---|---|---|
| `fork_to_bytes(fork)` | `(world_fork) -> str` | pure |
| `fork_from_bytes(bytes)` | `(str) -> Result<world_fork, str>` | `ecs` |

Guarantees, each backed by a composition test:

- **Roundtrip is identity.** `fork_from_bytes(fork_to_bytes(f))` is
  value-identical to `f` — entities keep their runtime ids, names, and
  component data; the id allocator transfers exactly (a spawn after ingesting
  the copy lands on the same id as a spawn after committing the original);
  pending events survive with their causality ids. Re-encoding the decoded
  fork is **byte-identical** (canonical encoding).
- **The wire is transparent to merge semantics.** Merging a fork that
  crossed a process boundary produces state-identical results to the same
  merge performed in-process — byte-for-byte through every state section.
  The one honest difference is provenance: the wire path labels records
  that crossed machines with their payload digest, the in-process path has
  no seam to invent.
- **Provenance rides the wire.** The payload carries the sender's ledger
  closure: for every value alive in the fork, the last write that produced
  it, plus the transitive emit chain behind those writes, plus emit records
  for in-flight events. `commit()` ingests it — foreign emit ids are
  remapped to fresh local ids (the in-flight queue's included, so handlers
  that fire *after* the commit still chain back to remote emits), entity
  ids follow any merge remap, and every ingested record is labeled
  `[via wire <digest>]`. The receiver names what it verified, not what the
  sender claims.
- **Schema drift runs `migrate` blocks on ingest.** The payload embeds its
  schema like `save_world()`; a receiver with a newer declaration migrates
  each component as it decodes. Two machines may disagree on schema version
  and still merge.
- **Corruption is an `Err`, not a crash.** The payload carries a blake3
  integrity digest; tampered or truncated bytes are rejected with a digest
  mismatch, garbage is a parse error. Network input is a system boundary.
- **Record & replay compose for free.** Bytes arrive through io (`tcp_read`,
  `read_file`), so a recorded session that ingested a remote fork replays
  bit-identically with no network present.
- **Big payloads ship packed (RADPACK).** Bodies past ~4 KB are emitted as
  `RADPACK1:<tag> <digest> <base64(deflate(body))>` — measured 6-8x smaller
  on realistic worlds. Small payloads stay legacy JSON (readable,
  grep-able); decoders accept both forever. The digest is always blake3 of
  the *uncompressed canonical body* and always the second space-separated
  token, so `split(bytes, " ")[1]` names the same world in either format.
  Recorded tapes (`--record`) use a raw-binary sibling (`RADPACKZ`, zstd) —
  files don't pay the base64 tax.

See `projects/dogfood/syncdesk/` for the flagship: a long-running server and offline
clients on separate processes — concurrent divergence, merge on reconnect,
field-level conflict reports, an in-flight event that rides the wire and
fires on the server, cross-machine `why()`, and a world that survives server
restarts via `save_world()`/`load_world()`.

### Delta sync: `fork_delta()`, `fork_apply()`

After the first full transfer, the world never needs to cross the wire
again. `fork_delta(base, f)` encodes only the **divergence** of `f` relative
to `base` — and within an entity or resource the base already holds, only
the **changed fields of the changed components** (`ent_patch` / `res_patch`
entries; an hp tick ships `[eid, [["Stats", [["hp", 27]]]], []]`). Full rows
travel only for spawns, renames, newly attached components, and layout
drift. Despawns, the in-flight queue, the id allocator, and the provenance
closure **restricted to touched values** complete the payload. Delta sync pays double: it shrinks state
and history at once, because the receiver already ingested the base's
provenance when it ingested the base.

```rad
// receiver, once: full transfer establishes the shared base
let base = fork_from_bytes(bytes) |> unwrap
commit(base)

// sender, every sync after: divergence only
let delta = fork_delta(base, fork())          // KBs, not MBs

// receiver: rebuild the sender's fork on its own copy of the base
let theirs = fork_apply(base, delta) |> unwrap
let merged = merge_forks(base, fork(), theirs) |> unwrap
```

| builtin | type | effect |
|---|---|---|
| `fork_delta(base, fork)` | `(world_fork, world_fork) -> str` | pure |
| `fork_apply(base, delta)` | `(world_fork, str) -> Result<world_fork, str>` | `ecs` |

Guarantees, each backed by a composition test:

- **Apply reconstructs exactly.** `fork_apply(base, fork_delta(base, f))`
  is state-identical to `f` (canonical full encodings match byte-for-byte):
  edits, spawns, despawns, renames, resources, allocator, and pending
  events all survive. The reconstruction's provenance honestly carries a
  `wire <digest>` origin — that is the one difference, and it is disclosed,
  not hidden.
- **Cost tracks the divergence, not the world.** Touched entities are found
  by CoW pointer comparison (O(divergence) when the forks share lineage,
  full-scan fallback when they don't), and every candidate is re-verified
  by value, so false positives cost a comparison, never bytes.
- **The reconstruction shares lineage with the receiver's base.** Apply is
  a CoW restore plus surgical edits — untouched columns stay shared — so
  the O(divergence) merge fast path works on wire-delivered forks, and a
  merge over a delta-delivered fork equals the in-process merge.
- **Provenance is restricted to the delta.** Only records for touched
  entities and changed resources travel, plus the transitive emit chain and
  the in-flight queue's emit records. `commit()` ingests them exactly like
  the full codec's; cross-machine `why()` works identically over the delta
  path.
- **Schema drift migrates on apply.** The delta embeds the schema of the
  types it ships; v1 rows arriving at a v2 receiver run its `migrate`
  block, exactly like `fork_from_bytes`. Field *patches* migrate too: a
  patched component whose shipped layout differs from the receiver's
  declared layout is patched by field name and then re-enters the `migrate`
  block, so derived fields (`shield = hp / 2`) stay coherent.
- **Wrong base and corruption are an `Err`.** The payload carries a blake3
  integrity digest plus a fingerprint of the base it was made against
  (allocator state, entity count, queue length); applying a delta to a
  world it doesn't describe is rejected, not fabricated. Base *identity* is
  the protocol's job — syncdesk keys served bases by the digest in the
  PULL payload's header, and `DPUSH <digest>\t<delta>` names its base by it.

At 10k entities with 200 touched, the delta is ~29 KB against ~1.5 MB for
the full payload (~54x), encodes in ~0.8 ms against ~45 ms, and applies in
~1.3 ms against ~71 ms (see [performance](performance.md)).

### Cross-machine `why()`

Because provenance rides the fork payload, `why()` answers for values this
machine never computed:

```text
Ticket of T-9 = { status: "open" }   (spawned in frame 0)   [via wire 738ec279, remote frame]
  <- by top-level code
```

The chain crosses the seam: a handler that fires locally for an event that
was emitted on another machine explains itself with the remote emit record
(`[via wire …]` on the emit line) and walks back to the remote cause.
Frames inside foreign records follow the *sender's* clock — the label says
so instead of pretending one timeline exists. Ledger ingestion is
component-granular: after a policy-resolved merge, the newest record for a
component is the sender's whole-component write even when the surviving
value mixes both sides field-by-field; the `commit() adopted a fork` note
discloses exactly that seam.

### Causality retention

The provenance ledger behind `why()` is a **window, not an archive**: it
retains the most recent 100,000 write and emit records each, evicting the
oldest. Long-running processes do not grow bookkeeping without bound. Emit
ids stay stable across eviction and commit seams keep their absolute
ordering; when a query reaches into evicted history, `why()` says
`older provenance was evicted by the retention window` instead of guessing.
Full history is always reconstructible by replaying a recorded trace.

### Streaming sessions (embedding API)

A host application (browser tab, game engine, editor) can keep one VM alive
as a **streaming session** instead of compiling per interaction. The
embedding API on `RadRuntime` (native and WASM, exported to JS by
`wasm-pack`):

| method | what it does |
|---|---|
| `runtime_features()` | JSON feature/version handshake for hosts before they enable advanced session features |
| `session_start(source)` | compile once, run top-level, fix the RNG seed (replicas converge) |
| `session_emit(event, fields_json)` | push one event; `fields_json` must be an object keyed by event fields, and `{"entity": "name"}` resolves handles |
| `session_pump()` | flush one frame through the declared handlers; returns that frame's prints |
| `session_render_delta()` | renderer-shaped JSON diff since the last render read: upserts, removes, and changed resources |
| `session_delta()` | the divergence since the last delta, as `fork_delta` bytes — one broadcast per flush |
| `session_apply(delta)` | apply a remote delta in order; wrong-lineage deltas are refused by the base fingerprint |
| `session_state()` / `session_load(state)` | full-state handshake for late joiners |
| `session_digest()` | state-only convergence receipt (`world_digest`) |
| `session_checkpoint()` | push the current world onto the capped undo ring before a user interaction |
| `session_undo()` / `session_redo()` | rewind or reapply a whole-world checkpoint; return `false` when empty |
| `session_why(entity, component)` | explain the live session's current value for a named entity/component |
| `session_preview(event, fields_json)` | emit and flush an event in a fork, return the preview world JSON, then roll back exactly |
| `run_traced(source)` | run a program with timeline tracing enabled and leave frames inspectable |
| `run_traced_with_patch(source, frame, entity, component, field, value_json)` | rerun with a field patch injected at a frame to preview the rewritten future |
| `timeline_len()` | number of captured timeline frames |
| `timeline_world(i)` | renderer-shaped JSON for captured frame `i` |
| `timeline_events()` | JSON event log sourced from the causality ledger |
| `why_at(frame, entity, component)` | causal explanation for a named entity/component as of a captured frame |

Host-pushed events get real causality records (`why()` answers for them),
and a session's frames are the same frame boundary record/replay counts.
Replicas never run handlers — they converge on state alone, which is what
makes a 3-tab browser demo agree byte-for-byte with the tab that did the
work. See `projects/playground/collab.html` for the wiring: BroadcastChannel
between same-browser tabs by default, or a real WebSocket relay
(`projects/playground/relay/relay.mjs`, `?relay=ws://host:8378`) so peers on other
machines join the same session — the relay is dumb fan-out; every
semantic stays in the VM.

`runtime_features()` reports `"causal_laws": 1` and
`"causal_constraints": 1` when the embedder can compile RFC-0001/RFC-0002
syntax. It also reports `"host_values": 1` and the active
`causal_value_limits` profile (`max_depth`, `max_nodes`,
`max_encoded_bytes`, and `max_collection_items`). WASM hosts opt in by
checking those markers before providing a Causal Laws program; the native CLI
uses `--experimental-laws`.

The `constraint_limits` object contains the version and fingerprint plus
per-invocation fuel/heap limits, the separately reserved aggregate fuel/heap
envelope, violation caps, and the exact canonical rejection byte cap. The
profile's value limits are the same limits shown in `causal_value_limits`;
setting either host profile updates the single transaction value domain.
Browser hosts can call `compile_and_run_result_json()` for a tagged
`settlement_rejected`, `runtime_error`, or `host_fault` result.

Rejection candidate values are frozen once per `(entity, component)` and
referenced by violations. Canonical bytes are produced through a bounded
writer. Capability rendering replaces hidden origins as a whole, including
law/resolver/intent identity and source metadata, instead of exposing a name
with only its payload removed.

Rust embedders exchange [`FrozenValue`](../../../core/vm/src/host_value.rs)
trees with the VM. A `ValueHandle<'vm>` may inspect one imported or global
value while its VM is borrowed, but cannot outlive that VM. The NaN-boxed raw
value and GC heap are deliberately crate-private. This prevents a heap pointer
from surviving its owner or being mutably aliased by copying a machine word.

Causal proposal and candidate capture uses the same limit profile. Cycles are
rejected. Shared acyclic subgraphs are serialized as trees and every repeated
edge is charged again, matching the canonical provenance representation. A
limit failure aborts the settlement without committing world or ledger state.

```js
const runtime = new RadRuntime()
JSON.parse(runtime.runtime_features())
runtime.session_start(source)
runtime.session_checkpoint()
runtime.session_emit("Click", JSON.stringify({ target: "button-1" }))
const printed = runtime.session_pump()
const render = JSON.parse(runtime.session_render_delta())
const why = runtime.session_why("button-1", "Style")
```

> **Backend note:** `core/vm` is the ground-truth implementation for
> speculative execution and event semantics. The historical C backend is frozen
> and should not be used as current feature-support evidence.

```rad
let future = fork()
let predicted = simulate(future, [system::Physics, system::AI], 10)

// Inspect the fork without committing
let predicted_hp = peek(predicted, hero, Health)?
if predicted_hp.hp > 0 {
    commit(predicted)
}
```
