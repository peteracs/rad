"""Independent set-based Dijkstra search, without interval-state pruning."""

import heapq
from itertools import count


def weighted_search(n, a_cost, b_cost):
    if not 3 <= n <= 30 or min(a_cost, b_cost) < 0 or a_cost + b_cost == 0:
        raise ValueError("unsupported weighted search")
    full = frozenset(range(n))
    start = (frozenset({0}),) * 2
    distances = {start: 0}
    serial = count()
    queue = [(0, next(serial), start)]
    while queue:
        distance, _, (outer, inner) = heapq.heappop(queue)
        if distance != distances[outer, inner]:
            continue
        for letter, cost in enumerate((a_cost, b_cost)):
            def step(q):
                return (q + 1) % n if letter == 0 else (0 if q == n - 1 else q)
            successor = (frozenset(q for q in full if step(q) in outer),
                         frozenset(q for q in outer if step(q) in inner))
            if successor[1] and distance + cost < distances.get(successor, float("inf")):
                distances[successor] = distance + cost
                heapq.heappush(queue, (distance + cost, next(serial), successor))
    return {"n": n, "a_cost": a_cost, "b_cost": b_cost,
            "cost": distances.get((full, full), -1), "states": len(distances)}


def verify_rows(rows, max_n):
    if type(max_n) is not int or not 3 <= max_n <= 30:
        raise ValueError("unsupported weighted matrix size")
    expected = {(n, a, b) for n in range(3, max_n + 1)
                for a, b in ((1, 1), (1, 0), (0, 1))}
    seen = set()
    for row in rows:
        if not isinstance(row, dict) or any(type(row.get(field)) is not int
                                           for field in ("n", "a_cost", "b_cost", "cost", "states")):
            raise ValueError("malformed weighted evidence")
        key = (row["n"], row["a_cost"], row["b_cost"])
        if key not in expected or key in seen:
            raise ValueError("duplicate or unexpected weighted instance")
        seen.add(key)
        reference = weighted_search(*key)
        if row != reference:
            raise ValueError(f"weighted search mismatch: {row} != {reference}")
        n, a, b = key
        formula = a * ((n - 1) ** 2 + 1) + b * (2 * n - 2)
        if row["cost"] != formula:
            raise ValueError(f"all-size cost theorem falsified at {key}")
    if seen != expected:
        raise ValueError("incomplete weighted evidence")
    return rows
