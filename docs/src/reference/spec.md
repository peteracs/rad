# Rad Language Specification

## Version: 0.5 (parser-surface certified)

---

This document describes the current v0.5 language and is the normative syntax
entry point. The generated inventory certifies all 203 public parser-owned
syntax forms, identifies six internal recovery forms, and binds all 262 public
builtins to executable documentation and Sovereign Grid evidence. The exact
acceptance receipt is the [Language reference and ultimate dogfood
certification](../project/language-reference-and-ultimate-dogfood-todo.md).

RAD has one canonical source grammar. Rest patterns and zero-field variant
shorthand are ordinary language syntax; there is no alternate parser mode.

---

{{#include spec_parts/lexical.md}}
{{#include spec_parts/types.md}}
{{#include spec_parts/declarations.md}}
{{#include spec_parts/statements.md}}
{{#include spec_parts/expressions.md}}
{{#include spec_parts/builtins.md}}
{{#include spec_parts/execution.md}}
{{#include spec_parts/effects_and_errors.md}}
