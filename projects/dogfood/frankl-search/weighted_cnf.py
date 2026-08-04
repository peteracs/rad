"""Small dependency-light CNF encoder for weighted Boolean upper bounds.

The encoder builds a ripple-adder circuit with enough bits for the maximum
reachable sum, then constrains that sum with an unsigned constant comparator.
Inputs are signed DIMACS literals paired with nonnegative integer weights.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class CnfBuilder:
    variable_count: int
    clauses: list[list[int]] = field(default_factory=list)

    def __post_init__(self) -> None:
        self.true_literal = self.new_variable()
        self.clauses.append([self.true_literal])

    @property
    def false_literal(self) -> int:
        return -self.true_literal

    def new_variable(self) -> int:
        self.variable_count += 1
        return self.variable_count

    def and_gate(self, left: int, right: int) -> int:
        output = self.new_variable()
        self.clauses.extend(([-output, left], [-output, right], [output, -left, -right]))
        return output

    def or_gate(self, left: int, right: int) -> int:
        output = self.new_variable()
        self.clauses.extend(([output, -left], [output, -right], [-output, left, right]))
        return output

    def xor_gate(self, left: int, right: int) -> int:
        output = self.new_variable()
        self.clauses.extend(
            (
                [-left, -right, -output],
                [left, right, -output],
                [left, -right, output],
                [-left, right, output],
            )
        )
        return output

    def add_vectors(self, left: list[int], right: list[int]) -> list[int]:
        if len(left) != len(right):
            raise ValueError("binary addends must have equal widths")
        carry = self.false_literal
        result: list[int] = []
        for left_bit, right_bit in zip(left, right):
            pair_xor = self.xor_gate(left_bit, right_bit)
            result.append(self.xor_gate(pair_xor, carry))
            pair_carry = self.and_gate(left_bit, right_bit)
            ripple_carry = self.and_gate(pair_xor, carry)
            carry = self.or_gate(pair_carry, ripple_carry)
        # The caller sizes the vector for the maximum reachable sum, so a true
        # final carry would signal an encoder bug or an understated bound.
        self.clauses.append([-carry])
        return result

    def constrain_at_most(self, bits: list[int], bound: int) -> None:
        if bound < 0:
            self.clauses.append([])
            return
        if bound >= 1 << len(bits):
            return
        equal_prefix = self.true_literal
        for index in reversed(range(len(bits))):
            bit = bits[index]
            bound_bit = bound >> index & 1
            if bound_bit == 0:
                self.clauses.append([-equal_prefix, -bit])
                equal_here = -bit
            else:
                equal_here = bit
            equal_prefix = self.and_gate(equal_prefix, equal_here)


def encode_weighted_at_most(
    input_variable_count: int,
    terms: list[tuple[int, int]],
    bound: int,
) -> tuple[int, list[list[int]]]:
    """Encode ``sum(weight * truth(literal)) <= bound`` into CNF."""

    if any(weight < 0 for _, weight in terms):
        raise ValueError("weights must be nonnegative")
    maximum_sum = sum(weight for _, weight in terms)
    bit_width = max(1, maximum_sum.bit_length())
    builder = CnfBuilder(input_variable_count)
    accumulator = [builder.false_literal] * bit_width
    for literal, weight in terms:
        addend = [literal if weight >> bit & 1 else builder.false_literal for bit in range(bit_width)]
        accumulator = builder.add_vectors(accumulator, addend)
    builder.constrain_at_most(accumulator, bound)
    return builder.variable_count, builder.clauses
