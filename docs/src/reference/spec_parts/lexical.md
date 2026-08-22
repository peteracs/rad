<a id="lexical-structure"></a>
## 1. Lexical structure

This chapter is normative for source text. The generated
[compiler-owned surface](generated/language-surface.md) is the completeness
denominator; the entries below define what those tokens mean.

<a id="lex-source"></a>
### 1.1 Source text and locations

RAD source is UTF-8. A leading UTF-8 byte-order mark is ignored. Source spans
use byte offsets; diagnostics expose one-based line and column positions. `LF`
and `CRLF` each advance one source line. Spaces, tabs, and newlines separate
tokens but are otherwise insignificant. Newlines do not terminate statements;
the grammar and delimiters do.

Authenticated multi-file bundles may provide host-owned line mappings. Source
comments cannot alter file identity or locations, and there is no user-authored
line-directive syntax.

<a id="lex-identifiers"></a>
### 1.2 Identifiers and keywords

```ebnf
identifier = ( ASCII_LETTER | "_" ) { ASCII_LETTER | DIGIT | "_" } ;
```

Identifiers are case-sensitive. These are hard keywords and cannot be ordinary
binding names:

```text
component struct entity resource system event on emit fn let mut
if else while for in return true false nil schedule and or not match when
use break continue type pure once async await rec pub as indexed unique
```

The parser accepts a hard-keyword token as a field name only in an
unambiguous field-name position. This permits records such as
`Receipt { entity: id }` without making `entity` a general identifier.

Current contextual words include declaration/contract words such as
`state`, `readonly`, `phase`, `serial`, `transient`, `ordered`, `owned`, `coowners`,
`migration`, `materialized`, `view`, `test`, `shared`, `model`, `transaction`,
`requires`, `changes_only`, `ensures`, `post_commit`, `intent`, `law`,
`resolver`, `constraint`, `watches`, `settle`, `propose`, `next`, `require`,
`where`, `select`, `after`, `before`, `accum`, `reads`, `writes`, `emits`, and
`io`. Their meaning is determined by their grammar position.

<a id="lex-literals"></a>
### 1.3 Literals

```ebnf
integer       = DIGIT { DIGIT } ;
float         = DIGIT { DIGIT } "." DIGIT { DIGIT }
              | DIGIT { DIGIT } ( "e" | "E" ) [ "+" | "-" ] DIGIT { DIGIT }
              | DIGIT { DIGIT } "." DIGIT { DIGIT }
                ( "e" | "E" ) [ "+" | "-" ] DIGIT { DIGIT } ;
boolean       = "true" | "false" ;
nil_literal   = "nil" ;
string        = '"' { string_character | escape } '"' ;
```

An optional leading `-` is the unary negation operator, not part of an integer
or float token. Integer tokens are parsed as signed 64-bit values and are
range-checked. Fixed-width values are explicit constructor calls such as
`u8(7)`, `i32(-4)`, and `f32(1.5)`; they are nominal runtime values, not
different token kinds.

Strings support `\\`, `\"`, `\n`, `\r`, `\t`, and Unicode source text.
Malformed escapes are lexical errors. `true`, `false`, and `nil` are dedicated
tokens.

<a id="lex-fstrings"></a>
### 1.4 Interpolated and multiline strings

```text
let short = f"asset={asset_id} load=${megawatts:04}"
let report = f"""Grid report
literal braces: {not_interpolation}
value: ${megawatts:.2f}
"""
```

Single-line f-strings accept both `{ expression }` and `${ expression }`.
Triple f-strings accept `${ expression }`; bare braces are literal text.
An interpolation may include a format specifier after `:`. Interpolations are
ordinary expressions and therefore follow the normal type/effect rules.

Line-based multiline strings start each source line with `\\` and concatenate
their payload lines with newline separators. Triple f-strings are preferred
when interpolation is required.

<a id="lex-comments"></a>
### 1.5 Comments

`//` starts a comment through the end of the physical line. RAD has no block
comment token. Comments are normally discarded; tooling may request preserved
`Comment` tokens. Whitespace and comments never affect semantic identity.

<a id="operators-and-precedence"></a>
### 1.6 Operators and precedence

The following table is ordered from lowest to highest precedence.

| Level | Forms | Associativity | Meaning |
|---:|---|---|---|
| 1 | `\|>` | left | pipeline |
| 2 | `or` | left, short-circuit | logical disjunction |
| 3 | `and` | left, short-circuit | logical conjunction |
| 4 | `==`, `!=`, `is` | left | equality and nominal variant test |
| 5 | `<`, `<=`, `>`, `>=` | left | ordered comparison |
| 6 | `\|` | left | bitwise OR |
| 7 | `^` | left | bitwise XOR |
| 8 | `&` | left | bitwise AND |
| 9 | `<<`, `>>` | left | integer shift |
| 10 | `+`, `-` | left | addition/subtraction |
| 11 | `*`, `/`, `%` | left | multiplication/division/remainder |
| 12 | `-`, `not`, `!`, `~`, `await`, `async` | right/prefix | unary/async forms |
| 13 | `.`, `()`, `[]`, `?` | left/postfix | access, call, index, propagation |

`and` and `or` require booleans. `!` and `not` are the same logical operation;
`~` is integer bitwise complement. `is` tests a state/sum variant and does not
perform structural equality. Integer division truncates toward zero. Shift
counts and arithmetic conversions are checked by the operand type contracts.

<a id="lex-punctuation"></a>
### 1.7 Delimiters and punctuation

| Token | Source | Role |
|---|---|---|
| braces | `{` `}` | declarations, blocks, maps, records |
| parentheses | `(` `)` | grouping, calls, tuples, parameters |
| brackets | `[` `]` | lists, indexing, schedules, list patterns |
| separators | `,` `:` `.` `::` | items, annotations, access, nominal paths |
| ranges/spread | `..` | argument/record spread and supported rest forms |
| arrows | `->` `=>` | return/transition types and match arms |
| assignment | `=` | binding, assignment, field defaults |
| contracts | `@` | callable contract prefix |
| optional/error | `?` | postfix result propagation |
| bucket append | `<<` | shift expression or indexed bucket-fill assignment |

The parser uses surrounding grammar to distinguish `{ key: value }` maps from
record construction, `<<` shift expressions from bucket-fill statements, and
`..` spread arguments from record/pattern rest.

<a id="lex-errors"></a>
### 1.8 Lexical errors and sentinels

Unterminated strings/interpolations, invalid escapes, out-of-range numeric
tokens, and unknown characters produce lexical diagnostics. `Error` is an
internal recovery token and `Eof` is the synthetic end-of-input sentinel;
neither can be written as source syntax.
