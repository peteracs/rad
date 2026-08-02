# Rad RFCs (Request for Comments)

RFCs are the process for proposing changes to the Rad language, its semantics, or its core tooling.

## When You Need an RFC

- New syntax or keywords
- Changes to the type system
- Changes to ECS, pipeline, or event semantics
- New standard library modules
- Changes to the module system
- Anything marked "Needs RFC" on the [roadmap](roadmap.md)

## When You Don't Need an RFC

- Bug fixes
- New builtin functions (open a feature request issue instead)
- Documentation improvements
- Tooling improvements (LSP, formatter, playground)
- New examples

## Process

1. **Draft** — Copy the [RFC template](rfc-template.md), fill it in, open a PR
2. **Under Review** — Community discusses in PR comments (1–2 weeks)
3. **Accepted** — Maintainer merges; implementation can begin
4. **Rejected** — Maintainer closes with rationale; can be reopened if circumstances change

## Index

| RFC | Title | Status |
|-----|-------|--------|
| [0001](../../rfcs/0001-causal-settlements.md) | Causal Settlements—Typed Intents, Laws, and Resolvers | Accepted for experimental implementation |
| [0000](rfc-template.md) | RFC Template | — |
