# Candidate-constraint semantic fixtures

These fixtures are the executable contract for RFC-0002. Run them with:

```text
rad snapshot --experimental-laws tests/fixtures/causal-constraints
```

They cover immutable base and complete candidate reads, watched-component
triggering and deduplication, canonical all-outcome collection, atomic
rejection, effect safety, stable violation codes, and the feature gate.
