# Compiler issue found by the mathematical search

The search uses a pure function with a local byte-buffer queue. Its queue
never escapes until the function has finished using it; updates are candidates
for the existing in-place bytecode optimization.

`Compiler::may_run_in_causal_region` classified **every** checked pure or
readonly helper as potentially causal, including programs compiled without
the experimental causal-laws feature. Thus each queue write copied the entire
preallocated queue. The work grew with both explored states and buffer capacity.

The reproduction is `single.rad -- 8 2`; it exhausts exactly 6,529 reachable
uncertainty states. The initial 2..10 matrix was stopped before completion
after this issue was identified, and is not counted as successful evidence.

The fix in `core/vm/src/compiler/lifecycle.rs` returns false immediately when
`causal_laws` is absent. Causal-enabled programs retain the conservative
lowering and runtime firewall. No mathematical opcode, solver builtin, or
precomputed answer was added to the VM.

The regression in `compiler/tests/byte_buffers.rs` uses the checked compilation
pipeline. It verifies executed in-place u8/u16/u32 opcodes for an ordinary pure
helper, verifies their absence when the same helper runs inside a law, and
checks that an aliased buffer remains independent in both modes.

Validation: the seven byte-buffer tests passed, followed by the complete
release VM unit executable built with `--no-default-features`: 1,288 passed,
four ignored. That Cargo switch excludes the optional Wasmtime host; it does
not disable source-level causal tests. Native CLI and research validation use
the default-feature release build.

`benchmark_fix.py` compares the old and new binaries on exactly the same
eight-state input and rejects any mathematical-output difference. Its measured
receipt is [compiler-fix-benchmark.json](compiler-fix-benchmark.json), copied
from `out/compiler-fix-benchmark.json`. Three alternating runs per binary gave
median wall times of 8.9136 s before and 0.15525 s after: a 57.41x speedup on
this input and machine, including process startup and compilation. All six
runs exhausted exactly 6,529 states and returned the same rejection.

The complete n=2..10 experiment now passes: 27 independently checked cases,
two worker counts, and three verified record/replay pairs. This includes the
causal-enabled audit, where the optimization intentionally remains disabled.

## Artifact hashing issue found during the follow-up

The original symbolic runner hashed LF text in memory, while Windows text
output translated its saved SMT queries and proof objects to CRLF. The solver
outcomes were unaffected, but the recorded hashes did not describe the actual
file bytes. Direct inspection reproduced the mismatch on all 44 query/proof
artifacts in the expanded campaign before the fix.

The runner now writes those artifacts with explicit LF newlines and reads
every file back to verify its recorded SHA-256. `test_symbolic.py` also changes
saved query and proof bytes and requires rejection. The regenerated receipts
supersede the old symbolic hashes. This is a Python artifact-generation fix;
the RAD compiler fix above is unchanged.
