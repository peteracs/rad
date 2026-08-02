# Installation

## Option 1: Build from source

Requires Rust 1.70+.

```bash
cargo build -p rad-vm --release
```

In this workspace, the binary is written to **`target/release/rad`** at the **repository root** (shared `target/` for all workspace members). Run files directly:

```bash
target/release/rad examples/demo.rad
```

On Windows, use `target\release\rad.exe`.

## Option 2: Forge CLI

Forge is Rad's project tool. It wraps the VM with scaffolding, testing, and formatting commands.

```bash
rad new myproject
cd myproject
rad main.rad
```

See [Using Rad CLI](./forge.md) for the full command reference.

## Editor support

A TextMate grammar for syntax highlighting is available in `tooling/editors/vscode/syntaxes/`. The Rad CLI provides a built-in LSP server (`rad lsp`) with diagnostics, hover, and go-to-definition.
