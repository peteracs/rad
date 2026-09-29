"""Tests of exact obstruction semantics, context reduction, and query export."""

from copy import deepcopy
import random
import unittest

import z3

from investigate import DIGITS, OUT, counts, independent_row, key, verify_dual
from literal import derive
from modular import independent as modular_row
from orbit_alias import binary_word


def descriptor(rule, before=None, after=None):
    return dict(kind=0, rule=rule, before=before or [], after=after or [], state=[], symbol=-1)


def simple_obstruction():
    return dict(memory=1, boundary=1792, rows=[dict(multiplier=1, constraint=descriptor(rule)) for rule in (0, 1, 10)])


class ExactLocalRanks(unittest.TestCase):
    def test_three_rule_letter_count_identity(self):
        # a + (b-g) + (g-a-b) = 0, while cg -> cab requires strict decrease.
        self.assertEqual(verify_dual(simple_obstruction()), 1)

    def test_overlapping_factors(self):
        self.assertEqual(counts([1, 1, 1, 1], 3), {key([1]): 4, key([1, 1]): 3, key([1, 1, 1]): 2})

    def test_rejects_missing_strictness(self):
        proof = simple_obstruction(); proof['boundary'] = 256
        with self.assertRaisesRegex(ValueError, 'dual identity'): verify_dual(proof)

    def test_rejects_uncancelled_coefficients(self):
        proof = simple_obstruction(); proof['rows'][0]['multiplier'] = 2
        with self.assertRaisesRegex(ValueError, 'dual identity'): verify_dual(proof)

    def test_rejects_negative_multiplier(self):
        proof = simple_obstruction(); proof['rows'][0]['multiplier'] = -1
        with self.assertRaisesRegex(ValueError, 'multiplier'): verify_dual(proof)

    def test_rejects_illegal_context(self):
        proof = simple_obstruction(); proof['rows'][0]['constraint']['before'] = [2]
        with self.assertRaisesRegex(ValueError, 'context digit'): verify_dual(proof)

    def test_context_truncation_preserves_every_coefficient(self):
        rng = random.Random(1937)
        for memory in range(1, 5):
            for rule in range(11):
                for _ in range(10):
                    before = [] if rule >= 8 else rng.choices(DIGITS, k=rng.randrange(12))
                    after = [] if rule < 2 else rng.choices(DIGITS, k=rng.randrange(12))
                    short_before = before[-(memory-1):] if memory > 1 else []
                    short_after = after[:memory-1]
                    self.assertEqual(independent_row(descriptor(rule, before, after), memory, 1792),
                                     independent_row(descriptor(rule, short_before, short_after), memory, 1792))

    def test_negative_digit_cycle_cannot_be_hidden_by_state_potential(self):
        row = dict(kind=1, state=[1, 1], symbol=1)
        coefficients, bound = independent_row(row, 3, 1792)
        self.assertEqual(coefficients, {key([1]): 1, key([1, 1]): 1, key([1, 1, 1]): 1})
        self.assertEqual(bound, 0)

    def test_saved_tracking_literals_are_actual_assertions(self):
        source = (OUT / 'm1-b1792.smt2').read_text(encoding='utf-8')
        solver = z3.Solver(); solver.add(z3.parse_smt2_string(source))
        self.assertEqual(solver.check(), z3.unsat)
        # Removing tracked assumptions makes this different formula satisfiable.
        weakened = '\n'.join(line for line in source.splitlines() if not line.startswith('(assert row_'))
        solver = z3.Solver(); solver.add(z3.parse_smt2_string(weakened))
        self.assertEqual(solver.check(), z3.sat)

    def test_actual_orbit_alias_separates_four_from_five_symbols(self):
        left, right = binary_word(674), binary_word(650)
        self.assertEqual(counts(left, 4), counts(right, 4))
        self.assertNotEqual(counts(left, 5), counts(right, 5))
        self.assertEqual(674 % 24, 650 % 24)

    def test_alias_normalization_visits_every_rule(self):
        proof, evidence = derive(dict(left=674, right=650, memory=4, modulus=24, steps=54, odds=34))
        self.assertEqual(evidence['rewrite_steps'], 330)
        self.assertEqual(set(evidence['rule_counts']), set(range(11)))
        self.assertEqual(verify_dual(proof), 34)

    def test_modular_dynamic_rule_keeps_terminal_cost(self):
        terms, bound = modular_row(dict(rule=0, residue=3, symbol=-1), 5, 1792)
        self.assertEqual(terms, {'w:3:0': 1, 't:1': 1, 't:3': -1})
        self.assertEqual(bound, 0)

    def test_modular_boundary_cannot_start_in_arbitrary_state(self):
        with self.assertRaisesRegex(ValueError, 'leading state'):
            modular_row(dict(rule=8, residue=2, symbol=-1), 5, 1792)

    def test_every_modular_conversion_preserves_its_terminal_state(self):
        for modulus in range(1, 18):
            for rule in range(2, 11):
                for residue in ([1 % modulus] if rule >= 8 else range(modulus)):
                    terms, _ = modular_row(dict(rule=rule, residue=residue, symbol=-1), modulus, 1792)
                    self.assertFalse(any(k.startswith('t:') for k in terms))


if __name__ == '__main__': unittest.main()
