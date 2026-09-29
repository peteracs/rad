"""Independently re-derive the sharp envelope ladder from RAD's printed rows.

The verifier does not trust any RAD number. It recomputes:

* block maxima from exact big-integer floors/ceilings (not RAD's recurrence);
* every finite ladder fact with exact rationals C_q / 3^q (no rounding);
* the bootstrap using its own block excess.

RAD's numbers are then required to agree: every RAD upper bound must be at
least the verifier's rigorous lower bound, and RAD's phase-zero enclosures
must contain the exact rationals.
"""

from fractions import Fraction
import json
from pathlib import Path

from reference import (BLOCK, S, SIGMA, SIGMA_BITS, SIGMA_NUM, SIGMA_UNIT, block_maxima,
                       exact_f, extremal_offsets, require)

LANES = 16
PROBES = (12, 41, 53, 306, 665)
TOLERANCE = 1 << 24  # RAD's recurrence enclosure is looser than exact rounding
HEADLINE_SLOPES = (Fraction(1, 3), Fraction(1, 4), Fraction(49, 200), Fraction(97, 400),
                   Fraction(241, 1000), Fraction(481, 2000), SIGMA / 3)


def read_rows(path):
    return [json.loads(line) for line in Path(path).read_text(encoding="utf-8-sig").splitlines()
            if line.startswith("{")]


def split_rows(rows):
    lanes = [r for r in rows if "candidate_phases" in r]
    assembled = [r for r in rows if "covered" in r]
    probes = [r for r in rows if r.get("kind") == "probe"]
    ladders = [r for r in rows if "vertices" in r]
    complete = [r for r in rows if r.get("kind") == "complete"]
    require(len(lanes) == LANES and len(assembled) == 1 and len(probes) == len(PROBES)
            and len(ladders) == 1 and len(complete) == 1, "unexpected row set")
    return lanes, assembled[0], probes, ladders[0], complete[0]


def verify_block(lanes, assembled, lower, upper):
    lanes = sorted(lanes, key=lambda r: r["first"])
    require(lanes[0]["first"] == 1 and lanes[-1]["last"] == BLOCK, "lanes do not span the block")
    for left, right in zip(lanes, lanes[1:]):
        require(left["last"] + 1 == right["first"], "lanes are not contiguous")
    best_upper = 0
    for lane in lanes:
        ms = range(lane["first"], lane["last"] + 1)
        require(lane["block"] == BLOCK, "lane block mismatch")
        require(lane["candidate_phases"] == sum(ms), "candidate phase count mismatch")
        inner = [m for m in ms if m < BLOCK]
        if inner:
            lane_lo = max(lower[m] - SIGMA_UNIT * m for m in inner)
            lane_hi = max(upper[m] - SIGMA_UNIT * m for m in inner)
            require(lane["excess_max"] >= lane_lo, "RAD lane excess is below a rigorous lower bound")
            require(lane["excess_max"] <= lane_hi + TOLERANCE, "RAD lane excess is implausibly loose")
            best_upper = max(best_upper, lane_hi)
        if lane["last"] == BLOCK:
            require(lower[BLOCK] <= lane["block_upper"] <= upper[BLOCK] + TOLERANCE, "RAD block maximum disagrees")
        else:
            require(lane["block_upper"] == 0, "non-final lane reported a block maximum")
    require(assembled["lanes"] == LANES and assembled["covered"] == BLOCK and assembled["contracts"],
            "assembled certificate incomplete")
    require(assembled["excess"] == max(0, max(l["excess_max"] for l in lanes)), "assembled excess is not the lane maximum")
    require(assembled["block_upper"] == lanes[-1]["block_upper"], "assembled block maximum mismatch")
    # The verifier's own certificate, from its own rounding.
    require(upper[BLOCK] <= SIGMA_UNIT * BLOCK, "block does not contract at sigma*")
    return best_upper, SIGMA_UNIT * BLOCK - upper[BLOCK]


def verify_probes(probes):
    facts = []
    for row, length in zip(probes, PROBES):
        lower, upper = block_maxima(length)
        require(row["block"] == length and row["sigma_bound"] == SIGMA_UNIT * length, "probe identity mismatch")
        require(lower[length] <= row["block_upper"], "RAD probe bound below the true maximum")
        # A rigorous failure: even the lower bound exceeds sigma* * length.
        require(not row["contracts"] and lower[length] > SIGMA_UNIT * length, "probe unexpectedly contracts")
        facts.append(dict(block=length, max_over_sigma_bound=float(Fraction(lower[length], SIGMA_UNIT * length))))
    return facts


def verify_ladder(ladder, excess_upper):
    vertices = ladder["vertices"]
    q_tail = ladder["tail_start"]
    require((ladder["sigma_numerator"], ladder["sigma_bits"], ladder["block"]) == (SIGMA_NUM, SIGMA_BITS, BLOCK),
            "ladder parameters mismatch")
    require(vertices[0] == 0 and all(a < b for a, b in zip(vertices, vertices[1:])), "vertices not increasing")
    require(ladder["gaps"] == [b - a for a, b in zip(vertices, vertices[1:])], "gap list mismatch")
    offsets = extremal_offsets(q_tail)
    f = [exact_f(offsets, q) for q in range(q_tail + 1)]
    # RAD's enclosures must contain the exact values.
    for v, lo, hi in zip(vertices, ladder["vertex_lo"], ladder["vertex_hi"]):
        require(Fraction(lo, S) <= f[v] <= Fraction(hi, S), f"RAD enclosure misses F_{v}")
    # Exact: every intermediate point lies strictly below its chord.
    for a, b in zip(vertices, vertices[1:]):
        for q in range(a + 1, b):
            require(f[q] * (b - a) < f[a] * (b - q) + f[b] * (q - a), f"point {q} not below chord {a}-{b}")
    # Exact strict concavity and exposure of the last vertex at sigma*.
    slopes = [(f[b] - f[a]) / (b - a) for a, b in zip(vertices, vertices[1:])]
    require(all(x > y for x, y in zip(slopes, slopes[1:])), "ladder not strictly concave")
    require(slopes[-1] > SIGMA, "last edge not steeper than sigma*")
    top = vertices[-1]
    # Exact tail below the sigma* ray, then the bootstrap with the verifier's own excess.
    for q in range(top + 1, q_tail + 1):
        require(f[q] - f[top] <= SIGMA * (q - top), f"tail point {q} above the sigma* ray")
    bootstrap = f[top] - SIGMA * top - (f[q_tail] - SIGMA * q_tail + Fraction(excess_upper, S))
    require(bootstrap > 0, "bootstrap fails with the verifier's own block excess")
    return f, slopes, bootstrap


def sharp_intercepts(f, vertices):
    rows = []
    for s in HEADLINE_SLOPES:
        values = [(f[v] / 3 - s * v, v) for v in vertices]
        best, v = max(values)
        rows.append(dict(slope=str(s), slope_decimal=f"{float(s):.10f}", attained_at_q=v,
                         intercept=str(best) if v <= 3 else None, intercept_decimal=f"{float(best):.12f}"))
    return rows


def verify_rows(rows):
    lanes, assembled, probes, ladder, complete = split_rows(rows)
    require(complete == dict(kind="complete", block=BLOCK, horizon=20000), "completion row mismatch")
    lower, upper = block_maxima(BLOCK)
    excess_upper, block_margin = verify_block(lanes, assembled, lower, upper)
    probe_facts = verify_probes(probes)
    f, slopes, bootstrap = verify_ladder(ladder, excess_upper)
    vertices = ladder["vertices"]
    return dict(
        theorem="For every s >= s* and q >= 0: C_q/3^q <= s q + max_v (C_v/3^v - s v), with equality at a vertex.",
        s_star=str(SIGMA / 3), s_star_decimal=f"{float(SIGMA / 3):.12f}",
        asymptotic_slope="1/(6 ln 2) = 0.240449173481",
        vertices=vertices, gaps=ladder["gaps"],
        edge_slopes=[f"{float(x / 3):.12f}" for x in slopes],
        block=BLOCK, block_margin=float(Fraction(block_margin, S)),
        block_excess_upper=float(Fraction(excess_upper, S)),
        tail_start=ladder["tail_start"], exact_bootstrap_margin=float(bootstrap),
        convergent_probes=probe_facts,
        sharp_intercepts=sharp_intercepts(f, vertices),
    )


if __name__ == "__main__":
    import sys
    print(json.dumps(verify_rows(read_rows(sys.argv[1])), indent=2))
