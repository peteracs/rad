"""Self-healing test harness for the Rad C backend.

Runs the full pipeline: emit_c.rad -> gcc -> execute, then diffs
the output against the Rust VM reference. Reports the first divergent
line on failure, and timing for each step on success.

Usage:
    py core/c-backend/test_c_backend.py [--keep] [--asan] [--debug-arena]

    --asan          Compile with -fsanitize=address (Linux/Mac only)
    --debug-arena   Compile with -DRAD_DEBUG_ARENA canary guards (all platforms)

Exit codes:
    0 = PASS (all tests match)
    1 = FAIL (divergence or build error)
"""

import os
import shutil
import subprocess
import sys
import time

if os.environ.get("RAD_RUN_FROZEN_C_BACKEND") != "1":
    print("C backend is frozen legacy code and is not part of normal Rad health checks.")
    print("core/vm is the ground truth. Set RAD_RUN_FROZEN_C_BACKEND=1 to run this historical harness.")
    sys.exit(2)

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

RAD = os.path.join(REPO, "target", "release",
                   "rad.exe" if os.name == "nt" else "rad")
GCC = "gcc"

COMPILER_DIR = os.path.join(REPO, "core", "c-backend", "src")
TARGET_DIR = os.path.join(REPO, "core", "c-backend", "target")

TEST_CASES = [
    ("stress_subset",           os.path.join(REPO, "benches", "stress_subset.rad")),
    ("test_ecs",                os.path.join(REPO, "benches", "test_ecs.rad")),
    ("test_closures",           os.path.join(REPO, "benches", "test_closures.rad")),
    ("test_match_literals",     os.path.join(REPO, "benches", "test_match_literals.rad")),
    ("test_lexer_standalone",   os.path.join(REPO, "benches", "test_lexer_standalone.rad")),
    ("test_multiline_string",   os.path.join(REPO, "benches", "test_multiline_string.rad")),
    ("test_parser_standalone",  os.path.join(REPO, "benches", "test_parser_standalone.rad")),
    ("test_emit_c_standalone",  os.path.join(REPO, "benches", "test_emit_c_standalone.rad")),
    ("test_platinum",           os.path.join(REPO, "benches", "test_platinum.rad")),
    ("test_diamond",            os.path.join(REPO, "benches", "test_diamond.rad")),
    ("test_value_types",        os.path.join(REPO, "benches", "test_value_types.rad")),
]

NEGATIVE_CASES = [
    ("neg_type_mismatch",     os.path.join(REPO, "tests", "conformance", "negative_type_mismatch.rad"),
     "Type mismatch"),
    ("neg_wrong_arity",       os.path.join(REPO, "tests", "conformance", "negative_wrong_arity.rad"),
     "expects 1 argument"),
    ("neg_immutable_assign",  os.path.join(REPO, "tests", "conformance", "negative_immutable_assign.rad"),
     "Cannot assign to immutable"),
]


def ensure_path():
    """Put gcc's bin directory first so cc1.exe loads matching MinGW DLLs (Windows)."""
    gcc = shutil.which("gcc")
    if gcc:
        bin_dir = os.path.dirname(os.path.abspath(gcc))
        os.environ["PATH"] = bin_dir + os.pathsep + os.environ.get("PATH", "")


def step(label, cmd, cwd=REPO, timeout=120):
    t0 = time.perf_counter()
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True,
                       timeout=timeout)
    dt = time.perf_counter() - t0
    if p.returncode != 0:
        err = (p.stderr or p.stdout or "<no output>").strip()
        print(f"  FAIL  [{label}] exit code {p.returncode}")
        for line in err.splitlines()[:30]:
            print(f"    | {line}")
        if not err and "gcc" in label.lower():
            print("    | (no compiler output — on Windows, ensure the same bin/ as `gcc` "
                  "is first on PATH so cc1.exe loads MinGW DLLs; see CONTRIBUTING.md)")
        return None, dt
    print(f"    {label:<30} {dt:.4f}s")
    return p.stdout, dt


def run_test(name, source_path, keep=False, asan=False, debug_arena=False):
    """Run one test case. Returns True on pass."""
    ensure_path()
    ext = ".exe" if os.name == "nt" else ""
    gen_c = os.path.join(TARGET_DIR, f"generated_{name}.c")
    gen_exe = os.path.join(TARGET_DIR, f"generated_{name}{ext}")

    src_path_fwd = source_path.replace("\\", "/")
    gen_c_fwd = gen_c.replace("\\", "/")

    emit_src = os.path.join(COMPILER_DIR, f"_test_emit_{name}.rad")
    try:
        with open(emit_src, "w") as f:
            f.write(
                'use "emit_c.rad"\n\n'
                "fn main() -> nil {\n"
                f'    compile_file_to_c("{src_path_fwd}", "{gen_c_fwd}")\n'
                "}\n"
            )

        out, _ = step("emit_c.rad (generate C)", [RAD, emit_src])
        if out is None:
            return False

        gcc_flags = [GCC, "-O2"]
        if os.environ.get("RAD_C_RELEASE") == "1":
            gcc_flags.append("-DRAD_RELEASE")
        if asan:
            gcc_flags = [GCC, "-O1", "-g", "-fsanitize=address",
                         "-fno-omit-frame-pointer"]
        elif debug_arena:
            gcc_flags = [GCC, "-O1", "-g", "-DRAD_DEBUG_ARENA"]
        gcc_flags += [gen_c, "-I", COMPILER_DIR, "-o", gen_exe,
                      "-Wl,--stack,33554432"]

        out, _ = step("gcc (compile C)", gcc_flags)
        if out is None:
            return False

        t0_exec = time.perf_counter()
        c_result = subprocess.run([gen_exe], cwd=REPO, capture_output=True,
                                   text=True, timeout=120)
        t_exec = time.perf_counter() - t0_exec
        c_out = c_result.stdout
        c_err = c_result.stderr or ""

        if c_result.returncode != 0 and not asan and not debug_arena:
            err = (c_err or c_out or "<no output>").strip()
            print(f"  FAIL  [native binary] exit code {c_result.returncode}")
            for line in err.splitlines()[:15]:
                print(f"    | {line}")
            return False
        if asan and "ERROR: AddressSanitizer" in c_err:
            print(f"  FAIL  ASAN errors detected:")
            for line in c_err.splitlines()[:30]:
                print(f"    | {line}")
            return False
        if debug_arena:
            if "CORRUPTION DETECTED" in c_err:
                print(f"  FAIL  Arena corruption detected:")
                for line in c_err.splitlines()[:30]:
                    print(f"    | {line}")
                return False
            if c_result.returncode != 0:
                err = (c_err or c_out or "<no output>").strip()
                print(f"  FAIL  [native binary] exit code {c_result.returncode}")
                for line in err.splitlines()[:15]:
                    print(f"    | {line}")
                return False
            arena_ok = [l for l in c_err.splitlines() if "[DEBUG_ARENA] OK:" in l]
            if arena_ok:
                print(f"    {arena_ok[0].strip()}")
        print(f"    {'native binary (execute)':<30}")

        # Benchmark fixtures are not required to satisfy the Rust typechecker;
        # compare against runtime output with checking disabled.
        ref_out, t_ref = step("Rust VM reference (execute)",
                              [RAD, "--no-check", source_path])
        if ref_out is None:
            return False

    finally:
        if os.path.isfile(emit_src) and not keep:
            os.remove(emit_src)

    c_lines = c_out.strip().splitlines()
    ref_lines = ref_out.strip().splitlines()

    c_elapsed = None
    ref_elapsed = None
    max_lines = max(len(c_lines), len(ref_lines))
    for i in range(max_lines):
        cl = c_lines[i] if i < len(c_lines) else "<missing>"
        rl = ref_lines[i] if i < len(ref_lines) else "<missing>"
        if cl.startswith("ELAPSED:") and rl.startswith("ELAPSED:"):
            try:
                c_elapsed = float(cl.split(":", 1)[1])
                ref_elapsed = float(rl.split(":", 1)[1])
            except ValueError:
                pass
            continue
        if cl != rl:
            print(f"  FAIL  Output diverges at line {i + 1}:")
            print(f"    expected: {rl}")
            print(f"    got:      {cl}")
            return False

    if len(c_lines) != len(ref_lines):
        print(f"  FAIL  Line count differs: C={len(c_lines)}"
              f" vs Rust={len(ref_lines)}")
        return False

    timing = f"  Native: {t_exec:.4f}s  Rust VM: {t_ref:.4f}s"
    if t_exec > 0:
        timing += f"  speedup: {t_ref / t_exec:.1f}x"
    if c_elapsed is not None and ref_elapsed is not None:
        timing += f"  [in-proc: C={c_elapsed:.4f}s VM={ref_elapsed:.4f}s"
        if c_elapsed > 0:
            timing += f" speedup={ref_elapsed/c_elapsed:.1f}x"
        timing += "]"
    print(f"  PASS  All {len(ref_lines)} lines match.{timing}")
    return True


def run_separate_test(keep=False, debug_arena=False):
    """Test separate compilation mode: emit per-module files, compile & link."""
    ensure_path()
    source_path = os.path.join(REPO, "tests", "conformance", "test_separate_multi.rad")
    sep_dir = os.path.join(TARGET_DIR, "separate_test")
    if os.path.isdir(sep_dir):
        import shutil
        shutil.rmtree(sep_dir)
    os.makedirs(sep_dir, exist_ok=True)
    ext = ".exe" if os.name == "nt" else ""
    gen_exe = os.path.join(sep_dir, f"separate_test{ext}")

    src_fwd = source_path.replace("\\", "/")
    sep_fwd = sep_dir.replace("\\", "/")

    emit_src = os.path.join(COMPILER_DIR, "_test_separate_emit.rad")
    try:
        with open(emit_src, "w") as f:
            f.write(
                'use "emit_c.rad"\n\n'
                "fn main() -> nil {\n"
                f'    compile_file_separate("{src_fwd}", "{sep_fwd}")\n'
                "}\n"
            )
        out, _ = step("emit separate modules", [RAD, emit_src])
        if out is None:
            return False

        c_files = sorted([f for f in os.listdir(sep_dir) if f.endswith(".c")])
        if not c_files:
            print(f"  FAIL  No .c files generated in {sep_dir}")
            return False

        obj_files = []
        gcc_base = [GCC, "-O2", "-DRAD_SEPARATE_COMPILATION",
                    "-I", COMPILER_DIR, "-I", sep_dir]
        if os.environ.get("RAD_C_RELEASE") == "1":
            gcc_base.append("-DRAD_RELEASE")
        if debug_arena:
            gcc_base = [GCC, "-O1", "-g", "-DRAD_SEPARATE_COMPILATION",
                        "-DRAD_DEBUG_ARENA", "-I", COMPILER_DIR, "-I", sep_dir]

        runtime_obj = os.path.join(sep_dir, "runtime.o")
        out_r, _ = step("gcc (compile runtime.c)",
                        gcc_base + ["-c", os.path.join(COMPILER_DIR, "runtime.c"),
                                    "-o", runtime_obj])
        if out_r is None:
            return False
        obj_files.append(runtime_obj)

        for cf in c_files:
            obj = os.path.join(sep_dir, cf.replace(".c", ".o"))
            out_c, _ = step(f"gcc (compile {cf})",
                            gcc_base + ["-c", os.path.join(sep_dir, cf), "-o", obj])
            if out_c is None:
                return False
            obj_files.append(obj)

        out_link, _ = step("gcc (link)", [GCC, "-O2"] + obj_files + ["-o", gen_exe])
        if out_link is None:
            return False

        t0 = time.perf_counter()
        p = subprocess.run([gen_exe], cwd=REPO, capture_output=True, text=True,
                           timeout=120)
        t_exec = time.perf_counter() - t0
        c_out = p.stdout

        ref_out, t_ref = step("Rust VM reference",
                              [RAD, "--no-check", source_path])
        if ref_out is None:
            return False

        c_lines = c_out.strip().splitlines()
        ref_lines = ref_out.strip().splitlines()
        for i in range(max(len(c_lines), len(ref_lines))):
            cl = c_lines[i] if i < len(c_lines) else "<missing>"
            rl = ref_lines[i] if i < len(ref_lines) else "<missing>"
            if cl != rl:
                print(f"  FAIL  Output diverges at line {i + 1}:")
                print(f"    expected: {rl}")
                print(f"    got:      {cl}")
                return False

        print(f"  PASS  Separate compilation: {len(c_files)} modules, "
              f"{len(ref_lines)} lines match.  "
              f"Native: {t_exec:.4f}s  Rust VM: {t_ref:.4f}s")
        return True
    finally:
        if os.path.isfile(emit_src) and not keep:
            os.remove(emit_src)


def run_negative_test(name, source_path, expected_fragment):
    """Verify that compilation is rejected with a matching error message."""
    ensure_path()
    emit_src = os.path.join(COMPILER_DIR, f"_test_neg_{name}.rad")
    src_fwd = source_path.replace("\\", "/")
    gen_c = os.path.join(TARGET_DIR, f"generated_{name}.c").replace("\\", "/")
    try:
        with open(emit_src, "w") as f:
            f.write(
                'use "emit_c.rad"\n\n'
                "fn main() -> nil {\n"
                f'    compile_file_to_c("{src_fwd}", "{gen_c}")\n'
                "}\n"
            )
        p = subprocess.run([RAD, emit_src], cwd=REPO,
                           capture_output=True, text=True, timeout=60)
        combined = (p.stdout or "") + (p.stderr or "")
        if p.returncode == 0:
            print(f"  FAIL  Expected compilation to fail, but it succeeded")
            return False
        if expected_fragment in combined:
            print(f"  PASS  Correctly rejected: {expected_fragment}")
            return True
        print(f"  FAIL  Compilation failed but missing expected error fragment")
        print(f"    expected: '{expected_fragment}'")
        for line in combined.strip().splitlines()[:10]:
            print(f"    | {line}")
        return False
    finally:
        if os.path.isfile(emit_src):
            os.remove(emit_src)


def main():
    keep = "--keep" in sys.argv
    asan = "--asan" in sys.argv
    debug_arena = "--debug-arena" in sys.argv
    ensure_path()

    if not os.path.isfile(RAD):
        sys.exit(f"Missing Rust VM binary: {RAD}\n"
                 f"  Build with: cargo build --release --bin rad  ")

    os.makedirs(TARGET_DIR, exist_ok=True)

    if asan:
        mode = "ASAN"
    elif debug_arena:
        mode = "Debug Arena"
    else:
        mode = "Normal"
    print(f"=== C Backend Test Suite ({mode}) ===\n")

    passed = 0
    failed = 0
    for name, src in TEST_CASES:
        print(f"  [{name}]")
        if run_test(name, src, keep=keep, asan=asan, debug_arena=debug_arena):
            passed += 1
        else:
            failed += 1
        print()

    # Separate compilation test
    print(f"--- Separate Compilation Test ---\n")
    print(f"  [test_separate]")
    if run_separate_test(keep=keep, debug_arena=debug_arena):
        passed += 1
    else:
        failed += 1
    print()

    print(f"--- Negative Tests (should be rejected) ---\n")
    for name, src, frag in NEGATIVE_CASES:
        print(f"  [{name}]")
        if run_negative_test(name, src, frag):
            passed += 1
        else:
            failed += 1
        print()

    print(f"Results: {passed} passed, {failed} failed")
    sys.exit(0 if failed == 0 else 1)


if __name__ == "__main__":
    main()
