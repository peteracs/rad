"""Generate, independently verify, replay, and hash the envelope-ladder evidence."""

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from verify import read_rows, require, verify_rows

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(args, output, workers):
    start = time.perf_counter_ns()
    run = subprocess.run([str(x) for x in args], cwd=ROOT, capture_output=True, timeout=1200,
                         env={**os.environ, "RAYON_NUM_THREADS": str(workers)})
    output.write_bytes(run.stdout)
    output.with_suffix(".stderr.txt").write_bytes(run.stderr)
    require(run.returncode == 0, (run.stdout + run.stderr).decode("utf-8", errors="replace"))
    return dict(command=[str(x) for x in args], workers=workers, elapsed_ns=time.perf_counter_ns() - start,
                stdout_sha256=digest(output), stderr_sha256=digest(output.with_suffix(".stderr.txt")))


def main():
    out = HERE / "out"
    out.mkdir(exist_ok=True)
    rad = ROOT / "target/release" / ("rad.exe" if os.name == "nt" else "rad")
    commands, reference, summary = [], None, None
    for workers in (1, 4):
        print(f"RAD envelope-ladder certificate, workers={workers}", flush=True)
        output, trace = out / f"run-{workers}.txt", out / f"run-{workers}.radr"
        commands.append(execute([rad, HERE / "main.rad", "--strict-types", "--deny-warnings",
                                 "--experimental-laws", "--record", trace], output, workers))
        rows = read_rows(output)
        if reference is None:
            print("Independent exact-rational and exact-rounding verification", flush=True)
            summary = verify_rows(rows)
        else:
            require(reference == rows, "worker count changed the certificate")
        reference = rows
        require("resolver `Assemble`" in output.read_text(encoding="utf-8"), "causal provenance absent")
        replay = out / f"replay-{workers}.txt"
        commands.append(execute([rad, "replay", trace], replay, 5 - workers))
        require(b"Replay verified: world digest matches" in replay.read_bytes() + replay.with_suffix(".stderr.txt").read_bytes(),
                "replay failed")
    tests = out / "tests.txt"
    commands.append(execute([sys.executable, "-O", "-m", "unittest", "discover", "-s", HERE, "-p", "test_verify.py"], tests, 1))
    sources = sorted(list(HERE.glob("*.rad")) + list(HERE.glob("*.py")) + list(HERE.glob("*.md")))
    report = dict(format="rad-collatz-ladder-v1", completed_utc=datetime.now(timezone.utc).isoformat(),
                  sources={p.relative_to(HERE.parent).as_posix(): digest(p) for p in sources},
                  artifacts={p.name: digest(p) for p in sorted(out.iterdir()) if p.is_file()},
                  binary_sha256=digest(rad), commands=commands, results=summary,
                  scope="Finite exact certificates plus a written all-size proof of a sharp envelope family; "
                        "no Collatz solution or historical-priority claim.")
    (HERE / "evidence.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(summary, indent=2), flush=True)


if __name__ == "__main__":
    main()
