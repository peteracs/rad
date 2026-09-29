"""Run the finite correction campaign, independent audit, and cross-worker replay."""

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from verify import require, verify

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(args, output, workers):
    start = time.perf_counter_ns()
    run = subprocess.run([str(s) for s in args], cwd=ROOT, capture_output=True, timeout=180,
                         env={**os.environ, "RAYON_NUM_THREADS": str(workers)})
    output.write_bytes(run.stdout)
    output.with_suffix(".stderr.txt").write_bytes(run.stderr)
    require(run.returncode == 0, (run.stdout + run.stderr).decode("utf-8", errors="replace"))
    return dict(command=[str(s) for s in args], workers=workers, elapsed_ns=time.perf_counter_ns() - start,
                stdout_sha256=digest(output), stderr_sha256=digest(output.with_suffix(".stderr.txt")))


def main():
    out = HERE / "out"
    out.mkdir(exist_ok=True)
    rad = ROOT / "target/release" / ("rad.exe" if os.name == "nt" else "rad")
    commands, reference, summary = [], None, None
    for workers in (1, 4):
        print(f"RAD collision capacities and correction certificates, workers={workers}", flush=True)
        output, trace = out / f"run-{workers}.jsonl", out / f"run-{workers}.radr"
        commands.append(execute([rad, HERE / "main.rad", "--strict-types", "--deny-warnings", "--record", trace], output, workers))
        rows = [json.loads(line) for line in output.read_text(encoding="utf-8").splitlines() if line.startswith("{")]
        if reference is None:
            print("Independent integer, Fraction, collision, and orbit verification", flush=True)
            summary = verify(rows)
        else:
            require(reference == rows, "worker count changed certificates")
        reference = rows
        replay = out / f"replay-{workers}.txt"
        commands.append(execute([rad, "replay", trace], replay, 5 - workers))
        require(b"Replay verified: world digest matches" in replay.read_bytes() + replay.with_suffix(".stderr.txt").read_bytes(), "replay failed")
    commands.append(execute([sys.executable, "-O", "-m", "unittest", "discover", "-s", HERE, "-p", "test_verify.py"], out / "tests.txt", 1))
    sources = list(HERE.glob("*.rad")) + list(HERE.glob("*.py")) + list(HERE.glob("*.md"))
    sources += [HERE.parent / p for p in ("natural.rad", "barrier.rad", "escape/exact.rad", "escape/verify.py")]
    report = dict(format="rad-collatz-correction-v1", completed_utc=datetime.now(timezone.utc).isoformat(),
                  sources={p.relative_to(HERE.parent).as_posix(): digest(p) for p in sorted(sources)},
                  artifacts={p.name: digest(p) for p in sorted(out.iterdir()) if p.is_file()},
                  binary_sha256=digest(rad), commands=commands, results=summary,
                  scope="Exact finite arithmetic supports the written uniform correction theorem. Pairwise-distinct states are required. This does not certify a Collatz proof or historical priority.")
    (HERE / "evidence.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(summary, indent=2), flush=True)


if __name__ == "__main__":
    main()
