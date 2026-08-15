# Experiments

This directory preserves non-authoritative implementation experiments.
Nothing here defines current RAD language behavior or participates in normal
release health.

- `c-backend/` is the frozen self-hosted C/AOT experiment. Its opt-in harnesses
  are retained for archaeology and possible future research.

An experiment graduates only by acquiring an explicit owner, supported API,
tests, documentation, and release gates. It does not become core merely by
being performance-sensitive.
