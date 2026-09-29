"""Independent exact certificates for finite Collatz correction bounds."""

from fractions import Fraction
from functools import lru_cache
import importlib.util
import math
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("escape_arithmetic", HERE.parent / "escape/verify.py")
arithmetic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(arithmetic)
require = arithmetic.require


def iterate(n, k):
    odds = 0
    for _ in range(k):
        if n & 1:
            n = (3 * n + 1) // 2
            odds += 1
        else:
            n //= 2
    return odds, n


def profile(depth):
    groups = [set() for _ in range(depth + 1)]
    odd_groups = [set() for _ in range(depth + 1)]
    size = 1 << depth
    checks, survivors = 0, 0
    first, merges = {}, []
    for start in range(size):
        q, end = iterate(start, depth)
        require(0 <= end < 3 ** q, "representative image bound failed")
        require(q == 0 or end % 3 != 0, "image divisibility bound failed")
        groups[q].add(end)
        smaller = first.setdefault((q, end), start)
        x, power, survives = start, 1, True
        for j in range(1, depth + 1):
            if x & 1:
                x = (3 * x + 1) // 2
                power *= 3
            else:
                x //= 2
            survives = survives and power >= 1 << j
        if survives:
            survivors += 1
            if smaller < start:
                witness = dict(start=start, smaller=smaller, odds=q, endpoint=end)
                verify_merge(depth, witness)
                merges.append(witness)
        if start & 1:
            odd_groups[q].add(end)
        if start < 16 or start >= size - 16:
            require(iterate(start + 3 * size, depth) == (q, end + 3 ** (q + 1)), "translation failed")
            checks += 1
    all_counts, odd_counts = list(map(len, groups)), list(map(len, odd_groups))
    for q in range(1, depth + 1):
        require(odd_counts[q] <= min(math.comb(depth - 1, q - 1), 2 * 3 ** (q - 1)), "odd capacity failed")
    merges.sort(key=lambda w: (w["odds"], w["endpoint"], w["start"]))
    return dict(depth=depth, inputs=size, classes=sum(all_counts), odd_classes=sum(odd_counts),
                by_odds=all_counts, odd_by_odds=odd_counts, translation_checks=checks,
                coefficient_survivors=survivors, merged_survivors=merges)


def verify_merge(depth, witness):
    r, s, q, y = (witness[key] for key in ("start", "smaller", "odds", "endpoint"))
    require(0 < s < r < 1 << depth, "merge does not reduce a positive integer")
    require(iterate(r, depth) == iterate(s, depth) == (q, y), "false affine merge")
    for b in (1, 17):
        target = (q, y + b * 3 ** q)
        require(iterate(r + (b << depth), depth) == target, "translated start failed")
        require(iterate(s + (b << depth), depth) == target, "translated smaller input failed")


@lru_cache(None)
def odd_capacity(k):
    return 1 if k == 0 else sum(min(math.comb(k - 1, q - 1), 2 * 3 ** (q - 1)) for q in range(1, k + 1))


def correction_data(limit, h):
    require(0 <= h <= limit <= 512, "invalid bound domain")
    require(arithmetic.exponent_data(156, 100)["passes_one_over_28"], "bad capacity exponent")
    # Direct binomial formula and rational sum, not the RAD Pascal recurrence.
    dyadic = sum((Fraction(odd_capacity(k), 1 << k) for k in range(h, limit + 1)), Fraction(0))
    cumulative = int(dyadic * (1 << limit))
    require(Fraction(cumulative, 1 << limit) == dyadic, "dyadic conversion lost precision")
    total = dyadic + Fraction(2 * (h + 1), 1 << h) + Fraction(4429, 64) * Fraction(42, 43) ** (limit + 1)
    denominator = (1 << limit) * 64 * 43 ** (limit + 1)
    numerator = int(total * denominator)
    require(Fraction(numerator, denominator) == total, "total lost precision")
    l2, _ = arithmetic.log_bounds(1, 3, True)
    unit = 3 * denominator * l2
    rhs = numerator * arithmetic.S
    bits = rhs // unit + 1
    margin = bits * unit - rhs
    geometric = 2 * 42 ** 29 - 43 ** 29
    require(min(margin, geometric) > 0, "nonpositive certificate margin")
    return dict(limit=limit, minimum_bits=h, dyadic_numerator=str(cumulative),
                sum_numerator=str(numerator), sum_denominator=str(denominator),
                correction_bits=bits, logarithm_margin=str(margin), geometric_margin=str(geometric))


def verify_bound(row):
    require(row == correction_data(row["limit"], row["minimum_bits"]), "incorrect correction certificate")


def audit_orbits():
    starts = list(range(1, 2049)) + [(1 << k) - 1 for k in (64, 128, 256, 512, 1024)]
    max_ratio, max_start = Fraction(0), 0
    transitions, censored, shell_checks = 0, 0, 0
    for start in starts:
        states, seen, q, product = [start], {start}, 0, Fraction(1)
        for _ in range(20000):
            x = states[-1]
            y = (3 * x + 1) // 2 if x & 1 else x // 2
            if y in seen:
                break
            seen.add(y)
            states.append(y)
            if x & 1:
                q += 1
                product *= Fraction(3 * x + 1, 3 * x)
        else:
            censored += 1
        depth = len(states) - 1
        transitions += depth
        ratio = Fraction(states[-1] * (1 << depth), start * 3 ** q)
        require(ratio == product, "affine coefficient and exact product disagree")
        require(ratio < 2048, "uniform finite correction bound falsified")
        if ratio > max_ratio:
            max_ratio, max_start = ratio, start
        counts = {}
        for x in states:
            if x & 1:
                k = x.bit_length() - 1
                counts[k] = counts.get(k, 0) + 1
        for k, count in counts.items():
            if k <= 256:
                require(count <= odd_capacity(k) + k, "finite dyadic capacity failed")
                shell_checks += 1
    return dict(starts=len(starts), max_input_bits=max(starts).bit_length(), transitions=transitions,
                censored=censored, shell_checks=shell_checks, maximum_ratio=str(max_ratio), maximum_start=str(max_start))


def verify(rows):
    profiles = [r for r in rows if "classes" in r]
    require([r["depth"] for r in profiles] == list(range(1, 19)), "missing collision profiles")
    for row in profiles:
        require(row == profile(row["depth"]), "literal collision profile differs")
    bounds = [r for r in rows if "correction_bits" in r]
    require([r["minimum_bits"] for r in bounds] == [0, 16, 32, 64, 71, 128, 256], "missing bound jobs")
    for row in bounds:
        verify_bound(row)
    require(len(rows) == 25, "unexpected output")
    require(bounds[0]["correction_bits"] == 11, "uniform correction constant changed")
    return dict(collision_inputs=sum(r["inputs"] for r in profiles), collision_depth=18,
                classes_at_18=profiles[-1]["classes"], odd_classes_at_18=profiles[-1]["odd_classes"],
                coefficient_survivors_at_18=profiles[-1]["coefficient_survivors"],
                merged_survivors_at_18=len(profiles[-1]["merged_survivors"]),
                merge_certificates=sum(len(r["merged_survivors"]) for r in profiles),
                correction_constant=2048, bounds=[dict(minimum_bits=r["minimum_bits"], correction_bits=r["correction_bits"]) for r in bounds],
                literal_orbits=audit_orbits())
