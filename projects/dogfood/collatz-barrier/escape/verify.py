"""Independent Fraction arithmetic and binomial/strip counting for escape proofs."""

from fractions import Fraction
from functools import lru_cache
import math

S = 1 << 192


def require(condition, message):
    if not condition:
        raise ValueError(message)


@lru_cache(None)
def log_bounds(a, b, old=False):
    require(b > 0 and (0 <= 3 * a <= b or (old and (a, b) == (1, 2))), "bad logarithm domain")
    ratio = Fraction(a, b)
    power = S * a // b
    total = 0
    for j in range(192):
        total += power // (2 * j + 1)
        power = power * a * a // (b * b)
    lower = 2 * total
    upper = lower + (385 if old else 1153)
    # Independent exact rational series, not the rounded recurrence.
    partial = 2 * sum((ratio ** (2 * j + 1) / (2 * j + 1) for j in range(192)), Fraction(0))
    tail = 2 * ratio ** 385 / (385 * (1 - ratio * ratio))
    require(Fraction(lower, S) <= partial, "invalid logarithm lower bound")
    require(partial + tail < Fraction(upper, S), "invalid logarithm upper bound")
    return lower, upper


def exponent_data(p, q):
    require(q < p <= 2 * q, "weight outside 1..2")
    l2, u2 = log_bounds(1, 3, True)
    l3, u3 = log_bounds(1, 2, True)
    lz, uz = log_bounds(p - q, p + q)
    lc, uc = log_bounds(p - q, p + 3 * q)
    ls, us = l2 + lc, u2 + uc
    ulo, uhi, vlo, vhi = l2 * (l3 + lz), u2 * (u3 + uz), l3 * ls, u3 * us
    floor = (100000000 * ulo) // vhi - 100000000
    lower_margin = 100000000 * ulo - (100000000 + floor) * vhi
    upper_margin = (100000001 + floor) * vlo - 100000000 * uhi
    require(min(lower_margin, upper_margin) > 0, "exponent not enclosed")
    return dict(numerator=p, denominator=q, exponent_floor=floor, exponent_scale=100000000,
                lower_margin=str(lower_margin), upper_margin=str(upper_margin),
                passes_one_over_28=28 * ulo > 29 * vhi)


def strip_counts(width, limit):
    # Separate (depth,odd-count) DP uses Python integers, not limb arithmetic.
    frontier = {0: 1}
    counts = []
    for k in range(1, limit + 1):
        after = {}
        for q, ways in frontier.items():
            for bit in (0, 1):
                odd = q + bit
                if 3 ** odd * width >= 1 << k and 3 ** odd <= width * (1 << k):
                    after[odd] = after.get(odd, 0) + ways
        frontier = after
        counts.append(sum(after.values()))
    return counts


def strip_data(width):
    for k, words in enumerate(strip_counts(width, 256), 1):
        modulus = 1 << k
        margin = 3 * modulus - width * words
        if margin > 0:
            linear = (3 * width * words + margin - 1) // margin
            constant = (3 * words * modulus + margin - 1) // margin
            return dict(width=width, block_length=k, words=str(words), modulus=str(modulus),
                        margin=str(margin), linear_coefficient=str(linear), constant=str(constant))
    raise ValueError("no strip certificate")


def verify_strip(row):
    require(row == strip_data(row["width"]), "strip data or rounded deadline differs")
    w, r, d = row["width"], int(row["words"]), int(row["modulus"])
    margin = 3 * d - w * r
    require(margin > 0, "nonpositive strip margin")
    require(int(row["linear_coefficient"]) * margin >= 3 * w * r, "linear deadline rounded down")
    require(int(row["constant"]) * margin >= 3 * r * d, "constant deadline rounded down")


def direct_parity_counts(width, depth):
    count = 0
    for word in range(1 << depth):
        odds = 0
        for k in range(1, depth + 1):
            odds += (word >> (k - 1)) & 1
            if 3 ** odds * width < 1 << k or 3 ** odds > width * (1 << k):
                break
        else:
            count += 1
    return count


def literal_packing_audit():
    """Test the finite state-height and residue-capacity inequalities on real orbits.

    Never infer an infinite trajectory from a finite run. Stop at the first
    repeated state; zero and negative starting values are not included.
    """
    checks, repeats, branches = 0, 0, 0
    for n in range(1, 513):
        states, coefficients, seen = [n], [Fraction(1)], {n}
        for _ in range(128):
            x = states[-1]
            odd = x & 1
            y = (3 * x + 1) // 2 if odd else x // 2
            if y in seen:
                repeats += 1
                break
            seen.add(y)
            states.append(y)
            coefficients.append(coefficients[-1] * (Fraction(3, 2) if odd else Fraction(1, 2)))
        for length in (8, 16, 32, 64, 128):
            if length >= len(states):
                continue
            spread = max(coefficients[:length + 1]) / min(coefficients[:length + 1])
            height = spread * (n + Fraction(length, 3))
            require(max(states[:length + 1]) <= height, "state-height bound failed")
            # Exact capacity bound with a terminal binomial tail, for all useful k.
            for k in range(1, min(length, 20) + 1):
                cutoff = next(q for q in range(k + 1) if 3 ** q * spread >= 1 << k)
                words = sum(math.comb(k, q) for q in range(cutoff, k + 1))
                count = length - k + 1
                require(count <= words * (height // (1 << k) + 1), "residue capacity failed")
                branches += 1
            checks += 1
    return dict(positive_starts=512, prefixes=checks, capacity_checks=branches, repeats_observed=repeats)


def verify(rows):
    weights = [r for r in rows if "exponent_scale" in r]
    require([r["numerator"] for r in weights] == list(range(150, 171)), "missing or duplicate weight jobs")
    for row in weights:
        require(row == exponent_data(row["numerator"], 100), "exponent certificate differs")
    winner = max(weights, key=lambda r: (r["exponent_floor"], -r["numerator"]))
    require(winner["passes_one_over_28"], "no certified improvement")
    selections = [r for r in rows if "proposals" in r]
    require(selections == [dict(numerator=winner["numerator"], exponent_floor=winner["exponent_floor"], proposals=21)],
            "causal selection differs")
    strips = [r for r in rows if "block_length" in r]
    require([r["width"] for r in strips] == [2, 4, 8, 16, 32, 64, 128, 256], "missing strips")
    for row in strips:
        verify_strip(row)
    audits = [r for r in rows if "residues" in r]
    require([r["depth"] for r in audits] == list(range(1, 17)), "missing residue audits")
    counts = {width: strip_counts(width, 16) for width in (2, 4, 8, 16)}
    for row in audits:
        d = row["depth"]
        require(row == dict(depth=d, residues=1 << d, transitions=d * (1 << d),
                            counts=[counts[w][d - 1] for w in (2, 4, 8, 16)]), "literal residue audit differs")
    for width in (2, 4, 8, 16):
        require(direct_parity_counts(width, 16) == counts[width][-1], "direct parity enumeration differs")
    require(len(rows) == 46, "unknown output rows")
    return dict(selected_weight=f"{winner['numerator']}/100", exponent_floor=winner["exponent_floor"],
                exponent_scale=100000000, simple_exponent="1/28", weight_jobs=21,
                strip_certificates=strips, residue_cases=sum(r["residues"] for r in audits),
                literal_transitions=sum(r["transitions"] for r in audits),
                literal_packing=literal_packing_audit())
