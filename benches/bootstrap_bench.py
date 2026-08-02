"""Bootstrap benchmark: measures compilation speed across bootstrap stages.

Stage 1 -- Rust VM compiles & runs stress_test.rad (full grammar).
Stage 2 -- Self-hosted Rad compiler (lexer.rad + parser.rad) parses
           stress_subset.rad, running on the Rust VM.
Stage 3 -- Full C backend pipeline:
    3a  emit_c.rad generates C (via Rust VM)
    3b  gcc -O2 compiles the C
    3c  native binary execution
"""

import argparse
import os
import shutil
import statistics
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

RAD = os.path.join(REPO, "target", "release",
                   "rad.exe" if os.name == "nt" else "rad")

STRESS_FULL = os.path.join(REPO, "benches", "stress_test.rad")
STRESS_SUBSET = os.path.join(REPO, "benches", "stress_subset.rad")

COMPILER_DIR = os.path.join(REPO, "core", "c-backend", "src")
TARGET_DIR = os.path.join(REPO, "core", "c-backend", "target")

GEN_C = os.path.join(TARGET_DIR, "generated_stress_subset.c")
GEN_EXE = os.path.join(TARGET_DIR,
                        "generated_stress_subset.exe" if os.name == "nt"
                        else "generated_stress_subset")

GCC = "gcc"


def ensure_path():
    """Put gcc's bin directory first so cc1.exe loads matching MinGW DLLs (Windows)."""
    gcc = shutil.which("gcc")
    if gcc:
        bin_dir = os.path.dirname(os.path.abspath(gcc))
        os.environ["PATH"] = bin_dir + os.pathsep + os.environ.get("PATH", "")


def timed(cmd, *, cwd, runs, timeout=120):
    """Run *cmd* `runs` times, return (list-of-seconds, None) or (None, err)."""
    times = []
    for _ in range(runs):
        t0 = time.perf_counter()
        p = subprocess.run(cmd, cwd=cwd, capture_output=True,
                           text=True, timeout=timeout)
        dt = time.perf_counter() - t0
        if p.returncode != 0:
            err = (p.stderr or p.stdout or "")[:400]
            return None, err
        times.append(dt)
    return times, None


def stage1(runs):
    """Rust VM baseline: compile + run full stress test."""
    print("Stage 1  Rust VM  (full grammar)")
    ts, err = timed([RAD, STRESS_FULL], cwd=REPO, runs=runs)
    if err:
        print(f"  FAIL  {err}")
        return None
    med = statistics.median(ts)
    print(f"  median {med:.4f}s   (all: {['%.4f' % t for t in ts]})")
    return med


def stage2(runs):
    """Self-hosted parser running on the Rust VM."""
    print("Stage 2  Rad-in-Rad on VM  (subset grammar)")

    subset_path = STRESS_SUBSET.replace("\\", "/")
    temp = os.path.join(COMPILER_DIR, "_bench_main.rad")
    try:
        with open(temp, "w") as f:
            f.write(
                'use "lexer.rad"\n'
                'use "parser.rad"\n\n'
                "fn main() -> nil {\n"
                f'    let src = read_file("{subset_path}")\n'
                "    let tokens = lex(src)\n"
                "    let ast = parse(tokens)\n"
                "    inspect_ast(ast)\n"
                "}\n"
            )
        ts, err = timed([RAD, temp], cwd=REPO, runs=runs)
        if err:
            print(f"  FAIL  {err}")
            return None
        med = statistics.median(ts)
        print(f"  median {med:.4f}s   (all: {['%.4f' % t for t in ts]})")
        return med
    finally:
        if os.path.exists(temp):
            os.remove(temp)


def stage3_emit(runs):
    """3a: emit_c.rad generates C via the Rust VM."""
    print("Stage 3a  C emit (rad-in-rad)")

    subset_path = STRESS_SUBSET.replace("\\", "/")
    gen_c_path = GEN_C.replace("\\", "/")
    temp = os.path.join(COMPILER_DIR, "_bench_emit.rad")
    try:
        with open(temp, "w") as f:
            f.write(
                'use "emit_c.rad"\n\n'
                "fn main() -> nil {\n"
                f'    compile_file_to_c("{subset_path}", "{gen_c_path}")\n'
                "}\n"
            )
        ts, err = timed([RAD, temp], cwd=REPO, runs=runs)
        if err:
            print(f"  FAIL  {err}")
            return None
        med = statistics.median(ts)
        print(f"  median {med:.4f}s   (all: {['%.4f' % t for t in ts]})")
        return med
    finally:
        if os.path.exists(temp):
            os.remove(temp)


def stage3_gcc(runs):
    """3b: gcc -O2 compiles the generated C."""
    print("Stage 3b  gcc -O2 compile")
    if not os.path.isfile(GEN_C):
        print("  SKIP  (no generated .c file)")
        return None
    ts, err = timed([GCC, "-O2", GEN_C, "-I", COMPILER_DIR, "-o", GEN_EXE],
                    cwd=REPO, runs=runs)
    if err:
        print(f"  FAIL  {err}")
        return None
    med = statistics.median(ts)
    print(f"  median {med:.4f}s   (all: {['%.4f' % t for t in ts]})")
    return med


def stage3_exec(runs):
    """3c: run the compiled native binary."""
    print("Stage 3c  Native execution")
    if not os.path.isfile(GEN_EXE):
        print("  SKIP  (no compiled binary)")
        return None
    ts, err = timed([GEN_EXE], cwd=REPO, runs=runs)
    if err:
        print(f"  FAIL  {err}")
        return None
    med = statistics.median(ts)
    print(f"  median {med:.4f}s   (all: {['%.4f' % t for t in ts]})")
    return med


def main():
    ap = argparse.ArgumentParser(description="Rad Bootstrap Benchmark")
    ap.add_argument("--runs", type=int, default=5)
    args = ap.parse_args()

    ensure_path()

    if not os.path.isfile(RAD):
        sys.exit(f"Missing {RAD}  -- build with: cargo build --release -p rad-vm")

    for f in (STRESS_FULL, STRESS_SUBSET):
        if not os.path.isfile(f):
            sys.exit(f"Missing {f}")

    os.makedirs(TARGET_DIR, exist_ok=True)
    print(f"Runs per stage: {args.runs}\n")

    t1 = stage1(args.runs)
    print()
    t2 = stage2(args.runs)
    print()
    t3a = stage3_emit(args.runs)
    print()
    t3b = stage3_gcc(args.runs)
    print()
    t3c = stage3_exec(args.runs)

    t3_total = None
    if t3a is not None and t3b is not None and t3c is not None:
        t3_total = t3a + t3b + t3c

    print("\n=== Results ===\n")
    print(f"{'Stage':<35} {'Median':>10} {'vs Rust':>10}")
    print("-" * 57)

    rows = [
        ("Rust VM (full)", t1),
        ("Rad-in-Rad on VM (subset)", t2),
        ("C emit (rad-in-rad)", t3a),
        ("gcc -O2 compile", t3b),
        ("Native C execution", t3c),
        ("Full C pipeline (emit+gcc+run)", t3_total),
    ]
    for name, t in rows:
        if t is None:
            print(f"{name:<35} {'SKIP':>10} {'':>10}")
        else:
            ratio = f"{t / t1:.1f}x" if t1 else "-"
            print(f"{name:<35} {t:>9.4f}s {ratio:>10}")


if __name__ == "__main__":
    main()
