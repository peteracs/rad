"""Verify coverage and integrity of the eight-generator CNF exclusion manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def canonical_digest(records: list[dict]) -> str:
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
    return digest.hexdigest()


def verify(path: Path) -> dict:
    document = json.loads(path.read_text(encoding="utf-8"))
    assert document["schema"] == "rad.boolean-lattice.generator8-q13-exclusions.v1"
    assert document["generator_count"] == 8
    assert document["column_count"] == 13
    assert document["family_size_range"] == [51, 63]
    assert document["maximum_column_weight"] == 3
    assert document["connected"] is True
    assert document["excluded_triple_counts"] == list(range(1, 14))
    assert document["complete_triple_count_coverage"] is True
    assert document["counterexample_exists"] is False

    records = document["runs"]
    assert len(records) == document["solver_runs"] == 82
    assert canonical_digest(records) == document["manifest_digest"]
    assert sum(record["clauses"] for record in records) == document[
        "total_clauses_across_runs"
    ]
    assert abs(
        sum(record["solve_seconds"] for record in records)
        - document["total_solve_seconds"]
    ) < 1e-6
    for record in records:
        assert record["result"] == "unsat"
        assert record["solver"] == "cadical195"
        assert len(record["cnf_digest"]) == 64
        assert record["variables"] > 0
        assert record["clauses"] > 0
        assert record["solve_seconds"] >= 0

    by_triples: dict[int, list[dict]] = {}
    for record in records:
        by_triples.setdefault(record["triple_count"], []).append(record)
    for triples in (1, 2):
        runs = by_triples[triples]
        assert len(runs) == 1
        assert runs[0]["singleton_count"] is None
        assert (runs[0]["minimum_family_size"], runs[0]["maximum_family_size"]) == (
            51,
            63,
        )
    for triples in (3, 4):
        runs = by_triples[triples]
        assert {run["singleton_count"] for run in runs} == set(range(9))
        assert all(
            (run["minimum_family_size"], run["maximum_family_size"]) == (51, 63)
            for run in runs
        )

    five = by_triples[5]
    exact_zero = [run for run in five if run["singleton_count"] == 0]
    assert {
        (run["minimum_family_size"], run["maximum_family_size"])
        for run in exact_zero
    } == {(family_size, family_size) for family_size in range(51, 64)}
    ranged = [run for run in five if run["singleton_count"] != 0]
    assert {run["singleton_count"] for run in ranged} == set(range(1, 9))
    assert all(
        (run["minimum_family_size"], run["maximum_family_size"]) == (51, 63)
        for run in ranged
    )
    for triples in range(6, 14):
        runs = by_triples[triples]
        for singletons in range(14 - triples):
            matching = [
                run for run in runs if run["singleton_count"] == singletons
            ]
            if triples == 11 and singletons == 0:
                assert {
                    run["fixed_triple_singleton_traces"] for run in matching
                } == set(range(6))
            else:
                assert len(matching) == 1
                assert matching[0]["fixed_triple_singleton_traces"] is None
            assert all(
                (run["minimum_family_size"], run["maximum_family_size"])
                == (51, 63)
                for run in matching
            )
    return document


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "certificate",
        nargs="?",
        type=Path,
        default=Path(
            "projects/dogfood/frankl-search/certificates/"
            "generator8-q13-exclusions.json"
        ),
    )
    args = parser.parse_args()
    document = verify(args.certificate)
    print(
        "verified eight-generator q=13 exclusions: "
        f"{document['solver_runs']} runs, triple counts 1..13, "
        f"digest {document['manifest_digest']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
