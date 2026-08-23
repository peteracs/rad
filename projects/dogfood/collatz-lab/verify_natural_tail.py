#!/usr/bin/env python3
"""Validate a natural-tail certificate with an independent native verifier.

The verifier executable shares no RAD VM or extension code.  This Python
entry point checks the certificate envelope and diagnostics around its exact,
parallel recomputation.  A missing executable is an error: there is no slower
or less independent fallback implementation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from typing import Any


SCHEMA = "rad.affine-natural-tail-certificate.v1"


class VerificationError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


def exact_scales(
    depths: list[int], verified_power: int, max_steps: int
) -> list[dict[str, Any]]:
    require(max(depths) <= 32, "natural-tail depth exceeds the uint64 verifier domain")
    require(max_steps <= 2048, "natural-tail horizon exceeds the verifier domain")
    executable_name = (
        "collatz-natural-tail-verifier.exe"
        if os.name == "nt"
        else "collatz-natural-tail-verifier"
    )
    executable = Path(__file__).parent / "verifier" / "target" / "release" / executable_name
    require(
        executable.is_file(),
        "independent verifier is not built; run `cargo build --manifest-path "
        "projects/dogfood/collatz-lab/verifier/Cargo.toml --release -j 1`",
    )
    command = [
        str(executable),
        ",".join(str(depth) for depth in depths),
        str(verified_power),
        str(max_steps),
    ]
    try:
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=0.75,
        )
    except subprocess.TimeoutExpired as error:
        raise VerificationError("independent verifier exceeded its 750 ms budget") from error
    require(
        completed.returncode == 0,
        "independent verifier failed: " + completed.stderr.strip(),
    )
    try:
        reports = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise VerificationError("independent verifier returned malformed JSON") from error
    require(isinstance(reports, list), "independent verifier returned a non-list report")
    require(
        [report.get("depth") for report in reports] == depths,
        "independent verifier returned the wrong depth sequence",
    )
    return reports


def critical_path_profile(depth: int) -> tuple[int, int]:
    unrestricted = [1] + [0] * depth
    meanders = [1] + [0] * depth
    for step in range(1, depth + 1):
        next_unrestricted = [0] * (depth + 1)
        next_meanders = [0] * (depth + 1)
        for odd_steps in range(step + 1):
            next_unrestricted[odd_steps] = unrestricted[odd_steps]
            count = meanders[odd_steps]
            if odd_steps:
                next_unrestricted[odd_steps] += unrestricted[odd_steps - 1]
                count += meanders[odd_steps - 1]
            if 3**odd_steps >= 1 << step:
                next_meanders[odd_steps] = count
        unrestricted, meanders = next_unrestricted, next_meanders
    terminal = sum(
        count
        for odd_steps, count in enumerate(unrestricted)
        if 3**odd_steps >= 1 << depth
    )
    return terminal, sum(meanders)


def first_descent(start: int) -> list[int]:
    value = peak = start
    steps = odd_steps = 0
    while value >= start:
        if value & 1:
            value = (3 * value + 1) // 2
            odd_steps += 1
        else:
            value //= 2
        peak = max(peak, value)
        steps += 1
    return [steps, value, peak, odd_steps]


def greedy_critical_shadow(depth: int) -> dict[str, Any]:
    residue = probe = odd_steps = 0
    coefficient = denominator = 1
    input_bits: list[int] = []
    parity_word: list[int] = []
    last_nonzero_input_bit = -1
    for step in range(1, depth + 1):
        parity = 0 if coefficient >= 1 << step else 1
        extension = (parity - probe) & 1
        residue += extension * denominator
        if extension:
            last_nonzero_input_bit = step - 1
        input_bits.append(extension)
        parity_word.append(parity)
        source = probe + extension * coefficient
        if parity:
            probe = (3 * source + 1) // 2
            coefficient *= 3
            odd_steps += 1
        else:
            probe = source // 2
        denominator *= 2
        require(coefficient >= denominator, "greedy parity word crossed its boundary")
    next_parity = 0 if coefficient >= 1 << (depth + 1) else 1
    next_forced_input_bit = (next_parity - probe) & 1
    return {
        "depth": depth,
        "residue": residue,
        "terminal": probe,
        "odd_steps": odd_steps,
        "input_bits": input_bits,
        "parity_word": parity_word,
        "last_nonzero_input_bit": last_nonzero_input_bit,
        "next_parity": next_parity,
        "next_forced_input_bit": next_forced_input_bit,
    }


def verify(document: dict[str, Any]) -> dict[str, Any]:
    canonical_input = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    certificate_sha256 = hashlib.sha256(canonical_input).hexdigest()
    require(document.get("schema") == SCHEMA, "unsupported certificate schema")
    require(document.get("zero_high_input_bits") is True, "certificate is not a natural-tail study")
    require(document.get("proposal_order_independent") is True, "producer order changed the settlement")
    require(
        document.get("speculative_worlds_left_live_state_unchanged") is True,
        "speculation changed live state",
    )
    floor = document.get("verified_convergence_floor", "")
    require(isinstance(floor, str) and floor.startswith("2^"), "invalid verified floor")
    verified_power = int(floor[2:])
    max_steps = document["max_steps"]
    reported_scales = document["scales"]
    depths = [scale["depth"] for scale in reported_scales]
    require(depths == sorted(set(depths)), "scales are not canonical and unique")
    require(all(depth < max_steps for depth in depths), "invalid natural-tail horizon")

    computed_scales = exact_scales(depths, verified_power, max_steps)
    checked_fields = [
        "depth",
        "survivor_classes",
        "coefficient_stops",
        "descents",
        "unresolved",
        "max_coefficient_stop_step",
        "max_coefficient_stop_residue",
        "max_descent_step",
        "max_descent_residue",
        "max_additive_delay",
        "max_peak",
        "max_peak_residue",
        "coefficient_stop_histogram",
        "descent_histogram",
    ]
    for reported, computed in zip(reported_scales, computed_scales, strict=True):
        for field in checked_fields:
            require(reported[field] == computed[field], f"forged depth {reported['depth']} {field}")
        require(
            len(reported["lane_signatures"]) == document["lane_count"],
            f"depth {reported['depth']} has the wrong lane-signature count",
        )
        terminal_words, meander_words = critical_path_profile(reported["depth"])
        require(
            reported["terminal_noncontracting_words"] == terminal_words,
            f"forged depth {reported['depth']} terminal word count",
        )
        require(
            reported["prefix_noncontracting_words"] == meander_words,
            f"forged depth {reported['depth']} prefix word count",
        )
        require(
            computed["survivor_classes"] == meander_words,
            f"depth {reported['depth']} residue and parity models disagree",
        )

    deepest = computed_scales[-1]
    record_trace = first_descent(deepest["max_descent_residue"])
    require(record_trace[0] == deepest["max_descent_step"], "record trace step mismatch")
    require(document["deepest_record_terminal"] == record_trace[1], "forged record terminal")
    require(document["deepest_record_odd_steps"] == record_trace[3], "forged record odd count")
    shadow_depth = document["greedy_critical_shadow"]["depth"]
    shadow = greedy_critical_shadow(shadow_depth)
    require(document["greedy_critical_shadow"] == shadow, "forged greedy critical shadow")
    require(
        document["greedy_shadow_first_descent"] == first_descent(shadow["residue"]),
        "forged greedy-shadow descent",
    )

    canonical_after = json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
    require(canonical_after == canonical_input, "independent verifier mutated its input certificate")
    return {
        "schema": "rad.affine-natural-tail-independent-verification.v1",
        "certificate_sha256": certificate_sha256,
        "verified_depths": depths,
        "deepest_survivors": computed_scales[-1]["survivor_classes"],
        "deepest_max_descent_step": computed_scales[-1]["max_descent_step"],
        "deepest_record_residue": computed_scales[-1]["max_descent_residue"],
        "status": "verified",
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("certificate", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    document = json.loads(args.certificate.read_text(encoding="utf-8"))
    report = verify(document)
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(encoded, encoding="utf-8")
    print(encoded, end="")


if __name__ == "__main__":
    main()
