"""Independently audit projected Boolean-partition frontier certificates."""

from __future__ import annotations

import argparse
import itertools
import json
from collections import Counter
from pathlib import Path


def permute(pattern: int, permutation: tuple[int, ...]) -> int:
    return sum(
        1 << target
        for source, target in enumerate(permutation)
        if pattern >> source & 1
    )


def quotient_size(tests: tuple[int, ...], variables: int) -> int:
    return len(
        {
            tuple(bool(subset & test) for test in tests)
            for subset in range(1 << variables)
        }
    )


def verify(path: Path) -> dict:
    document = json.loads(path.read_text(encoding="utf-8"))
    assert document["schema"] == "rad.boolean-lattice.projected-partition-frontier.v1"
    variables = document["variables"]
    maximum_weight = document["maximum_weight"]
    maximum_tests = document["maximum_tests"]
    threshold = document["quotient_threshold"]
    patterns = tuple(
        pattern
        for pattern in range(1, 1 << variables)
        if pattern.bit_count() <= maximum_weight
    )
    pattern_index = {pattern: index for index, pattern in enumerate(patterns)}
    selection_mask = lambda tests: sum(1 << pattern_index[test] for test in tests)
    assert document["test_patterns"] == len(patterns)
    permutations = tuple(itertools.permutations(range(variables)))
    assert document["permutations"] == len(permutations)

    labelled: set[tuple[int, ...]] = set()
    orbit_test_counts = Counter()
    previous = None
    for orbit in document["orbit_cores"]:
        tests = tuple(orbit["tests"])
        assert tests == tuple(sorted(set(tests)))
        assert 1 <= len(tests) <= maximum_tests
        assert all(test in patterns for test in tests)
        assert quotient_size(tests, variables) == orbit["quotient_size"] >= threshold
        assert all(
            quotient_size(tests[:index] + tests[index + 1 :], variables) < threshold
            for index in range(len(tests))
        )
        images = {
            tuple(sorted(permute(test, permutation) for test in tests))
            for permutation in permutations
        }
        canonical = min(images, key=selection_mask)
        assert tests == canonical
        canonical_mask = selection_mask(canonical)
        assert previous is None or previous < canonical_mask
        previous = canonical_mask
        assert labelled.isdisjoint(images)
        labelled.update(images)
        orbit_test_counts[len(tests)] += 1

    assert len(document["orbit_cores"]) == document["symmetry_orbits"]
    assert len(labelled) == document["labelled_minimal_cores"]
    labelled_by_size = Counter(len(core) for core in labelled)
    assert {str(size): count for size, count in sorted(labelled_by_size.items())} == {
        str(size): count
        for size, count in sorted(
            (int(size), count)
            for size, count in document["labelled_by_test_count"].items()
        )
    }
    return {
        "threshold": threshold,
        "labelled": len(labelled),
        "orbits": len(document["orbit_cores"]),
        "orbit_test_counts": dict(sorted(orbit_test_counts.items())),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "certificates",
        nargs="*",
        type=Path,
        default=[
            Path(
                f"projects/dogfood/frankl-search/certificates/"
                f"projected-partition-v5-w3-q{threshold}.json"
            )
            for threshold in range(26, 33)
        ],
    )
    args = parser.parse_args()
    for certificate in args.certificates:
        result = verify(certificate)
        print(
            f"q>={result['threshold']}: {result['labelled']} labelled minimal cores, "
            f"{result['orbits']} symmetry orbits"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
