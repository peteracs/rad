"""Independent literal-set audit of the general power-rank bound."""

from itertools import product


def power_image(mapping, power):
    states = set(range(len(mapping)))
    for _ in range(power):
        states = {mapping[q] for q in states}
    return states


def decrease(mapping, chain, fault=None):
    fault = tuple(range(len(mapping))) if fault is None else fault
    successor = [{mapping[q] for q in states} |
                 ({fault[q] for q in chain[j - 1]} if j else set())
                 for j, states in enumerate(chain)]
    return sum(map(len, chain)) - sum(map(len, successor))


def audit(n, budget):
    chains = [[{q for q in range(n) if thresholds[q] <= j}
               for j in range(budget + 1)]
              for thresholds in product(range(budget + 2), repeat=n)]
    maps = 0
    for mapping in product(range(n), repeat=n):
        maps += 1
        capacity = n - len(power_image(mapping, budget + 1))
        maximum = max(decrease(mapping, chain) for chain in chains)
        if maximum != capacity:
            raise ValueError(f"exact local capacity failed: {n}, {budget}, {mapping}")
        witness = [power_image(mapping, budget - j) for j in range(budget + 1)]
        if decrease(mapping, witness) != capacity:
            raise ValueError("image-power sharpness witness failed")
    return {"n": n, "budget": budget, "maps": maps,
            "chains_per_map": len(chains), "checks": maps * len(chains)}


def verify_rows(rows):
    reference = [audit(n, k) for n in range(2, 5) for k in range(4)]
    if rows != reference or any(type(value) is not int for row in rows for value in row.values()):
        raise ValueError("general capacity evidence is incomplete or incorrect")
    return reference
