# Sovereign Grid completion receipt

Status: **language-surface, hard-subsecond, and local release certification complete;
clean-commit publication remains in the [language-surface certification](../../../docs/src/project/language-reference-and-ultimate-dogfood-todo.md).**

The original implementation order is closed:

- [x] Transactional state transitions retain atomic `$transition` semantics.
- [x] Generic type aliases substitute recursively under strict checking.
- [x] The generated inventory covers 203 source forms, six internal forms, and
  all 257 builtins.
- [x] The stable production workflow executes every canonical syntax row.
- [x] Experimental causal settlements and relations, model checking, and FFI
  execute under their exact feature boundaries.
- [x] Thirty-three negative fixtures match their intended diagnostics; replay
  evidence is deterministic across worker counts.
- [x] Equivalent baseline and indexed workloads establish 307.16× release-profile median
  wall-time and 53,847.38× instruction improvements over 30 release samples.
- [x] The complete local acceptance runner is green and emits the source-bound
  report at `target/sovereign-grid-acceptance/report.json`.
- [x] Enforce less than 1,000,000,000 ns end-to-end for every Sovereign Grid
  run, test, negative, model, relation, FFI, replay, inspection, documentation
  example, and benchmark invocation without reducing its workload.
- [ ] Reissue the clean-commit acceptance receipt after the cumulative
  11-project portfolio and published documentation pass the same limit.

`main.rad` remains only one acceptance cell. The authoritative pass condition
is `accept.ps1`, the generated coverage manifest, and the repository release
gates together.
