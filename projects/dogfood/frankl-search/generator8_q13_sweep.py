"""Reproduce the remaining eight-generator, thirteen-column CNF sweep."""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent
SOLVER = ROOT / "generator_quotient_cnf.py"


def valid_cached(
    path: Path, triples: int, singletons: int, trace_count: int | None
) -> bool:
    if not path.exists():
        return False
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return False
    return (
        document.get("schema") == "rad.boolean-lattice.generator-quotient-cnf.v1"
        and document.get("triple_count") == triples
        and document.get("singleton_count") == singletons
        and document.get("fixed_triple_singleton_traces") == trace_count
        and document.get("minimum_family_size") == 51
        and document.get("maximum_family_size") == 63
        and document.get("result") == "unsat"
    )


def run_slice(
    triples: int,
    singletons: int,
    trace_count: int | None,
    out_dir: Path,
    core_frontier: Path,
    force: bool,
) -> dict:
    trace_suffix = "" if trace_count is None else f"-r{trace_count}"
    output = (
        out_dir
        / f"g8-q13-t{triples}-s{singletons}{trace_suffix}-cut-core26.json"
    )
    if not force and valid_cached(output, triples, singletons, trace_count):
        return {
            "triples": triples,
            "singletons": singletons,
            "trace_count": trace_count,
            "cached": True,
        }
    command = [
        sys.executable,
        str(SOLVER),
        "--generators",
        "8",
        "--columns",
        "13",
        "--max-column-weight",
        "3",
        "--min-family-size",
        "51",
        "--max-family-size",
        "63",
        "--fixed-column",
        "7",
        "--triple-count",
        str(triples),
        "--singleton-count",
        str(singletons),
        "--local-triple-absence-cut",
        "--projected-core-frontier",
        str(core_frontier),
        "--solver",
        "cadical195",
        "--output",
        str(output),
    ]
    if trace_count is not None:
        command[command.index("--local-triple-absence-cut"):command.index("--local-triple-absence-cut")] = [
            "--fixed-triple-singleton-traces",
            str(trace_count),
        ]
    completed = subprocess.run(
        command,
        cwd=ROOT.parents[2],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode == 1:
        raise RuntimeError(
            f"SAT witness found for t={triples}, s={singletons}:\n{completed.stdout}"
        )
    if completed.returncode != 0:
        raise RuntimeError(
            f"slice t={triples},s={singletons} failed ({completed.returncode}):\n"
            f"{completed.stderr}"
        )
    document = json.loads(output.read_text(encoding="utf-8"))
    if document.get("result") != "unsat":
        raise RuntimeError(f"slice t={triples},s={singletons} did not prove UNSAT")
    return {
        "triples": triples,
        "singletons": singletons,
        "trace_count": trace_count,
        "cached": False,
        "seconds": document["solve_seconds"],
        "digest": document["cnf_digest"],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--minimum-triples", type=int, default=6)
    parser.add_argument("--maximum-triples", type=int, default=13)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--force", action="store_true")
    parser.add_argument("--out-dir", type=Path, default=ROOT / "out")
    parser.add_argument(
        "--core-frontier",
        type=Path,
        default=ROOT / "certificates/projected-partition-v5-w3-q26.json",
    )
    args = parser.parse_args()
    if not 6 <= args.minimum_triples <= args.maximum_triples <= 13:
        raise ValueError("triple range must lie in 6..=13")
    if args.jobs < 1:
        raise ValueError("jobs must be positive")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    slices = []
    for triples in range(args.minimum_triples, args.maximum_triples + 1):
        for singletons in range(14 - triples):
            if triples == 11 and singletons == 0:
                slices.extend((triples, singletons, traces) for traces in range(6))
            else:
                slices.append((triples, singletons, None))
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as executor:
        futures = {
            executor.submit(
                run_slice,
                triples,
                singletons,
                trace_count,
                args.out_dir,
                args.core_frontier,
                args.force,
            ): (triples, singletons, trace_count)
            for triples, singletons, trace_count in slices
        }
        for future in concurrent.futures.as_completed(futures):
            result = future.result()
            results.append(result)
            state = "cached" if result["cached"] else f"{result['seconds']:.3f}s"
            trace = (
                "" if result["trace_count"] is None else f" r={result['trace_count']}"
            )
            print(
                f"t={result['triples']} s={result['singletons']}{trace}: "
                f"UNSAT ({state})",
                flush=True,
            )
    results.sort(
        key=lambda result: (
            result["triples"],
            result["singletons"],
            -1 if result["trace_count"] is None else result["trace_count"],
        )
    )
    assert len(results) == len(slices)
    print(f"complete: {len(results)} exact parameter slices")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
