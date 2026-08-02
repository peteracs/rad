## What does this PR do?

<!-- One sentence summary. Link to the issue if applicable: Fixes #123 -->

## Which area does this touch?

- [ ] Lexer / Parser
- [ ] Checker / Type system
- [ ] Compiler / Opcodes
- [ ] VM / Runtime
- [ ] Builtins
- [ ] Tooling (CLI, LSP, playground)
- [ ] Examples
- [ ] Documentation
- [ ] CI / Infrastructure

## Checklist

- [ ] `cargo test -p rad-vm` passes
- [ ] `cargo run -p rad-vm --bin rad -- snapshot tests/` passes
- [ ] `cargo clippy -p rad-vm -- -D warnings` is clean
- [ ] New/changed behavior has a conformance test in `tests/conformance/`
- [ ] `docs/src/reference/spec.md` updated (if syntax/semantics changed)
- [ ] CHANGELOG.md updated under `[Unreleased]`

## How to test this

<!-- Steps a reviewer can follow to verify the change works. -->

```bash
# Example:
rad examples/your_example.rad
```

## Screenshots / output

<!-- If applicable, paste terminal output or screenshots. -->
