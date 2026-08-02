---
name: Bug Report
about: Report a bug in the Rad language or tooling
title: "[Bug] "
labels: bug
assignees: ''
---

## Where it reproduces

- [ ] `rad` CLI (run, test, fmt, lint, `rad new`, snapshot, `rad lsp`, etc.)
- [ ] Browser playground / WASM

## Description

A clear description of the bug.

## Steps to Reproduce

```rad
// Minimal .rad code that reproduces the issue
```

**Command used:**
```bash
rad repro.rad
```

## Expected Behavior

What you expected to happen.

## Actual Behavior

What actually happened. Include the full error output if applicable.

## Environment

- OS: [e.g., Windows 11, macOS 14, Ubuntu 22.04]
- Rust version: [e.g., 1.76] — primary for `rad` and `rad lsp` (LSP is implemented in Rust, not Python)
- Python version: [e.g., 3.11] — only if the bug involves Python helper scripts (e.g. `core/c-backend/test_conformance_c.py`, `core/c-backend/test_c_backend.py`, `benches/compare.py`)
