# calcite — an expression-language interpreter written in Rad

A small, real programming language implemented in Rad, built to exercise the
type checker, the front-end, and the developer tools as hard as possible.

```
source text -> lexer -> Pratt parser -> recursive sum-type AST
            -> tree-walking evaluator -> pretty printer
```

## Run it

```powershell
target\debug\rad.exe projects\dogfood\calcite\main.rad
```

Expected: `33 passed, 0 failed of 33 checks (100.0%)`.

## The language it implements

Numbers, variables, `+ - * / % ^` (with `^` right-associative), comparisons,
`and` / `or` / `not` with short-circuit evaluation, parentheses, the builtins
`min max abs sqrt floor`, `let NAME = EXPR in EXPR`, and
`if COND then EXPR else EXPR`.

```
2 ^ 3 ^ 2                        = 512        (right-associative)
let a = 2 in let b = 3 in a ^ b  = 8
if yes and x == 3 then min(1, 2) else 99
```

## Modules

| File | Contents |
|---|---|
| `ast.rad` | `Expr`, a 7-variant **recursive** sum type; `render`, `node_count`, `depth` |
| `lexer.rad` | `Tok` sum type; `tokenize` returning `Result<list, str>` |
| `parser.rad` | precedence-climbing parser; every step returns `Result<Parsed, str>` |
| `eval.rad` | `Value` sum type; tree-walking `eval_expr` over an environment map |
| `main.rad` | driver: evaluation table, round-trip property, error cases, AST stats |

`main.rad` checks a real property: rendering an AST and re-parsing it must
produce a byte-identical rendering. `render` is fully parenthesized, so this
is a genuine parser/printer round-trip test, not a formatting check.

## What the language made hard

**Recursive sum-type fields.** A variant field is written `name: default`, and
the default doubles as the type hint — which has no answer for a
self-referential field. `rhs: nil` types the field as `nil`; `rhs: Expr = nil`
is a parse error. The only spelling that works is `rhs: Expr`, putting a bare
type name in the default slot. It is undocumented; see
`bugs/06_recursive_sum_type_field.rad`.

**Ignoring a variant field.** Opening braces on an arm forces you to bind every
field, `..` is compat-mode only, and binding fields you do not use produces
unused-variable warnings. The working answer — rename the unwanted bindings as
`lhs: _lhs` — is suggested by no diagnostic. This file's match arms use it
throughout. See `bugs/02_match_rest_hint_contradiction.rad`.

**`::` inside f-strings.** `f"{val(Color::Red { v: 1 })}"` is a parse error, so
every sum-type value must be bound to a local before interpolation. See
`bugs/05_fstring_double_colon.rad`.

**`rad fmt` corrupts this codebase.** The formatter rewrites `Result<int, str>`
as `Result < int, str >`. The sources here are kept in the correct form, so
`rad fmt --check` will fail on them. That is the formatter's bug, not the
code's; see `bugs/01_fmt_mangles_generics.rad`.

## `bugs/` — minimal reproducers

Each file is self-contained and states expected vs actual in its header.

| File | Finding | Status |
|---|---|---|
| `01_fmt_mangles_generics.rad` | `rad fmt` spaces out every generic type argument, and `fmt --check` then enforces it | reported |
| `02_match_rest_hint_contradiction.rad` | checker hints "Use `..`"; `..` is rejected in default mode | **fixed** |
| `02b_match_rest_rejected.rad` | applying that hint (E2503), plus cascading bogus errors | — |
| `03_pub_fn_not_reachability_root.rad` | `pub fn` is not a reachability root, so library helpers are "unused" and `--deny-warnings` fails | **fixed** |
| `04_lint_lib.rad` + `04_lint_importer.rad` | `rad lint` attributes a library's diagnostics to the importing file, wrong line and wrong variable | reported |
| `05_fstring_double_colon.rad` | any `::` path inside an f-string interpolation is a parse error | reported |
| `06_recursive_sum_type_field.rad` | no documented way to declare a recursive variant field | reported |
| `07_guarded_arms_defeat_exhaustiveness.rad` | **soundness:** all-guarded arms passed exhaustiveness and returned `nil` from a `-> str` function | **fixed** |

The three fixed items were fixed in `core/vm/src/checker/` (`typeck.rs`,
`reachability.rs`) with regression tests in `checker/tests.rs`. Their
reproducers now behave correctly when run against a binary built from current
source; the shared `target\debug\rad.exe` predates the fixes.

## `checker/` — diagnostics gallery

19 deliberately illegal programs plus a runner:

```powershell
powershell -File projects\dogfood\calcite\checker\run_gallery.ps1
```

Every `t*.rad` must be **rejected**; an `ACCEPTED` verdict is a checker hole.
This is how bug 07 was found — it was the only one of the 19 that compiled.
