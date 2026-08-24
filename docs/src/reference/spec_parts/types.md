<a id="type-system"></a>
## 2. Type system

RAD checks declarations, bindings, calls, fields, operators, patterns, and
effects before bytecode lowering. Unannotated private code may infer `any`;
public declarations and strict-checking mode require explicit boundaries.

<a id="type-primitives"></a>
### 2.1 Primitive and runtime types

| Type | Meaning |
|---|---|
| `nil` | Unit/absence value. |
| `bool` | `true` or `false`. |
| `int` | Signed integer with an inline fast range and checked big-integer representation outside it. |
| `float` | IEEE-754 binary64. |
| `str` | Immutable UTF-8 text. |
| `entity` | Generational ECS entity identity. |
| `any` | Gradual top type; operations may require runtime checks. |
| `bitset` | Integer-membership value used by the bitset API. |
| `world_fork` | Copy-on-write isolated world plus event/runtime state. |
| `system` | Statically named system reference created by `system::Name`. |
| `task<T>` | Result of an async call, consumed by `await`. |

`bytebuf` is a first-class binary value. Byte-buffer constructors and
functional setters return `bytebuf`, binary host APIs accept it directly, and
TypeScript declarations expose it as `Uint8Array`. `buffer_new` remains an
opaque VM value typed as `any`; only its corresponding text-buffer APIs may
inspect or mutate it. Network and native-extension handles are likewise opaque host values. Handles
are process-local, are rejected by persistent serialization, and must be
closed through the owning API.

<a id="type-native-scalars"></a>
### 2.2 Fixed-width native scalars

RAD provides exact-width scalar constructors and types:

```text
u8 u16 u32 u64
i8 i16 i32 i64
f32 f64
```

Conversions are explicit constructor calls. Integer conversion rejects values
outside the destination range. Narrowing `f64` to `f32` is explicit and uses
binary32 rounding. Native values retain their width/signedness tag in values,
maps, snapshots, replay, equality, and host transport.

<a id="type-collections"></a>
### 2.3 Collections, tuples, unions, and applications

```ebnf
type_expr     = union_type ;
union_type    = primary_type { "|" primary_type } ;
primary_type  = IDENT
              | IDENT "<" type_expr { "," type_expr } ">"
              | "(" [ type_expr { "," type_expr } ] ")"
              | function_type ;
```

Canonical compound types are:

```text
let names: list<str> = []
let by_id: map<u64, str> = {}
let pair: (int, str) = (7, "seven")
let scalar: int | str = 7
let maybe: Option<int> = Option::Some { value: 7 }
let result: Result<int, str> = Result::Ok { value: 7 }
```

Map keys may be `int`, fixed-width/nominal native scalars, `str`, `bool`,
`entity`, tuples composed only of valid key types, or `any` values whose runtime
value satisfies that rule. Floats, mutable containers, records, tasks, forks,
and host handles are not map keys.

Lists, maps, tuples, tasks, type applications, and union alternatives are
checked recursively. Generic substitution therefore applies inside nested
tuples, lists, maps, unions, and function types.

<a id="function-types"></a>
### 2.4 Function types

```ebnf
function_type = [ "pure" | "readonly" ] "fn"
                "(" [ type_expr { "," type_expr } ] ")"
                [ "->" type_expr ] ;
```

```text
let transform: fn(int) -> str
let projection: pure fn(int) -> int
let lookup: readonly fn(entity) -> int
```

`pure fn` has no observable shared-state effect. `readonly fn` may read ECS and
resources but cannot mutate them or perform I/O. Unqualified `fn` carries the
effects inferred for its statically resolved target. Purity is part of the
function type and is checked at every assignment, argument, return, and
capture boundary.

<a id="type-generics"></a>
### 2.5 Generic aliases and functions

```text
type Pair<T> = (T, T)
type Index<K, V> = map<K, list<V>>

pure fn identity<T>(value: T) -> T { return value }

let pair: Pair<int> = (3, 7)
let index: Index<str, entity> = {}
```

Type parameters are lexical to their declaration. A call infers type arguments
from annotated parameters and arguments; unresolved or contradictory
substitutions are errors in strict code. Aliases are transparent and cannot
form an infinitely expanding alias cycle.

<a id="type-nominal"></a>
### 2.6 Nominal types and native layout

```text
opaque type AssetId = u64

enum MissionKind: u8 {
    Medical = 1,
    Power = 2,
}

bitflags AssetFlags: u16 {
    Autonomous = 1,
    Priority = 8,
}

repr(C) packed struct SensorPacket {
    zone: u16,
    temperature: f32,
    alarm: u8,
}
```

Opaque types, native enums, and bitflags are nominal: equal representation does
not permit assignment or argument substitution. Construction names the target
type explicitly. Enum discriminants must be unique and representable. Primitive
bitflag masks must be nonzero, representable, and non-overlapping.

`repr(C)` fixes C field ordering/alignment; optional `packed` sets alignment to
one. `size_of`, `offset_of`, `encode_le`, `encode_be`, `decode_le`, and
`decode_be` use the declared layout. Native structs may contain only
ABI-supported fields. Encoding is byte-exact; decoding rejects wrong lengths,
unknown enum discriminants, invalid nominal values, and malformed data.

<a id="type-records"></a>
### 2.7 Components, resources, structs, states, events, and sums

Each declared component, resource, struct, state machine, event, and sum type
is nominal. Component/resource fields may be inferred from defaults in private
code; public fields require explicit types. Sum variants are namespaced under
their sum type and carry their declared fields. State values are constructed
from state references and may change only through a declared transition.

`Option<T>` and `Result<T, E>` are ordinary built-in nominal sum types:

```text
Option::Some { value: 7 }
Option::None {}
Result::Ok { value: 7 }
Result::Err { error: "denied" }
```

`?` propagates the error/absence through a compatible function return type.

<a id="type-assignability"></a>
### 2.8 Inference and assignability

An initializer must be assignable to its annotation. `int` may promote to
`float`; non-nominal numeric arguments may convert where the called API permits
it. Nominal native types never cross-convert implicitly. Collections are
checked element-by-element; tuples require equal arity; a value is assignable
to a union when it matches at least one alternative.

`any` is accepted at typed boundaries but shifts the relevant check to runtime.
Strict mode rejects missing public/entry return types and unresolved boundary
types instead of silently widening them.

<a id="type-pattern-binding"></a>
### 2.9 Destructuring, scope, and shadowing

```text
let (left, right): (int, int) = pair
for [index, value] in enumerate(values) { }
let Some { value } = maybe else { return }
let callback = fn([index, value]: (int, int)) -> int { return value }
```

Tuple/list destructuring requires matching arity. Refutable variant patterns
require `let ... else` or a `match`. Pattern bindings begin after the pattern
has matched; match bindings are visible in their guard and arm only; loop
bindings are visible only in the loop body. A nested binding may shadow an
outer name without changing the outer value.

`let unique` is not a type. It is a static ownership qualifier on a binding:
the value cannot be aliased, passed, returned, or captured in a way that loses
uniqueness. Mutation of that binding can therefore use in-place container
operations without copy-on-write cloning.
