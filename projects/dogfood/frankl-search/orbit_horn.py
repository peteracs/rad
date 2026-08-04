"""Domain-neutral Horn closure relations for finite join-orbit systems."""

from __future__ import annotations

from collections.abc import Iterator


def orbit_owner(orbits: list[tuple[int, ...]], universe_size: int) -> list[int]:
    result = [-1] * universe_size
    for index, orbit in enumerate(orbits):
        for value in orbit:
            result[value] = index
    if any(index < 0 for index in result):
        raise ValueError("orbits do not partition the finite universe")
    return result


def iter_join_horn_clauses(
    orbits: list[tuple[int, ...]],
    owner: list[int],
) -> Iterator[tuple[int, int, int]]:
    """Return all non-tautological ``left & right -> target`` clauses.

    The finite operation is bitwise OR.  Simultaneous rotation lets us fix one
    representative from the left orbit and vary only the relative rotation of
    the right orbit, reducing pair enumeration by one group factor.
    """

    for left_index, left_orbit in enumerate(orbits):
        representative = left_orbit[0]
        for right_index in range(left_index, len(orbits)):
            targets = {owner[representative | right] for right in orbits[right_index]}
            for target in sorted(targets):
                if target == left_index or target == right_index:
                    continue
                yield left_index, right_index, target


def all_join_horn_clauses(
    orbits: list[tuple[int, ...]],
    owner: list[int],
) -> list[tuple[int, int, int]]:
    return list(iter_join_horn_clauses(orbits, owner))


def violated_join_horn_clauses(
    selected: set[int],
    orbits: list[tuple[int, ...]],
    owner: list[int],
    limit: int,
) -> list[tuple[int, int, int]]:
    """Separate missing join implications from one concrete orbit selection."""

    found: list[tuple[int, int, int]] = []
    chosen = sorted(selected)
    for left_position, left_index in enumerate(chosen):
        representative = orbits[left_index][0]
        for right_index in chosen[left_position:]:
            targets = {owner[representative | right] for right in orbits[right_index]}
            for target in sorted(targets):
                if target in selected or target == left_index or target == right_index:
                    continue
                found.append((left_index, right_index, target))
                if len(found) >= limit:
                    return found
    return found
