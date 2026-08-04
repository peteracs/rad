"""Exact CNF model for bounded incidence-column quotient counterexamples."""

from __future__ import annotations

import argparse
import hashlib
import itertools
import json
from pathlib import Path
from time import perf_counter

from pysat.card import CardEnc, EncType
from pysat.formula import CNF, IDPool
from pysat.solvers import Solver


def permute_pattern(pattern: int, permutation: tuple[int, ...]) -> int:
    result = 0
    for source, target in enumerate(permutation):
        if pattern >> source & 1:
            result |= 1 << target
    return result


def projected_pattern(pattern: int, outside: tuple[int, ...]) -> int:
    return sum(
        1 << target
        for target, source in enumerate(outside)
        if pattern >> source & 1
    )


def add_projected_core_cut(
    cnf: CNF,
    pool: IDPool,
    selected: dict[int, int],
    generator_count: int,
    fixed_column: int,
    frontier_path: Path,
) -> tuple[int, int, int, str]:
    document = json.loads(frontier_path.read_text(encoding="utf-8"))
    outside = tuple(
        bit for bit in range(generator_count) if fixed_column >> bit & 1 == 0
    )
    if document.get("variables") != len(outside):
        raise ValueError("projected core frontier has the wrong variable count")
    local_permutations = tuple(itertools.permutations(range(len(outside))))
    labelled_cores: set[tuple[int, ...]] = set()
    for orbit in document.get("orbit_cores", []):
        tests = tuple(int(test) for test in orbit["tests"])
        for permutation in local_permutations:
            labelled_cores.add(
                tuple(sorted(permute_pattern(test, permutation) for test in tests))
            )

    selectors_by_trace: dict[int, list[int]] = {}
    for pattern, selector in selected.items():
        trace = projected_pattern(pattern, outside)
        if trace != 0:
            selectors_by_trace.setdefault(trace, []).append(selector)
    trace_presence: dict[int, int] = {}
    for trace, selectors in selectors_by_trace.items():
        presence = pool.id(("projected-trace", fixed_column, trace))
        trace_presence[trace] = presence
        cnf.append([-presence, *selectors])

    core_variables = []
    for core_index, core in enumerate(sorted(labelled_cores)):
        if any(trace not in trace_presence for trace in core):
            continue
        core_variable = pool.id(("projected-core", fixed_column, core_index))
        core_variables.append(core_variable)
        for trace in core:
            cnf.append([-core_variable, trace_presence[trace]])
    cnf.append([-selected[fixed_column], *core_variables])
    digest = hashlib.blake2b(
        frontier_path.read_bytes(), digest_size=32
    ).hexdigest()
    return (
        int(document["quotient_threshold"]),
        len(labelled_cores),
        len(core_variables),
        digest,
    )


def add_stabilizer_lex_leaders(
    cnf: CNF,
    pool: IDPool,
    selected: dict[int, int],
    generator_count: int,
    fixed_column: int,
) -> int:
    inside = tuple(bit for bit in range(generator_count) if fixed_column >> bit & 1)
    outside = tuple(bit for bit in range(generator_count) if not (fixed_column >> bit & 1))
    identity = tuple(range(generator_count))
    leaders = 0
    ordered_patterns = sorted(selected)
    for inside_image in itertools.permutations(inside):
        for outside_image in itertools.permutations(outside):
            permutation = list(identity)
            for source, target in zip(inside, inside_image, strict=True):
                permutation[source] = target
            for source, target in zip(outside, outside_image, strict=True):
                permutation[source] = target
            permutation_tuple = tuple(permutation)
            if permutation_tuple == identity:
                continue
            leaders += 1
            prefix_equal = pool.id(("lex-prefix", leaders, 0))
            cnf.append([prefix_equal])
            for index, pattern in enumerate(ordered_patterns):
                left = selected[pattern]
                right = selected[permute_pattern(pattern, permutation_tuple)]
                cnf.append([-prefix_equal, -left, right])
                next_equal = pool.id(("lex-prefix", leaders, index + 1))
                cnf.append([-next_equal, prefix_equal])
                cnf.append([-next_equal, -left, right])
                cnf.append([-next_equal, left, -right])
                cnf.append([-prefix_equal, -left, -right, next_equal])
                cnf.append([-prefix_equal, left, right, next_equal])
                prefix_equal = next_equal
    return leaders


def solve(
    generator_count: int,
    column_count: int,
    maximum_column_weight: int,
    minimum_family_size: int,
    maximum_family_size: int,
    fixed_column: int | None,
    require_connected: bool,
    triple_count: int | None,
    singleton_count: int | None,
    triple_intersections: tuple[int, int, int] | None,
    local_triple_absence_cut: bool,
    projected_minority_encoding: bool,
    projected_core_frontier: Path | None,
    all_triple_core_frontier: Path | None,
    fixed_triple_singleton_traces: int | None,
    solver_name: str,
    output: Path | None,
) -> int:
    if not 1 <= generator_count <= 10:
        raise ValueError("generator_count must lie in 1..=10")
    patterns = [
        pattern
        for pattern in range(1, 1 << generator_count)
        if pattern.bit_count() <= maximum_column_weight
    ]
    if not 1 <= column_count <= len(patterns):
        raise ValueError("column_count exceeds the legal pattern class")
    if fixed_column is not None and fixed_column not in patterns:
        raise ValueError("fixed_column is not a legal pattern")
    if projected_minority_encoding and minimum_family_size != maximum_family_size:
        raise ValueError("projected_minority_encoding requires an exact family size")
    if projected_core_frontier is not None and all_triple_core_frontier is not None:
        raise ValueError("choose either fixed or all-triple projected core cuts")
    if fixed_triple_singleton_traces is not None and not 0 <= fixed_triple_singleton_traces <= 5:
        raise ValueError("fixed_triple_singleton_traces must lie in 0..=5")

    pool = IDPool()
    cnf = CNF()
    selected = {pattern: pool.id(("column", pattern)) for pattern in patterns}
    first = [pool.id(("first", subset)) for subset in range(1 << generator_count)]
    cnf.extend(
        CardEnc.equals(
            lits=list(selected.values()),
            bound=column_count,
            vpool=pool,
            encoding=EncType.seqcounter,
        ).clauses
    )
    if fixed_column is not None:
        cnf.append([selected[fixed_column]])
        stabilizer_lex_leaders = add_stabilizer_lex_leaders(
            cnf,
            pool,
            selected,
            generator_count,
            fixed_column,
        )
    else:
        stabilizer_lex_leaders = 0
    active_core_frontier = projected_core_frontier or all_triple_core_frontier
    if active_core_frontier is not None:
        if fixed_column is None or fixed_column.bit_count() != 3:
            raise ValueError("projected_core_frontier requires a fixed triple column")
        anchors = (
            [pattern for pattern in patterns if pattern.bit_count() == 3]
            if all_triple_core_frontier is not None
            else [fixed_column]
        )
        projected_core_labelled = 0
        projected_core_usable = 0
        projected_core_threshold = None
        projected_core_digest = None
        for anchor in anchors:
            threshold, labelled, usable, digest = add_projected_core_cut(
                cnf,
                pool,
                selected,
                generator_count,
                anchor,
                active_core_frontier,
            )
            projected_core_threshold = threshold
            projected_core_labelled += labelled
            projected_core_usable += usable
            projected_core_digest = digest
        if projected_core_threshold > minimum_family_size // 2 + 1:
            raise ValueError(
                "projected core threshold exceeds the necessary absence bound"
            )
    else:
        projected_core_threshold = None
        projected_core_labelled = 0
        projected_core_usable = 0
        projected_core_digest = None
    necessary_singleton_traces = (
        max(0, minimum_family_size // 2 + 1 - 27)
        if projected_minority_encoding
        else 0
    )
    fixed_singleton_trace_variables: list[int] = []
    if (
        fixed_column is not None
        and fixed_column.bit_count() == 3
        and (
            fixed_triple_singleton_traces is not None
            or necessary_singleton_traces > 0
        )
    ):
        outside = tuple(
            bit
            for bit in range(generator_count)
            if fixed_column >> bit & 1 == 0
        )
        for local_index in range(len(outside)):
            trace = 1 << local_index
            selectors = [
                selector
                for pattern, selector in selected.items()
                if projected_pattern(pattern, outside) == trace
            ]
            presence = pool.id(("projected-trace", fixed_column, trace))
            fixed_singleton_trace_variables.append(presence)
            cnf.append([-presence, *selectors])
            for selector in selectors:
                cnf.append([-selector, presence])
        if fixed_triple_singleton_traces is not None:
            cnf.extend(
                CardEnc.equals(
                    lits=fixed_singleton_trace_variables,
                    bound=fixed_triple_singleton_traces,
                    vpool=pool,
                    encoding=EncType.seqcounter,
                ).clauses
            )
        if necessary_singleton_traces > 0:
            cnf.extend(
                CardEnc.atleast(
                    lits=fixed_singleton_trace_variables,
                    bound=necessary_singleton_traces,
                    vpool=pool,
                    encoding=EncType.seqcounter,
                ).clauses
            )
    if triple_count is not None:
        triple_columns = [
            selected[pattern]
            for pattern in patterns
            if pattern.bit_count() == 3
        ]
        if not 0 <= triple_count <= min(column_count, len(triple_columns)):
            raise ValueError("triple_count is outside the legal range")
        cnf.extend(
            CardEnc.equals(
                lits=triple_columns,
                bound=triple_count,
                vpool=pool,
                encoding=EncType.seqcounter,
            ).clauses
        )
    if singleton_count is not None:
        singleton_columns = [
            selected[pattern]
            for pattern in patterns
            if pattern.bit_count() == 1
        ]
        if not 0 <= singleton_count <= min(column_count, len(singleton_columns)):
            raise ValueError("singleton_count is outside the legal range")
        cnf.extend(
            CardEnc.equals(
                lits=singleton_columns,
                bound=singleton_count,
                vpool=pool,
                encoding=EncType.seqcounter,
            ).clauses
        )
    if triple_intersections is not None:
        if fixed_column is None:
            raise ValueError("triple_intersections requires a fixed column")
        if len(triple_intersections) != 3 or any(count < 0 for count in triple_intersections):
            raise ValueError("triple_intersections must contain three nonnegative counts")
        if triple_count is not None and sum(triple_intersections) != triple_count - 1:
            raise ValueError("triple_intersections must account for every non-fixed triple")
        for intersection_size, count in enumerate(triple_intersections):
            category = [
                selected[pattern]
                for pattern in patterns
                if pattern.bit_count() == 3
                and pattern != fixed_column
                and (pattern & fixed_column).bit_count() == intersection_size
            ]
            cnf.extend(
                CardEnc.equals(
                    lits=category,
                    bound=count,
                    vpool=pool,
                    encoding=EncType.seqcounter,
                ).clauses
            )
    if require_connected:
        full = (1 << generator_count) - 1
        for left in range(1, full):
            if left & 1 == 0:
                continue
            right = full ^ left
            cnf.append(
                [
                    selected[pattern]
                    for pattern in patterns
                    if pattern & left and pattern & right
                ]
            )

    # sep(S,T) is true exactly when a selected incidence column distinguishes
    # the two generator subcollections.  first(S) then identifies the
    # lexicographically first input producing each distinct quotient member.
    cnf.append([first[0]])
    separation_variables = 0
    separation_implications = 0
    separation_by_pair: dict[tuple[int, int], int] = {}
    for subset in range(1, 1 << generator_count):
        separations = []
        for prior in range(subset):
            distinguishing = [
                selected[pattern]
                for pattern in patterns
                if bool(subset & pattern) != bool(prior & pattern)
            ]
            separation = pool.id(("separation", subset, prior))
            separation_by_pair[(subset, prior)] = separation
            separation_variables += 1
            separations.append(separation)
            cnf.append([-separation, *distinguishing])
            for variable in distinguishing:
                cnf.append([-variable, separation])
                separation_implications += 1
            cnf.append([-first[subset], separation])
        cnf.append([first[subset], *(-separation for separation in separations)])

    # If a selected coordinate is a strict minority in a family of at least
    # M members, deleting every generator incident with that coordinate must
    # still leave at least floor(M / 2) + 1 distinct quotient states.  For a
    # weight-three column this is a quotient on only g - 3 generators.  The
    # following exact projected partition either strengthens the ranged model
    # for triples or, at one exact family size, replaces the larger mixed
    # global minority counters altogether.
    local_absence_bound = minimum_family_size // 2 + 1
    local_first_variables = 0
    projected_patterns = (
        patterns
        if projected_minority_encoding
        else [pattern for pattern in patterns if pattern.bit_count() == 3]
        if local_triple_absence_cut
        else []
    )
    for pattern in projected_patterns:
        outside_subsets = [
            subset
            for subset in range(1 << generator_count)
            if subset & pattern == 0
        ]
        local_first = []
        for local_index, subset in enumerate(outside_subsets):
            variable = pool.id(("local-first", pattern, subset))
            local_first_variables += 1
            local_first.append(variable)
            if local_index == 0:
                cnf.append([variable])
                continue
            separations = [
                separation_by_pair[(subset, prior)]
                for prior in outside_subsets[:local_index]
            ]
            for separation in separations:
                cnf.append([-variable, separation])
            cnf.append([variable, *(-separation for separation in separations)])
        projected_bound = CardEnc.atleast(
            lits=local_first,
            bound=local_absence_bound,
            vpool=pool,
            encoding=EncType.seqcounter,
        )
        guard = -selected[pattern]
        cnf.extend([clause + [guard] for clause in projected_bound.clauses])

    cnf.extend(
        CardEnc.atleast(
            lits=first,
            bound=minimum_family_size,
            vpool=pool,
            encoding=EncType.seqcounter,
        ).clauses
    )
    cnf.extend(
        CardEnc.atmost(
            lits=first,
            bound=maximum_family_size,
            vpool=pool,
            encoding=EncType.seqcounter,
        ).clauses
    )

    # 2*frequency(pattern) < family_size is equivalent to an at-most
    # constraint over mixed first/not-first literals.  Guard every generated
    # clause by the column selector so unselected patterns carry no meaning.
    if not projected_minority_encoding:
        for pattern in patterns:
            mixed = [
                first[subset] if subset & pattern else -first[subset]
                for subset in range(1 << generator_count)
            ]
            absent_inputs = 1 << (generator_count - pattern.bit_count())
            minority = CardEnc.atmost(
                lits=mixed,
                bound=absent_inputs - 1,
                vpool=pool,
                encoding=EncType.seqcounter,
            )
            guard = -selected[pattern]
            cnf.extend([clause + [guard] for clause in minority.clauses])

    digest = hashlib.blake2b(digest_size=32)
    for clause in cnf.clauses:
        digest.update(len(clause).to_bytes(4, "little"))
        for literal in clause:
            digest.update(int(literal).to_bytes(4, "little", signed=True))
    build_seconds = perf_counter()
    started = perf_counter()
    with Solver(name=solver_name, bootstrap_with=cnf.clauses) as solver:
        satisfiable = solver.solve()
        elapsed = perf_counter() - started
        model = set(solver.get_model() or [])
    columns = [pattern for pattern, variable in selected.items() if variable in model]
    family = sorted(
        {
            sum(1 << coordinate for coordinate, pattern in enumerate(columns) if subset & pattern)
            for subset in range(1 << generator_count)
        }
    ) if satisfiable else []
    frequencies = [
        sum(member >> coordinate & 1 for member in family)
        for coordinate in range(len(columns))
    ]
    if satisfiable and (
        len(columns) != column_count
        or not minimum_family_size <= len(family) <= maximum_family_size
        or not all(2 * frequency < len(family) for frequency in frequencies)
    ):
        raise RuntimeError("SAT model failed independent quotient validation")
    document = {
        "schema": "rad.boolean-lattice.generator-quotient-cnf.v1",
        "generator_count": generator_count,
        "column_count": column_count,
        "maximum_column_weight": maximum_column_weight,
        "minimum_family_size": minimum_family_size,
        "maximum_family_size": maximum_family_size,
        "fixed_column": fixed_column,
        "connected": require_connected,
        "triple_count": triple_count,
        "singleton_count": singleton_count,
        "triple_intersections": triple_intersections,
        "local_triple_absence_cut": local_triple_absence_cut,
        "projected_minority_encoding": projected_minority_encoding,
        "local_absence_bound": local_absence_bound
        if local_triple_absence_cut or projected_minority_encoding
        else None,
        "local_first_variables": local_first_variables,
        "projected_core_frontier": str(projected_core_frontier)
        if projected_core_frontier
        else None,
        "all_triple_core_frontier": str(all_triple_core_frontier)
        if all_triple_core_frontier
        else None,
        "projected_core_labelled": projected_core_labelled,
        "projected_core_usable": projected_core_usable,
        "projected_core_threshold": projected_core_threshold,
        "fixed_triple_singleton_traces": fixed_triple_singleton_traces,
        "necessary_singleton_traces": necessary_singleton_traces,
        "projected_core_digest": projected_core_digest,
        "variables": pool.top,
        "clauses": len(cnf.clauses),
        "separation_variables": separation_variables,
        "separation_implications": separation_implications,
        "stabilizer_lex_leaders": stabilizer_lex_leaders,
        "cnf_digest": digest.hexdigest(),
        "solver": solver_name,
        "result": "sat" if satisfiable else "unsat",
        "counterexample_exists": satisfiable,
        "solve_seconds": elapsed,
        "columns": columns,
        "family": family,
        "frequencies": frequencies,
    }
    encoded = json.dumps(document, indent=2, sort_keys=True)
    print(encoded)
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(encoded + "\n", encoding="utf-8")
    return 1 if satisfiable else 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--generators", type=int, default=8)
    parser.add_argument("--columns", type=int, default=13)
    parser.add_argument("--max-column-weight", type=int, default=3)
    parser.add_argument("--min-family-size", type=int, default=51)
    parser.add_argument("--max-family-size", type=int, default=63)
    parser.add_argument("--fixed-column", type=lambda value: int(value, 0), default=0b111)
    parser.add_argument("--allow-disconnected", action="store_true")
    parser.add_argument("--triple-count", type=int)
    parser.add_argument("--singleton-count", type=int)
    parser.add_argument(
        "--triple-intersections",
        help="counts of other triples meeting the fixed triple in 0,1,2 vertices",
    )
    parser.add_argument(
        "--local-triple-absence-cut",
        action="store_true",
        help="expose the necessary projected quotient bound for selected triples",
    )
    parser.add_argument(
        "--projected-minority-encoding",
        action="store_true",
        help="for an exact family size, encode minority through outside quotients",
    )
    parser.add_argument(
        "--projected-core-frontier",
        type=Path,
        help="minimal projected test cores used as a redundant fixed-column cut",
    )
    parser.add_argument(
        "--all-triple-core-frontier",
        type=Path,
        help="apply a projected test-core frontier to every selected triple",
    )
    parser.add_argument(
        "--fixed-triple-singleton-traces",
        type=int,
        help="exact number of singleton traces outside the fixed triple",
    )
    parser.add_argument("--solver", default="cadical195")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    return solve(
        args.generators,
        args.columns,
        args.max_column_weight,
        args.min_family_size,
        args.max_family_size,
        args.fixed_column,
        not args.allow_disconnected,
        args.triple_count,
        args.singleton_count,
        tuple(int(value) for value in args.triple_intersections.split(","))
        if args.triple_intersections
        else None,
        args.local_triple_absence_cut,
        args.projected_minority_encoding,
        args.projected_core_frontier,
        args.all_triple_core_frontier,
        args.fixed_triple_singleton_traces,
        args.solver,
        args.output,
    )


if __name__ == "__main__":
    raise SystemExit(main())
