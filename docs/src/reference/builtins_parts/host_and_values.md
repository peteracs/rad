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
