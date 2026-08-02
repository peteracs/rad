# Causal Laws semantic fixtures

These fixtures are the executable contract for RFC-0001. They intentionally
land before the implementation. Run them with the authoritative VM snapshot
harness after enabling the experimental feature:

```text
rad snapshot tests/fixtures/causal-laws --experimental-laws
```

Positive fixtures use `// expect:` assertions. Negative fixtures use the
existing `// expect-runtime-error:` directive because checker diagnostics are
reported through the same snapshot-harness error channel.
