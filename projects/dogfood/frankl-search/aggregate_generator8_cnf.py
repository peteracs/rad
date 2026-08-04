"""Assemble the exact eight-generator quotient exclusions into one manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def load_unsat(path: Path) -> dict:
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("schema") != "rad.boolean-lattice.generator-quotient-cnf.v1":
        raise ValueError(f"{path}: unexpected schema")
    if document.get("result") != "unsat" or document.get("counterexample_exists"):
        raise ValueError(f"{path}: exclusion is not UNSAT")
    if document.get("generator_count") != 8 or document.get("column_count") != 13:
        raise ValueError(f"{path}: wrong quotient dimensions")
    if document.get("maximum_column_weight") != 3 or not document.get("connected"):
        raise ValueError(f"{path}: wrong structural frontier")
    return document


def entry(path: Path, document: dict) -> dict:
    return {
        "source": path.name,
        "triple_count": document["triple_count"],
        "singleton_count": document.get("singleton_count"),
        "minimum_family_size": document["minimum_family_size"],
        "maximum_family_size": document["maximum_family_size"],
        "triple_intersections": document.get("triple_intersections"),
        "fixed_triple_singleton_traces": document.get(
            "fixed_triple_singleton_traces"
        ),
        "projected_minority_encoding": document.get(
            "projected_minority_encoding", False
        ),
        "local_triple_absence_cut": document.get(
            "local_triple_absence_cut", False
        ),
        "variables": document["variables"],
        "clauses": document["clauses"],
        "solve_seconds": document["solve_seconds"],
        "cnf_digest": document["cnf_digest"],
        "solver": document["solver"],
        "result": document["result"],
    }


def find_exact_size(out_dir: Path, family_size: int) -> Path:
    matches = []
    for path in out_dir.glob(f"g8-q13-t5-s0-m{family_size}-*.json"):
        document = json.loads(path.read_text(encoding="utf-8"))
        if (
            document.get("minimum_family_size") == family_size
            and document.get("maximum_family_size") == family_size
            and document.get("result") == "unsat"
        ):
            matches.append(path)
    if not matches:
        raise FileNotFoundError(f"missing exact t=5,s=0,m={family_size} exclusion")
    return sorted(matches, key=lambda path: (path.stat().st_mtime_ns, path.name))[0]


def assemble(out_dir: Path) -> dict:
    paths: list[Path] = [
        out_dir / "generator8-hypergraph-t1-cnf.json",
        out_dir / "generator8-hypergraph-t2-cnf.json",
    ]
    for triples in (3, 4):
        paths.extend(
            out_dir / f"generator8-hypergraph-t{triples}-s{singletons}-cnf.json"
            for singletons in range(9)
        )
    paths.extend(find_exact_size(out_dir, family_size) for family_size in range(51, 64))
    paths.extend(out_dir / f"g8-q13-t5-s{s}-cut.json" for s in range(1, 9))
    for triples in range(6, 14):
        for singletons in range(14 - triples):
            if triples == 11 and singletons == 0:
                paths.extend(
                    out_dir / f"g8-q13-t11-s0-r{traces}-cut-core26.json"
                    for traces in range(6)
                )
            else:
                paths.append(
                    out_dir
                    / f"g8-q13-t{triples}-s{singletons}-cut-core26.json"
                )

    records = [entry(path, load_unsat(path)) for path in paths]
    total_clauses = sum(record["clauses"] for record in records)
    total_seconds = sum(record["solve_seconds"] for record in records)
    digest = hashlib.blake2b(digest_size=32)
    for record in sorted(
        records,
        key=lambda value: (
            value["triple_count"],
            -1 if value["singleton_count"] is None else value["singleton_count"],
            value["minimum_family_size"],
            value["maximum_family_size"],
            -1
            if value["fixed_triple_singleton_traces"] is None
            else value["fixed_triple_singleton_traces"],
            value["cnf_digest"],
        ),
    ):
        digest.update(json.dumps(record, sort_keys=True).encode("utf-8"))
        digest.update(b"\n")
    return {
        "schema": "rad.boolean-lattice.generator8-q13-exclusions.v1",
        "generator_count": 8,
        "column_count": 13,
        "family_size_range": [51, 63],
        "maximum_column_weight": 3,
        "connected": True,
        "excluded_triple_counts": list(range(1, 14)),
        "complete_triple_count_coverage": True,
        "counterexample_exists": False,
        "solver_runs": len(records),
        "total_clauses_across_runs": total_clauses,
        "total_solve_seconds": total_seconds,
        "manifest_digest": digest.hexdigest(),
        "runs": records,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--out-dir", type=Path, default=Path("projects/dogfood/frankl-search/out")
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(
            "projects/dogfood/frankl-search/certificates/"
            "generator8-q13-exclusions.json"
        ),
    )
    args = parser.parse_args()
    document = assemble(args.out_dir)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(document, indent=2, sort_keys=True) + "\n"
    args.output.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
