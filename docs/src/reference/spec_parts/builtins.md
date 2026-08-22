<a id="builtins"></a>
## 6. Builtin invocation

The VM installs exactly the names in the
[generated builtin API catalog](generated/builtin-api.md). That catalog is
generated from `Builtin::ALL` and the canonical type/effect schemes; the
documentation gate rejects missing, duplicate, reordered, or orphaned entries.
This section defines rules shared by all entries. The curated
[Builtin Functions](builtins.md) guide groups related operations and gives
longer workflows.

<a id="builtin-resolution"></a>
### 6.1 Resolution and calls

A bare call resolves a lexical binding before a builtin name. Builtins are not
module declarations and cannot be imported or mutated. Direct builtin calls
are type-checked from the canonical polymorphic scheme. Passing a builtin as a
function value retains its parameter, return, and purity contract.

Arity is exact except where an entry explicitly declares variadic arguments.
Argument evaluation is left-to-right. Invalid arity, type, range, capability,
region, platform, or runtime state is an error unless the signature explicitly
returns `Option` or `Result` for that condition.

<a id="builtin-values"></a>
### 6.2 Value and collection semantics

Scalar conversions are explicit and checked. List, map, set-like, string, and
buffer transformations preserve RAD value semantics: a returned collection is
the new value, while mutation is permitted only through a unique binding or an
explicit mutable buffer/host handle API. Collection iteration is deterministic
under the ordering documented by its entry.

Collection builtins take the collection as their first ordinary argument so
they compose with `|>`. Higher-order operations check callback type and purity;
callbacks participate in transitive effects and cost analysis.

<a id="builtin-world"></a>
### 6.3 ECS, indexes, views, provenance, and persistence

World reads and writes use declared canonical component/resource identities.
Indexed and ordered operations read or write their synthetic/index authority;
they are not effect-free. View operations read runtime-maintained materialized
state and never rebuild source populations. Provenance operations inspect the
same causal ledger that writes, transactions, events, host records, views, and
relations maintain.

Persistence and fork APIs validate full schema/program/runtime identity before
adoption. Data-only codecs reject tasks, worlds, sockets, native handles, and
other process-owned capabilities. No loader, decoder, or merge operation
partially mutates the destination on failure.

<a id="builtin-host"></a>
### 6.4 Host, IO, network, and FFI operations

Host calls have explicit IO/async/allocation/determinism classifications.
Native-only operations report platform unavailability on WASM. Network and
filesystem operations return typed handles owned by the creating VM and require
explicit close where their entry says so. Extension calls first validate ABI,
layout, opaque nominal identity, effect descriptor, and plugin generation.

Sandbox execution applies the intersection of the builtin's ordinary effect
contract and the supplied guest capabilities. An omitted permission does not
manufacture authority. Guest output is data-only and untrusted text is kept
separate from host diagnostics.

<a id="builtin-regions"></a>
### 6.5 Region availability

Purity alone does not determine region availability. The canonical effect
firewall classifies each builtin for transaction body, post-commit, settlement,
and sandbox execution. For example, staged ECS writes are legal in a stable
transaction but prohibited in post-commit; relation candidate writes are legal
only in their resolver settlement. The generated API entry reports each
classification.

<a id="builtin-complexity"></a>
### 6.6 Complexity and allocation

Only an explicit API guarantee assigns a complexity class. Important classes
are constant, logarithmic, output-linear, population-linear, and population
`n log n`. `entities`, query/filter/map, sort, range materialization, snapshot,
serialization, and provenance rendering may allocate and scale with their
documented input domain. `lookup`, entity-name lookup, view revision, and exact
index seeks use their declared indexes.

The generated entry distinguishes guest-value, VM-runtime, and host-boundary
allocation where the implementation can prove a contract. “No allocation” is
never inferred merely because a source expression contains no collection
literal.

<a id="builtin-errors"></a>
### 6.7 Error and replay behavior

Each generated entry lists its recoverable return channel and runtime failure
conditions. Deterministic APIs reproduce the same semantic result for the same
RAD inputs. Seeded APIs include the seed in replay identity. Recordable host
APIs serialize input/output/effect identity; replay must consume the record and
must not call live external state.
