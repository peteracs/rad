"""Conformance matrix: run conformance tests through the C backend.

For each test:
  1. emit_c.rad batch-generates C from all .rad sources
  2. A C compiler (TCC or GCC) compiles each .c file
  3. The native binary runs and output is compared to // expect: directives

By default, TCC is preferred when available (--compiler auto) for ~5x faster
compile times. Use --compiler gcc to force GCC, or --compiler tcc to require TCC.

Usage:
    py core/c-backend/test_conformance_c.py [--verbose] [--filter PATTERN] [--compiler {gcc,tcc,auto}]
"""

import argparse
import atexit
import glob
import os
import re
import signal
import shutil
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass

if os.environ.get("RAD_RUN_FROZEN_C_BACKEND") != "1":
    print("C backend is frozen legacy code and is not part of normal Rad health checks.")
    print("core/vm is the ground truth. Set RAD_RUN_FROZEN_C_BACKEND=1 to run this historical harness.")
    sys.exit(2)

_child_procs: list[subprocess.Popen] = []


def _cleanup_children():
    for p in _child_procs:
        try:
            if p.poll() is None:
                p.kill()
                p.wait(timeout=3)
        except Exception:
            pass


atexit.register(_cleanup_children)

if os.name == "nt":
    signal.signal(signal.SIGBREAK, lambda *_: (_cleanup_children(), sys.exit(1)))
signal.signal(signal.SIGTERM, lambda *_: (_cleanup_children(), sys.exit(1)))

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CONFORMANCE_DIR = os.path.join(REPO, "tests", "conformance")
COMPILER_DIR = os.path.join(REPO, "core", "c-backend", "src")
TARGET_ROOT = os.path.join(REPO, "core", "c-backend", "target", "conformance_c")

RAD = os.path.join(REPO, "target", "release", "rad.exe" if os.name == "nt" else "rad")
GCC = "gcc"
TCC_COMPAT = os.path.join(COMPILER_DIR, "tcc_compat.c")
EXT = ".exe" if os.name == "nt" else ""

TCC_SEARCH_PATHS = [
    os.path.join("D:\\msys64", "opt", "tcc-dev", "tcc.exe"),
    os.path.join("D:\\msys64", "opt", "tcc", "tcc", "tcc.exe"),
]


def find_tcc() -> str | None:
    found = shutil.which("tcc")
    if found:
        return found
    for p in TCC_SEARCH_PATHS:
        if os.path.isfile(p):
            return p
    return None

EXPECT_LINE = re.compile(r"^\s*//\s*expect:\s*(.*)$")
EXPECT_RUNTIME_ERROR = re.compile(r"^\s*//\s*expect-runtime-error:\s*(.+)\s*$")
BACKEND_DIRECTIVE = re.compile(r"^\s*//\s*backend:\s*(both|rust)\s*$")

STATUSES = [
    "PASS",
    "SKIP",
    "EMIT_FAIL",
    "GCC_FAIL",
    "RUN_FAIL",
    "OUTPUT_MISMATCH",
    "ERROR_MISMATCH",
]


@dataclass
class TestCase:
    name: str
    rad_path: str
    expected: list[str]
    runtime_error: str | None
    backend: str
    gen_c: str
    gen_exe: str

    @property
    def runnable(self) -> bool:
        return self.backend != "rust" and (bool(self.expected) or self.runtime_error is not None)


class ProgressDisplay:
    def __init__(self, enabled: bool, width: int = 28):
        self.enabled = enabled
        self.width = width
        self._active = False
        self._last_len = 0

    def _write_live(self, text: str) -> None:
        if not self.enabled:
            return
        padded = text
        if len(text) < self._last_len:
            padded += " " * (self._last_len - len(text))
        sys.stdout.write("\r" + padded)
        sys.stdout.flush()
        self._last_len = max(self._last_len, len(text))
        self._active = True

    def bar(self, stage: str, done: int, total: int, detail: str = "") -> None:
        if total <= 0:
            text = f"{stage:8s} [done]"
            if detail:
                text += f" {detail}"
            self._write_live(text)
            return
        ratio = min(1.0, max(0.0, done / total))
        filled = int(self.width * ratio)
        bar = "#" * filled + "-" * (self.width - filled)
        percent = int(ratio * 100)
        text = f"{stage:8s} [{bar}] {done:>3}/{total:<3} {percent:>3}%"
        if detail:
            text += f" | {detail}"
        self._write_live(text)

    def spinner(self, stage: str, elapsed_s: float, detail: str = "") -> None:
        frames = "|/-\\"
        frame = frames[int(elapsed_s * 10) % len(frames)]
        text = f"{stage:8s} [{frame}] {elapsed_s:5.1f}s"
        if detail:
            text += f" | {detail}"
        self._write_live(text)

    def println(self, line: str = "") -> None:
        if self.enabled and self._active:
            sys.stdout.write("\r" + (" " * self._last_len) + "\r")
            sys.stdout.flush()
            self._active = False
            self._last_len = 0
        print(line)

    def finish(self) -> None:
        if self.enabled and self._active:
            self.println("")


def ensure_path() -> None:
    """Put gcc's bin directory first so cc1.exe loads matching MinGW DLLs (Windows)."""
    gcc = shutil.which("gcc")
    if not gcc:
        return
    bin_dir = os.path.dirname(os.path.abspath(gcc))
    os.environ["PATH"] = bin_dir + os.pathsep + os.environ.get("PATH", "")


def extract_expectations(rad_path: str) -> tuple[list[str], str | None, str]:
    expected: list[str] = []
    runtime_error: str | None = None
    backend = "both"
    with open(rad_path, encoding="utf-8") as f:
        for line in f:
            m = EXPECT_LINE.match(line)
            if m:
                expected.append(m.group(1).rstrip("\r"))
            m = EXPECT_RUNTIME_ERROR.match(line)
            if m:
                runtime_error = m.group(1).rstrip("\r")
            m = BACKEND_DIRECTIVE.match(line)
            if m:
                backend = m.group(1)
    return expected, runtime_error, backend


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run conformance tests against C backend")
    parser.add_argument("--verbose", action="store_true", help="Print PASS/SKIP lines")
    parser.add_argument("--filter", default=None, help="Run tests whose filename contains this substring")
    parser.add_argument(
        "--jobs",
        type=int,
        default=max(1, os.cpu_count() or 1),
        help="Parallel jobs (default: cpu_count)",
    )
    parser.add_argument(
        "--keep-artifacts",
        action="store_true",
        help="Keep generated C/exe artifacts in core/c-backend/target/conformance_c",
    )
    parser.add_argument(
        "--no-progress",
        action="store_true",
        help="Disable live progress bar output",
    )
    parser.add_argument(
        "--force-progress",
        action="store_true",
        help="Force progress bar even when stdout is not a TTY",
    )
    parser.add_argument(
        "--compiler",
        choices=["gcc", "tcc", "auto"],
        default="auto",
        help="C compiler to use (default: auto — prefers tcc if installed)",
    )
    return parser.parse_args()


def load_tests(filter_text: str | None, run_dir: str) -> list[TestCase]:
    rad_files = sorted(glob.glob(os.path.join(CONFORMANCE_DIR, "*.rad")))
    if filter_text:
        rad_files = [f for f in rad_files if filter_text in os.path.basename(f)]

    tests: list[TestCase] = []
    for rad_path in rad_files:
        name = os.path.splitext(os.path.basename(rad_path))[0]
        expected, runtime_error, backend = extract_expectations(rad_path)
        gen_c = os.path.join(run_dir, f"gen_{name}.c")
        gen_exe = os.path.join(run_dir, f"gen_{name}{EXT}")
        tests.append(
            TestCase(
                name=name,
                rad_path=rad_path,
                expected=expected,
                runtime_error=runtime_error,
                backend=backend,
                gen_c=gen_c,
                gen_exe=gen_exe,
            )
        )
    return tests


EMIT_BATCH_SIZE = 30


def _run_rad_emit(script_body: str, timeout: float = 180) -> tuple[int, str, str]:
    """Run a Rad emit script and return (returncode, stdout, stderr)."""
    emit_rel = "core/c-backend/src/_emit_all.rad"
    emit_abs = os.path.join(REPO, "core", "c-backend", "src", "_emit_all.rad")
    try:
        with open(emit_abs, "w", encoding="utf-8") as f:
            f.write(script_body)

        import threading

        proc = subprocess.Popen(
            [RAD, emit_rel],
            cwd=REPO,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        _child_procs.append(proc)

        result = {"stdout": b"", "stderr": b"", "done": False}

        def communicate_thread():
            try:
                result["stdout"], result["stderr"] = proc.communicate()
            except Exception:
                pass
            result["done"] = True

        bg = threading.Thread(target=communicate_thread, daemon=True)
        bg.start()

        emit_start = time.perf_counter()
        while not result["done"]:
            now = time.perf_counter()
            if now - emit_start > timeout:
                proc.kill()
                bg.join(timeout=5)
                return -1, "", "timeout"
            time.sleep(0.2)
        bg.join(timeout=5)

        stdout = result["stdout"].decode("utf-8", errors="replace")
        stderr = result["stderr"].decode("utf-8", errors="replace")
        return proc.returncode, stdout, stderr
    finally:
        if os.path.isfile(emit_abs):
            os.remove(emit_abs)


def _emit_batch(batch: list[TestCase], timeout: float = 180) -> tuple[bool, str]:
    body = 'use "emit_c.rad"\n\nfn main() -> nil {\n'
    for t in batch:
        src_rel = os.path.relpath(t.rad_path, REPO).replace("\\", "/")
        out_rel = os.path.relpath(t.gen_c, REPO).replace("\\", "/")
        body += f'    compile_file_to_c_unchecked("{src_rel}", "{out_rel}")\n'
    body += "}\n"

    rc, stdout, stderr = _run_rad_emit(body, timeout)
    if rc != 0:
        lines = (stderr.strip() or stdout.strip()).splitlines()
        return False, lines[0] if lines else "emit failed"
    return True, ""


def _emit_checked_single(test: TestCase, timeout: float = 60) -> tuple[str, str]:
    """Emit a single test with the checker enabled (soft-fail).
    Returns (status, detail) -- PASS if checker caught the expected error,
    or a status indicating what happened."""
    src_rel = os.path.relpath(test.rad_path, REPO).replace("\\", "/")
    out_rel = os.path.relpath(test.gen_c, REPO).replace("\\", "/")

    body = 'use "emit_c.rad"\n\nfn main() -> nil {\n'
    body += f'    let ok = compile_file_to_c_checked_soft("{src_rel}", "{out_rel}")\n'
    body += '    if not ok { print("__CHECKER_REJECTED__") }\n'
    body += "}\n"

    rc, stdout, stderr = _run_rad_emit(body, timeout)
    combined = stdout + "\n" + stderr

    if "__CHECKER_REJECTED__" in combined and test.runtime_error is not None:
        if test.runtime_error in combined:
            return "PASS", ""
        return "ERROR_MISMATCH", f"checker rejected but missing '{test.runtime_error}'"

    if rc != 0:
        lines = (stderr.strip() or stdout.strip()).splitlines()
        return "EMIT_FAIL", lines[0] if lines else "emit failed"

    return "__EMITTED__", ""


def emit_all_c(
    runnable: list[TestCase],
    progress: ProgressDisplay | None = None,
    record_fn=None,
) -> list[TestCase]:
    """Emit C files in batches. Returns the list of tests that need compile+run.
    Tests from failed batches are recorded via record_fn as EMIT_FAIL.
    Tests with runtime_error are tried with the checker first; if the checker
    catches the error, they are recorded as PASS immediately."""
    if not runnable:
        return []

    normal = [t for t in runnable if t.runtime_error is None]
    checked = [t for t in runnable if t.runtime_error is not None]

    emitted: list[TestCase] = []
    done = 0
    total = len(runnable)

    for i in range(0, len(normal), EMIT_BATCH_SIZE):
        batch = normal[i : i + EMIT_BATCH_SIZE]
        if progress is not None:
            progress.bar("emit", done, total, f"unchecked batch {i // EMIT_BATCH_SIZE + 1}")
        ok, detail = _emit_batch(batch)
        if ok:
            emitted.extend(batch)
        elif record_fn is not None:
            for t in batch:
                record_fn("EMIT_FAIL", t.name, detail)
        done += len(batch)

    for t in checked:
        if progress is not None:
            progress.bar("emit", done, total, f"checked: {t.name}")
        status, detail = _emit_checked_single(t)
        done += 1
        if status == "PASS":
            if record_fn is not None:
                record_fn("PASS", t.name, "")
        elif status == "__EMITTED__":
            emitted.append(t)
        elif record_fn is not None:
            record_fn(status, t.name, detail)

    if progress is not None:
        progress.bar("emit", total, total, "done")
    return emitted


def compile_one_gcc(test: TestCase) -> tuple[str, str]:
    if not os.path.isfile(test.gen_c):
        return "EMIT_FAIL", "no .c generated"
    try:
        p = subprocess.run(
            [GCC, "-O0", test.gen_c, "-I", COMPILER_DIR, "-o", test.gen_exe, "-lm"],
            cwd=REPO,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=60,
        )
        if p.returncode != 0:
            lines = (p.stderr or "").strip().splitlines()
            return "GCC_FAIL", lines[0] if lines else "unknown"
        return "PASS", ""
    except subprocess.TimeoutExpired:
        return "GCC_FAIL", "timeout"


def compile_one_tcc(test: TestCase, tcc_exe: str) -> tuple[str, str]:
    if not os.path.isfile(test.gen_c):
        return "EMIT_FAIL", "no .c generated"
    try:
        cmd = [tcc_exe, test.gen_c, "-I", COMPILER_DIR, "-o", test.gen_exe]
        if os.path.isfile(TCC_COMPAT):
            cmd.insert(2, TCC_COMPAT)
        p = subprocess.run(
            cmd,
            cwd=REPO,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=30,
        )
        if p.returncode != 0:
            lines = (p.stderr or "").strip().splitlines()
            return "GCC_FAIL", lines[0] if lines else "unknown"
        return "PASS", ""
    except subprocess.TimeoutExpired:
        return "GCC_FAIL", "timeout"


def run_and_compare(test: TestCase) -> tuple[str, str]:
    try:
        p = subprocess.run(
            [test.gen_exe],
            cwd=REPO,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except subprocess.TimeoutExpired:
        return "RUN_FAIL", "timeout"

    combined = (p.stdout or "") + "\n" + (p.stderr or "")
    if test.runtime_error is not None:
        if p.returncode != 0 and test.runtime_error in combined:
            return "PASS", ""
        if p.returncode == 0:
            return "ERROR_MISMATCH", "expected runtime error but got exit 0"
        return "ERROR_MISMATCH", f"exit {p.returncode} but missing '{test.runtime_error}'"

    if p.returncode != 0:
        lines = combined.strip().splitlines()
        msg = lines[0] if lines else "unknown"
        return "RUN_FAIL", f"exit {p.returncode}: {msg}"

    actual = [ln.rstrip("\r") for ln in (p.stdout or "").splitlines()]
    while actual and actual[-1] == "":
        actual.pop()

    if actual == test.expected:
        return "PASS", ""

    for i in range(max(len(actual), len(test.expected))):
        a = actual[i] if i < len(actual) else "<missing>"
        e = test.expected[i] if i < len(test.expected) else "<missing>"
        if a != e:
            return "OUTPUT_MISMATCH", f"line {i + 1}: expected '{e}', got '{a}'"

    return "OUTPUT_MISMATCH", f"line count: {len(actual)} vs {len(test.expected)}"


def main() -> int:
    args = parse_args()
    verbose = args.verbose
    jobs = max(1, args.jobs)
    show_progress = (not args.no_progress) and (sys.stdout.isatty() or args.force_progress)
    progress = ProgressDisplay(enabled=show_progress)

    ensure_path()
    if not os.path.isfile(RAD):
        print(f"Missing Rust VM binary: {RAD}")
        return 1

    tcc_exe = None
    if args.compiler == "tcc":
        tcc_exe = find_tcc()
        if not tcc_exe:
            print("Error: --compiler=tcc but tcc not found")
            return 1
    elif args.compiler == "auto":
        tcc_exe = find_tcc()
    use_tcc = tcc_exe is not None
    compiler_name = f"tcc ({tcc_exe})" if use_tcc else "gcc"
    print(f"Compiler: {compiler_name}", flush=True)

    os.makedirs(TARGET_ROOT, exist_ok=True)
    run_dir = os.path.join(TARGET_ROOT, f"r{os.getpid()}_{int(time.time())}")
    os.makedirs(run_dir, exist_ok=True)

    tests = load_tests(args.filter, run_dir)
    counts = {k: 0 for k in STATUSES}
    failures: list[tuple[str, str, str]] = []

    def record(status: str, name: str, detail: str) -> None:
        counts[status] += 1
        if status == "PASS":
            if verbose:
                progress.println(f"  PASS  {name}")
        elif status == "SKIP":
            if verbose:
                progress.println(f"  SKIP  {name}: {detail}")
        else:
            progress.println(f"  {status:16s} {name}: {detail[:120]}")
            failures.append((status, name, detail))

    t0 = time.perf_counter()

    runnable: list[TestCase] = []
    for test in tests:
        if test.backend == "rust":
            record("SKIP", test.name, "backend: rust only")
        elif not test.expected and test.runtime_error is None:
            record("SKIP", test.name, "no expect directives")
        else:
            runnable.append(test)

    emit_t0 = time.perf_counter()
    progress.bar("emit", 0, 1, f"{len(runnable)} files")
    emitted = emit_all_c(
        runnable,
        progress=progress if show_progress else None,
        record_fn=record,
    )
    progress.finish()
    emit_elapsed = time.perf_counter() - emit_t0
    print(f"Emit took {emit_elapsed:.1f}s ({len(emitted)}/{len(runnable)} files)", flush=True)

    compile_t0 = time.perf_counter()
    compiled: list[TestCase] = []
    total_compile = len(emitted)
    compile_done = 0
    def compile_one(test: TestCase) -> tuple[str, str]:
        if use_tcc:
            return compile_one_tcc(test, tcc_exe)
        return compile_one_gcc(test)

    compile_jobs = 1 if use_tcc else jobs
    with ThreadPoolExecutor(max_workers=compile_jobs) as pool:
        futures = {pool.submit(compile_one, test): test for test in emitted}
        for fut in as_completed(futures):
            test = futures[fut]
            status, detail = fut.result()
            compile_done += 1
            if status == "PASS":
                compiled.append(test)
            else:
                record(status, test.name, detail)
            if show_progress:
                progress.bar("compile", compile_done, total_compile,
                             f"ok={len(compiled)} fail={counts['GCC_FAIL'] + counts['EMIT_FAIL']}")
            elif compile_done % 25 == 0:
                print(f"  compile: {compile_done}/{total_compile}...", flush=True)
    progress.finish()
    compile_elapsed = time.perf_counter() - compile_t0
    print(f"Compile took {compile_elapsed:.1f}s ({len(compiled)} ok, "
          f"{counts['GCC_FAIL'] + counts['EMIT_FAIL']} fail)", flush=True)

    run_t0 = time.perf_counter()
    run_done = 0
    total_run = len(compiled)
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = {pool.submit(run_and_compare, test): test for test in compiled}
        for fut in as_completed(futures):
            test = futures[fut]
            status, detail = fut.result()
            record(status, test.name, detail)
            run_done += 1
            if show_progress:
                progress.bar("run", run_done, total_run,
                             f"pass={counts['PASS']} fail={len(failures)}")
            elif run_done % 25 == 0:
                print(f"  run: {run_done}/{total_run}...", flush=True)
    progress.finish()
    run_elapsed = time.perf_counter() - run_t0
    print(f"Run took {run_elapsed:.1f}s ({counts['PASS']} pass, {len(failures)} fail)",
          flush=True)

    elapsed = time.perf_counter() - t0

    print(f"\n=== Conformance C Backend Matrix ({elapsed:.1f}s) ===")
    print(f"  Total:          {len(tests)}")
    for key in STATUSES:
        if counts[key]:
            print(f"  {key:16s} {counts[key]}")

    if failures:
        print("\n--- Failures by category ---")
        for cat in ["EMIT_FAIL", "GCC_FAIL", "RUN_FAIL", "OUTPUT_MISMATCH", "ERROR_MISMATCH"]:
            cat_fails = [f for f in failures if f[0] == cat]
            if not cat_fails:
                continue
            print(f"\n  {cat} ({len(cat_fails)}):")
            for _, name, detail in cat_fails:
                print(f"    {name}: {detail[:120]}")

    if args.keep_artifacts:
        print(f"\nArtifacts kept at: {run_dir}")
    else:
        shutil.rmtree(run_dir, ignore_errors=True)

    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
