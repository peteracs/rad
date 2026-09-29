"""Run the RAD research program and independently check its saved evidence."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time

from verify import read_rows, verify_rows

PROJECT = Path(__file__).resolve().parent
ROOT = PROJECT.parents[2]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run(command, output, workers):
    start = time.perf_counter_ns()
    result = subprocess.run([str(s) for s in command], cwd=ROOT, capture_output=True,
                            timeout=300, env={**os.environ, "RAYON_NUM_THREADS": str(workers)})
    elapsed = time.perf_counter_ns() - start
    output.write_bytes(result.stdout)
    output.with_suffix(".stderr.txt").write_bytes(result.stderr)
    if result.returncode:
        raise RuntimeError((result.stdout + result.stderr).decode("utf-8", errors="replace"))
    return dict(command=[str(s) for s in command], workers=workers, elapsed_ns=elapsed,
                stdout_sha256=sha(output), stderr_sha256=sha(output.with_suffix(".stderr.txt")))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", type=Path, default=ROOT / "target/release" / ("rad.exe" if os.name == "nt" else "rad"))
    parser.add_argument("--depth", type=int, default=4096)
    args = parser.parse_args()
    out = PROJECT / "out"
    out.mkdir(exist_ok=True)
    receipts, reference, summary = [], None, None
    for workers in (1, 4):
        print(f"RAD exact envelopes and literal audit, depth={args.depth}, workers={workers}", flush=True)
        output, trace = out / f"run-{workers}.jsonl", out / f"run-{workers}.radr"
        receipts.append(run([args.rad, PROJECT / "main.rad", "--strict-types", "--deny-warnings",
                             "--record", trace, "--", args.depth], output, workers))
        rows = read_rows(output)
        if reference is not None and rows != reference:
            raise RuntimeError("worker count changed mathematical output")
        if reference is None:
            print("Independent Python integer, rational, parity-DP and trajectory verification", flush=True)
            summary = verify_rows(rows, args.depth)
        reference = rows
        replay = out / f"replay-{workers}.txt"
        receipts.append(run([args.rad, "replay", trace], replay, 5 - workers))
        if b"Replay verified: world digest matches" not in replay.read_bytes() + replay.with_suffix(".stderr.txt").read_bytes():
            raise RuntimeError("replay did not verify its digest")
    evidence = dict(format="rad-collatz-barrier-v1", completed_utc=datetime.now(timezone.utc).isoformat(),
                    python=platform.python_version(), platform=platform.platform(), binary_sha256=sha(args.rad),
                    sources={p.name: sha(p) for p in sorted(PROJECT.iterdir()) if p.suffix in {".rad", ".py", ".md"}},
                    artifacts={p.name: sha(p) for p in sorted(out.iterdir()) if p.suffix in {".radr", ".jsonl", ".txt"}},
                    commands=receipts, results=summary,
                    scope="Exact executable certificates plus written proofs; the two large-horizon corollaries use explicitly cited external computational bounds. No Collatz proof or priority claim.")
    encoded = json.dumps(evidence, indent=2) + "\n"
    (PROJECT / "evidence.json").write_text(encoded, encoding="utf-8", newline="\n")
    print(json.dumps(summary, indent=2), flush=True)


if __name__ == "__main__":
    main()
