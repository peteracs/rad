## 1. Lexical Structure

### 1.1 Keywords

```
component  resource  struct  entity  state  system  event  on  emit  fn  let  mut
if  else  while  for  in  return  break  continue  true  false  nil  schedule
and  or  not  match  when  where  use  as  type  pure  once  indexed  unique
async  await  rec  pub  readonly  phase  update
```

### 1.2 Operators

| Operator | Precedence | Associativity | Description |
|---|---|---|---|
| `\|>` | 1 (lowest) | Left | Pipeline |
| `or` | 2 | Left | Logical OR |
| `and` | 3 | Left | Logical AND |
| `==` `!=` `is` | 4 | Left | Equality / Variant Check |
| `<` `>` `<=` `>=` | 5 | Left | Comparison |
| `|` | 6 | Left | Bitwise OR |
| `^` | 7 | Left | Bitwise XOR |
| `&` | 8 | Left | Bitwise AND |
| `<<` `>>` | 9 | Left | Shifts |
| `+` `-` | 10 | Left | Addition |
| `*` `/` `%` | 11 | Left | Multiplication (`str * int` repeats strings) |
| `-` `not` `!` `~` `await` | 12 | Right (prefix) | Unary |
| `.` `()` `[]` `?` | 13 (highest) | Left (postfix) | Access, call, index, try |

In **v0.5 DX compatibility mode**, `..` is recognized in `match` binding lists as a rest-binding marker.

Note: some declaration keywords are accepted as contextual identifiers in identifier positions where parsing is unambiguous (for example, `entity`, `before`, `after`, and `update` can be used as variable names). `update` is a contextual keyword: `update(...)` starts an update statement only when followed by `(`; otherwise it parses as an identifier. The experimental Causal Laws words `intent`, `law`, `resolver`, `constraint`, `watches`, `settle`, `propose`, `next`, `require`, `else`, and `key` are also contextual rather than reserved.

### 1.3 Literals

- **Integer:** `0`, `42`, `-7`
- **Float:** `3.14`, `0.5`, `-2.0`
- **String:** `"hello"`, `"line\nbreak"`, `"tab\there"`
- **Multi-line String:** `\\line 1\n\\line 2` (Zig-style line-based literals)
- **Boolean:** `true`, `false`
- **Nil:** `nil`
- **List:** `[1, 2, 3]`, `[]`

### 1.4 Comments

```
// Single-line comment (to end of line)
```

### 1.5 Identifiers

Identifiers start with a letter or underscore, followed by letters, digits, or underscores. Keywords cannot be used as identifiers.

### 1.6 Formal Grammar (EBNF)

The following Extended Backus-Naur Form grammar defines the complete syntax of Rad. Terminals are in `"quotes"`, `{ X }` means zero-or-more, `[ X ]` means optional, and `|` separates alternatives.

```ebnf
program        = { declaration } ;

declaration    = [ "pub" ] ( component_decl | resource_decl | struct_decl | entity_decl | state_decl
               | system_decl | event_decl | fn_decl | type_decl | type_alias_decl
               | intent_decl | law_decl | resolver_decl )
               | on_handler | use_decl | test_decl | statement ;

(* Top-level declarations *)
component_decl = "component" IDENT [ VERSION ] "{" { [ "indexed" ] field_def [ "," ] } "}" ;
resource_decl  = [ "transient" ] "resource" IDENT [ VERSION ] "{" { field_def [ "," ] } "}" ;
(* VERSION is an identifier of exactly `v` + digits, e.g. `v2` — the type's
   declared schema version, embedded per type in save_world() output and
   handed to `migrate X(old, from_version)` on load. *)
struct_decl    = "struct" IDENT "{" { field_def [ "," ] } "}" ;
intent_decl    = "intent" IDENT "{" { [ "key" ] IDENT ":" type_expr [ "," ] } "}" ;
law_decl       = "law" IDENT "(" [ typed_params ] ")" block ;
resolver_decl  = "resolver" IDENT "for" IDENT "(" IDENT "," IDENT ")" block ;
constraint_decl = "constraint" IDENT "for" IDENT "(" IDENT "," IDENT ")"
                  [ "watches" IDENT { "," IDENT } ] block ;
entity_decl    = "entity" IDENT "{" { component_entry [ "," ] } "}" ;
entity_expr    = "entity" [ expr ] "{" { component_entry [ "," ] } "}" ;
component_entry = component_init | expr ;
component_init  = IDENT "{" { field_init [ "," ] } "}" ;
field_def      = IDENT ":" [ type_expr "=" ] expr ;
field_init     = IDENT ":" expr ;

state_decl     = "state" IDENT "{" { state_def } "}" ;
state_def      = IDENT "{" { "on" IDENT "->" IDENT [ "when" expr ] [ "," ] } "}" ;

system_decl    = "system" IDENT "(" [ sys_param { "," sys_param } ] ")"
                 { "after" IDENT { "," IDENT } | "before" IDENT { "," IDENT } }
                 block ;
sys_param      = IDENT [ ":" [ "mut" | "accum" ] IDENT ] ;

event_decl     = "event" IDENT "{" { event_field [ "," ] } "}" ;
event_field    = IDENT [ ":" type_expr ] ;
on_handler     = [ "async" ] "on" IDENT [ "once" ] "(" IDENT ")" [ ( "when" | "where" ) expr ] block ;

fn_decl        = [ "pure" | "async" | effect { effect } ] "fn" IDENT [ type_params ] "(" [ param { "," param } ] ")"
                 [ "->" type_expr ] block ;
effect         = "io" | "ecs" | "readonly" | "event" ;
param          = [ "mut" ] IDENT [ ":" type_expr ] ;

type_decl      = "type" IDENT [ type_params ] "{" { variant_def } "}" ;
type_alias_decl = "type" IDENT [ type_params ] "=" type_expr ;
type_params    = "<" IDENT { "," IDENT } ">" ;
type_expr      = single_type { "|" single_type } ;
single_type    = IDENT | "nil"
               | IDENT "<" type_expr { "," type_expr } ">"
               | "(" [ type_expr { "," type_expr } ] ")"
               | "fn" "(" [ type_expr { "," type_expr } ] ")" [ "->" type_expr ] ;
variant_def    = IDENT "{" { field_init [ "," ] } "}" ;

use_decl       = "use" STRING [ "as" IDENT ] [ ":" IDENT ] ;
migrate_decl   = "migrate" IDENT "(" IDENT [ "," IDENT ] ")" block ;
phase_decl     = [ "serial" ] "phase" IDENT ( "[" { IDENT [ "," ] } "]" | "{" { IDENT [ "," ] } "}" ) ;

test_decl      = "test" ( IDENT | STRING ) [ "for" IDENT "in" expr { "," IDENT "in" expr } ] block ;

(* Statements *)
block          = "{" { statement } "}" ;
statement      = let_stmt | let_else_stmt | assign_stmt | if_stmt | while_stmt | for_stmt
               | return_stmt | emit_stmt | schedule_stmt | match_stmt | settle_stmt
               | propose_stmt | next_stmt | constraint_require_stmt
               | "break" | "continue" | expr_stmt ;

let_stmt       = "let" [ "unique" ] [ "mut" ] [ "rec" ] ( IDENT | "(" IDENT { "," IDENT } ")" ) [ ":" type_expr ] "=" expr ;
let_else_stmt  = "let" [ "mut" ] IDENT ( "{" { IDENT [ ":" IDENT ] [ "," ] } "}" | "(" IDENT ")" ) [ ":" type_expr ] "=" expr "else" block ;
assign_stmt    = expr "=" expr ;
if_stmt        = "if" expr block [ "else" ( if_stmt | block ) ] ;
while_stmt     = "while" expr block ;
for_stmt       = "for" ( IDENT { "," IDENT } | "(" IDENT { "," IDENT } ")" | "[" IDENT { "," IDENT } "]" ) "in" expr [ "where" expr ] block ;
return_stmt    = "return" [ expr ] ;
emit_stmt      = "emit" IDENT "{" { field_init [ "," ] } "}" [ "after" expr ] ;
schedule_stmt  = "schedule" [ "serial" ] "[" schedule_target { "," schedule_target } "]" ;
schedule_target = IDENT [ "." IDENT ] | "system" "::" IDENT { "::" IDENT } ;
match_stmt     = "match" expr "{" { match_case } "}" ;
match_case     = match_pattern [ ( "when" | "if" ) expr ] "=>" ( block | expr ) ;
settle_stmt    = "settle" "{" { statement } "}" ;
propose_stmt   = "propose" IDENT "{" { field_init [ "," ] } "}" ;
next_stmt      = "next" "(" expr "," component_init ")" ;
constraint_require_stmt = "require" expr "else" STRING ;
match_pattern  = "_" | INT | FLOAT | STRING | "true" | "false"
               | "has" IDENT [ "(" IDENT ")" ]
               | IDENT [ "{" { IDENT [ ":" IDENT ] [ "," ] } [ ".." ] "}" | "(" IDENT ")" ] ;
expr_stmt      = expr ;

(* Expressions — listed in ascending precedence *)
expr           = pipe_expr ;
pipe_expr      = or_expr { "|>" or_expr } ;
or_expr        = and_expr { "or" and_expr } ;
and_expr       = eq_expr { "and" eq_expr } ;
eq_expr        = cmp_expr { ( "==" | "!=" | "is" ) cmp_expr } ;
cmp_expr       = bitor_expr { ( "<" | ">" | "<=" | ">=" ) bitor_expr } ;
bitor_expr     = bitxor_expr { "|" bitxor_expr } ;
bitxor_expr    = bitand_expr { "^" bitand_expr } ;
bitand_expr    = shift_expr { "&" shift_expr } ;
shift_expr     = add_expr { ( "<<" | ">>" ) add_expr } ;
add_expr       = mul_expr { ( "+" | "-" ) mul_expr } ;
mul_expr       = unary_expr { ( "*" | "/" | "%" ) unary_expr } ;
unary_expr     = ( "-" | "not" | "!" | "~" | "await" ) unary_expr | "async" postfix_expr | postfix_expr ;
postfix_expr   = primary { "." IDENT | "(" [ call_arg { "," call_arg } ] ")" | "[" expr "]" | "?" } ;
call_arg       = expr | ".." expr ;

primary        = INT | FLOAT | STRING | FSTRING | "true" | "false" | "nil"
               | "[" [ expr { "," expr } ] "]"
               | "{" [ expr ":" expr { "," expr ":" expr } ] "}"
               | "(" expr ")"
               | match_expr
               | if_expr
               | entity_expr
               | system_ref
               | field_accessor_fn
               | IDENT "::" IDENT [ "{" { field_init [ "," ] } [ ".." expr ] "}" ]
               | IDENT "{" { field_init [ "," ] } [ ".." expr ] "}"
               | IDENT
               | fn_expr
               | query_expr ;

match_expr     = "match" expr "{" { match_case } "}" ;
if_expr        = "if" expr "{" expr "}" "else" ( if_expr | "{" expr "}" ) ;
system_ref     = "system" "::" IDENT { "::" IDENT } ;
field_accessor_fn = "." IDENT { "." IDENT } ;

fn_expr        = "fn" "(" [ param { "," param } ] ")" [ "->" type_expr ] block ;

query_expr     = "query" "{" [ [ "mut" ] IDENT { "," [ "mut" ] IDENT } ] "}" [ "select" IDENT { "," IDENT } ] [ "where" expr ] ;

(* Terminals *)
IDENT          = ( LETTER | "_" ) { LETTER | DIGIT | "_" } ;
INT            = DIGIT { DIGIT } ;
FLOAT          = DIGIT { DIGIT } "." DIGIT { DIGIT } ;
STRING         = '"' { CHAR } '"' ;
FSTRING        = 'f"' { CHAR | "{" expr [ ":" FORMAT_SPEC ] "}" | "${" expr [ ":" FORMAT_SPEC ] "}" } '"' ;
TRIPLE_FSTRING = 'f"""' { CHAR | "${" expr [ ":" FORMAT_SPEC ] "}" } '"""' ;   (* NB: only ${} interpolates; bare {} are literal *)

FORMAT_SPEC    = { CHAR } ;   (* Python-style mini-language: [[fill]align][sign][#][0][width][.precision][type] *)
```

**Compatibility extension (v0.5 DX mode only):**

```ebnf
match_case_compat_v05 = IDENT [ "{" { IDENT "," } [ IDENT ] [ "," ] [ ".." ] "}" ] "=>" block ;
primary_compat_v05    = ... | IDENT "::" IDENT ;
```

`match_case_compat_v05`: the `..` rest marker, if present, must be the final entry inside the braces. Named bindings after `..` are a parse error (`E2503`).

`primary_compat_v05` allows zero-field sum-variant shorthand (`Type::Variant`) when ambiguity can be resolved.

---
