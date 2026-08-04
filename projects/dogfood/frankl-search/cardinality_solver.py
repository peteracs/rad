"""Small domain-neutral incremental cardinality solver adapters."""

from __future__ import annotations

from typing import Protocol


class CardinalitySolver(Protocol):
    def add_clause(self, literals: list[int]) -> None: ...

    def add_atmost(self, literals: list[int], bound: int) -> None: ...

    def add_atleast(self, literals: list[int], bound: int) -> None: ...

    def add_exactly(self, literals: list[int], bound: int) -> None: ...

    def solve(self) -> bool: ...

    def positive_model(self) -> set[int]: ...

    def prefer(self, positive: set[int]) -> None: ...

    def close(self) -> None: ...


class MiniCardSolver:
    def __init__(self, variable_count: int) -> None:
        from pysat.solvers import Solver

        self._variable_count = variable_count
        self._solver = Solver(name="minicard")

    def add_clause(self, literals: list[int]) -> None:
        self._solver.add_clause(literals)

    def add_atmost(self, literals: list[int], bound: int) -> None:
        self._solver.add_atmost(literals, bound)

    def add_atleast(self, literals: list[int], bound: int) -> None:
        self._solver.add_atmost([-literal for literal in literals], len(literals) - bound)

    def add_exactly(self, literals: list[int], bound: int) -> None:
        self.add_atmost(literals, bound)
        self.add_atleast(literals, bound)

    def solve(self) -> bool:
        return self._solver.solve()

    def positive_model(self) -> set[int]:
        return {
            literal
            for literal in self._solver.get_model() or []
            if 0 < literal <= self._variable_count
        }

    def prefer(self, positive: set[int]) -> None:
        self._solver.set_phases(
            [index if index in positive else -index for index in range(1, self._variable_count + 1)]
        )

    def close(self) -> None:
        self._solver.delete()


class Z3CardinalitySolver:
    def __init__(self, variable_count: int) -> None:
        import z3

        self._z3 = z3
        self._variables = [z3.Bool(f"x_{index}") for index in range(1, variable_count + 1)]
        self._solver = z3.Solver()
        self._solver.set(random_seed=0)
        self._model = None

    def _literal(self, literal: int):
        variable = self._variables[abs(literal) - 1]
        return variable if literal > 0 else self._z3.Not(variable)

    def add_clause(self, literals: list[int]) -> None:
        if not literals:
            self._solver.add(self._z3.BoolVal(False))
        else:
            self._solver.add(self._z3.Or(*(self._literal(literal) for literal in literals)))

    def add_atmost(self, literals: list[int], bound: int) -> None:
        self._solver.add(self._z3.PbLe([(self._literal(literal), 1) for literal in literals], bound))

    def add_atleast(self, literals: list[int], bound: int) -> None:
        self._solver.add(self._z3.PbGe([(self._literal(literal), 1) for literal in literals], bound))

    def add_exactly(self, literals: list[int], bound: int) -> None:
        self._solver.add(self._z3.PbEq([(self._literal(literal), 1) for literal in literals], bound))

    def solve(self) -> bool:
        result = self._solver.check()
        if result == self._z3.unknown:
            raise RuntimeError(f"Z3 returned unknown: {self._solver.reason_unknown()}")
        if result == self._z3.sat:
            self._model = self._solver.model()
            return True
        self._model = None
        return False

    def positive_model(self) -> set[int]:
        if self._model is None:
            return set()
        return {
            index
            for index, variable in enumerate(self._variables, 1)
            if self._z3.is_true(self._model.eval(variable, model_completion=True))
        }

    def prefer(self, positive: set[int]) -> None:
        for index, variable in enumerate(self._variables, 1):
            self._solver.set_initial_value(variable, index in positive)

    def close(self) -> None:
        self._model = None


class CnfCardinalitySolver:
    def __init__(self, variable_count: int) -> None:
        from pysat.solvers import Solver

        self._input_variable_count = variable_count
        self._top_id = variable_count
        self._solver = Solver(name="cadical195")

    def add_clause(self, literals: list[int]) -> None:
        self._solver.add_clause(literals)

    def _encode(self, literals: list[int], bound: int, kind: str) -> None:
        from pysat.card import CardEnc, EncType

        encoded = getattr(CardEnc, kind)(
            literals,
            bound=bound,
            top_id=self._top_id,
            encoding=EncType.kmtotalizer,
        )
        self._top_id = encoded.nv
        self._solver.append_formula(encoded.clauses)

    def add_atmost(self, literals: list[int], bound: int) -> None:
        self._encode(literals, bound, "atmost")

    def add_atleast(self, literals: list[int], bound: int) -> None:
        self._encode(literals, bound, "atleast")

    def add_exactly(self, literals: list[int], bound: int) -> None:
        self.add_atleast(literals, bound)
        self.add_atmost(literals, bound)

    def solve(self) -> bool:
        return self._solver.solve()

    def positive_model(self) -> set[int]:
        return {
            literal
            for literal in self._solver.get_model() or []
            if 0 < literal <= self._input_variable_count
        }

    def prefer(self, positive: set[int]) -> None:
        self._solver.set_phases(
            [
                index if index in positive else -index
                for index in range(1, self._input_variable_count + 1)
            ]
        )

    def close(self) -> None:
        self._solver.delete()


def make_cardinality_solver(name: str, variable_count: int) -> CardinalitySolver:
    if name == "minicard":
        return MiniCardSolver(variable_count)
    if name == "z3":
        return Z3CardinalitySolver(variable_count)
    if name == "cadical":
        return CnfCardinalitySolver(variable_count)
    raise ValueError(f"unknown cardinality solver: {name}")
