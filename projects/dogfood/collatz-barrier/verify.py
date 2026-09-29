"""Independent Python integer/Fraction verification; acceptance survives python -O.

RAD generates certificates with limb arithmetic. This verifier uses Python's
integer arithmetic, literal trajectories, and a separate max-plus parity DP.
The proofs connecting the finite certificates to all integers are in RESULTS.md.
"""

from fractions import Fraction
import json
from pathlib import Path

SCALE = 1 << 192


def require(condition, message):
    if not condition:
        raise ValueError(message)


def logarithm(r):
    terms = [Fraction(2, (2 * j + 1) * r ** (2 * j + 1)) for j in range(192)]
    partial = sum(terms, Fraction(0))
    tail = Fraction(2, 385 * r ** 385) / (1 - Fraction(1, r * r))
    lower = 2 * sum(SCALE // ((2 * j + 1) * r ** (2 * j + 1)) for j in range(192))
    upper = lower + 385
    require(Fraction(lower, SCALE) <= partial, "log lower bound failed")
    require(partial + tail < Fraction(upper, SCALE), "log tail bound failed")
    return lower, upper


def verify_barrier(row):
    a, b = row["lower_numerator"], row["lower_denominator"]
    c, d = row["upper_numerator"], row["upper_denominator"]
    floor = int(row["floor_value"])
    l2, u2 = logarithm(3)
    l3, u3 = logarithm(2)
    require(min(a, b, c, d, floor) > 0, "nonpositive Farey data")
    require(c * b - a * d == 1, "Farey determinant failed")
    require(a * u2 < b * l3, "lower rational is on the wrong side")
    gap = c * l2 - d * u3
    require(gap > 0, "upper rational is on the wrong side")
    margin = 3 * floor * gap - d * SCALE
    require(margin > 0, "additive bound does not imply descent")
    require(str(gap) == row["gap_lower"], "log gap certificate changed")
    require(str(margin) == row["strict_margin"], "strict margin certificate changed")
    require(row["last_depth"] == a + c - 1, "horizon is off by one")
    return {"label": row["label"], "floor": str(floor), "last_depth": a + c - 1}


def expected_envelopes(limit):
    a, offset, best = 1, 0, 1
    rows = []
    for q in range(1, limit + 1):
        offset = 3 * offset + (1 << (a.bit_length() - 1))
        a *= 3
        depth = a.bit_length()
        if depth > limit:
            break
        gap = (1 << depth) - a
        cutoff = offset // gap + 1
        if cutoff > best:
            best = cutoff
            # Independently reconstruct the offset as a sum over odd positions.
            literal_offset = sum((1 << ((3 ** j).bit_length() - 1)) * 3 ** (q - 1 - j)
                                 for j in range(q))
            require(literal_offset == offset, "offset expansion disagrees")
            rows.append(dict(depth=depth, odd_steps=q, cutoff=cutoff,
                             max_coefficient=str(a), max_offset=str(offset), gap=str(gap)))
    return rows


def parity_dp(limit):
    """Max-plus dynamic programming over (depth, odd count), not the greedy rule."""
    powers = [3 ** q for q in range(limit + 1)]
    frontier = {0: 0}
    maxima = {}
    counts = {0: 1}
    stopped_words = 0
    for depth in range(1, limit + 1):
        after, after_counts = {}, {}
        for odd, offset in frontier.items():
            for bit in (0, 1):
                q = odd + bit
                c = 3 * offset + (1 << (depth - 1)) if bit else offset
                if powers[q] < 1 << depth:
                    maxima[q] = max(maxima.get(q, -1), c)
                    stopped_words += counts[odd]
                else:
                    after[q] = max(after.get(q, -1), c)
                    after_counts[q] = after_counts.get(q, 0) + counts[odd]
        frontier, counts = after, after_counts
    for q, actual in maxima.items():
        expected = sum((1 << ((3 ** j).bit_length() - 1)) * 3 ** (q - 1 - j)
                       for j in range(q))
        require(actual == expected, f"parity DP disproves extremizer at q={q}")
    return {"depth": limit, "odd_counts": len(maxima) - 1, "first_contraction_words": str(stopped_words)}


def literal_range(start, end, limit):
    powers = [3 ** q for q in range(limit + 1)]
    row = dict(start=start, end=end, tested=end - start, steps=0,
               max_tau=0, witness=0, peak=0, censored=0)
    for n in range(start, end):
        value, odd = n, 0
        for depth in range(1, limit + 1):
            if value & 1:
                value = (3 * value + 1) // 2
                odd += 1
            else:
                value //= 2
            row["peak"] = max(row["peak"], value)
            row["steps"] += 1
            if powers[odd] < 1 << depth:
                require(value < n, f"CST failure at n={n}, depth={depth}")
                if depth > row["max_tau"]:
                    row["max_tau"], row["witness"] = depth, n
                break
            require(value >= n, "literal descent before coefficient contraction")
        else:
            row["censored"] += 1
    return row


def verify_rotation(rows):
    require(len(rows) == 12, "missing rotation block lengths")
    summary = []
    for m, row in enumerate(rows, 1):
        values = []
        for phase in range(m):
            total = Fraction(0)
            for i in range(m):
                k = i - phase
                if k >= 0:
                    total += Fraction(1 << ((3 ** k).bit_length() - 1), 3 ** k)
                else:
                    total += Fraction(3 ** (-k), 1 << (3 ** (-k)).bit_length())
            values.append(total)
        maximum = max(values)
        expected = dict(block_length=m, phases=m, maximizing_phase=values.index(maximum),
                        maximum_scaled=int(maximum * row["denominator"]), denominator=(1 << 18) * 3 ** 11)
        require(Fraction(expected["maximum_scaled"], expected["denominator"]) == maximum,
                "rotation scaling was not exact")
        require(row == expected, "rotation certificate differs")
        require(maximum < 9 if m == 12 else maximum <= Fraction(3 * m, 4) + Fraction(11, 36),
                "uniform quarter-slope envelope falsified")
        if m == 3:
            require(values[0] - Fraction(9, 4) == Fraction(11, 36), "sharp intercept not attained at phase zero")
        summary.append(dict(length=m, maximum=str(maximum), phase=values.index(maximum)))
    return summary


def verify_rows(rows, limit):
    require(rows[0] == dict(kind="exhaustive", max_depth=20, odd_counts=12), "RAD exhaustive audit missing")
    require(rows[-1] == dict(kind="complete", envelope_limit=limit), "completion receipt missing")
    barriers = [row for row in rows if "last_depth" in row]
    require(len(barriers) == 2, "wrong barrier count")
    require([(r["label"], r["floor_value"], r["last_depth"]) for r in barriers] == [
        ("least-Collatz-counterexample", str(1 << 71), 114208327603),
        ("CST-with-published-starting-value-bound", "28000000000000000000", 10439860590)],
        "barrier identities changed")
    checked = [verify_barrier(row) for row in barriers]
    rotation = verify_rotation([row for row in rows if "block_length" in row])
    envelopes = [row for row in rows if "cutoff" in row]
    require(envelopes == expected_envelopes(limit), "envelope record sequence differs")
    cutoff = envelopes[-1]["cutoff"]
    literal = [row for row in rows if "tested" in row]
    require(len(literal) == 16, "missing literal lanes")
    for lane, row in enumerate(literal):
        start = 2 + (cutoff - 2) * lane // 16
        end = 2 + (cutoff - 2) * (lane + 1) // 16
        require(row == literal_range(start, end, limit), f"literal lane {lane} differs")
    require(len(rows) == 4 + len(envelopes) + 16 + 12, "unexpected or duplicated output rows")
    return dict(barriers=checked, envelope_limit=limit, envelope_cutoff=cutoff,
                envelope_records=[{k: r[k] for k in ("depth", "odd_steps", "cutoff")} for r in envelopes],
                literal_starts=sum(r["tested"] for r in literal),
                literal_steps=sum(r["steps"] for r in literal),
                literal_max_tau=max(r["max_tau"] for r in literal),
                literal_peak=max(r["peak"] for r in literal),
                literal_censored=sum(r["censored"] for r in literal),
                rotation_phases=78, rotation_blocks=rotation,
                independent_parity_dp=parity_dp(256))


def read_rows(path):
    return [json.loads(line) for line in Path(path).read_text(encoding="utf-8-sig").splitlines()
            if line.startswith("{")]
