"""Fail-closed, independent verification of RAD's complete experiment matrix."""

import argparse
import json
from pathlib import Path

from oracle import direct_images, solve, square_word


def verify_rows(rows, max_n):
    if type(max_n) is not int or not 2 <= max_n <= 10:
        raise ValueError("invalid matrix size")
    expected_keys = {(n, k) for n in range(2, max_n + 1) for k in range(3)}
    seen = set()
    verified = []
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError("evidence must be an object")
        for name in ("n", "budget", "length", "target", "discovered", "expanded"):
            if type(row.get(name)) is not int:
                raise ValueError(f"{name} must be an integer")
        key = (row["n"], row["budget"])
        if key not in expected_keys or key in seen:
            raise ValueError(f"unexpected or duplicate instance: {key}")
        seen.add(key)
        reference = solve(*key)
        for field, value in reference.items():
            if row.get(field) != value:
                raise ValueError(f"{key}: {field} differs: {row.get(field)!r} != {value!r}")
        if row["length"] >= 0:
            outcomes = direct_images(key[0], row["word"], key[1])
            if outcomes != {row["target"]}:
                raise ValueError(f"{key}: explicit deletion trajectories do not agree")
        if key[1] == 1:
            word = square_word(key[0])
            if direct_images(key[0], word, 1) != {0}:
                raise ValueError(f"{key}: uniform construction failed")
        verified.append(reference)
    if seen != expected_keys:
        raise ValueError(f"missing matrix instances: {sorted(expected_keys - seen)}")
    return verified


def read_rows(path):
    return [json.loads(line) for line in Path(path).read_text(encoding="utf-8-sig").splitlines()
            if line.startswith("{")]


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("evidence", type=Path)
    parser.add_argument("--max-n", type=int, required=True)
    args = parser.parse_args()
    checked = verify_rows(read_rows(args.evidence), args.max_n)
    print(json.dumps({"verified_instances": len(checked), "max_n": args.max_n,
                      "independent_exact_bfs": True, "explicit_deletion_audit": True}))
