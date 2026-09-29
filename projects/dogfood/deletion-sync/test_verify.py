"""Adversarial evidence checks and an independent small-word oracle audit."""

import copy
from itertools import product
import unittest

from oracle import direct_images, solve, square_word
from verify import verify_rows


class VerificationTests(unittest.TestCase):
    def setUp(self):
        self.rows = [solve(n, k) for n in (2, 3) for k in range(3)]

    def test_complete_matrix(self):
        self.assertEqual(len(verify_rows(self.rows, 3)), 6)

    def test_mutated_evidence_is_rejected(self):
        for field, value in (("word", "b"), ("length", 1), ("target", 1),
                             ("discovered", 0), ("expanded", 0), ("n", True)):
            with self.subTest(field=field):
                rows = copy.deepcopy(self.rows)
                rows[4][field] = value
                with self.assertRaises(ValueError):
                    verify_rows(rows, 3)

    def test_missing_duplicate_and_wrong_instances_are_rejected(self):
        for rows in (self.rows[:-1], self.rows + self.rows[:1], self.rows[1:]):
            with self.assertRaises(ValueError):
                verify_rows(rows, 3)

    def test_n3_optimum_by_literal_word_enumeration(self):
        for length in range(9):
            for letters in product("ab", repeat=length):
                self.assertNotEqual(len(direct_images(3, "".join(letters), 1)), 1)
        self.assertEqual(direct_images(3, square_word(3), 1), {0})

    def test_uniform_word_and_fragile_shortcut(self):
        for n in range(3, 11):
            word = square_word(n)
            self.assertEqual(len(word), n * n)
            self.assertEqual(direct_images(n, word, 1), {0})
            self.assertGreater(len(direct_images(n, word, 2)), 1)

    def test_rank_budget_for_all_small_nested_sets_and_idempotents(self):
        # Test the local lemma even on chains that no automaton word can reach.
        for n in range(2, 5):
            transformations = []
            for mapping in product(range(n), repeat=n):
                permutation = len(set(mapping)) == n
                idempotent = all(mapping[mapping[q]] == mapping[q] for q in range(n))
                if permutation or idempotent:
                    transformations.append((mapping, 0 if permutation else n - len(set(mapping))))
            for budget in range(4):
                for thresholds in product(range(budget + 2), repeat=n):
                    chain = [{q for q in range(n) if thresholds[q] <= j}
                             for j in range(budget + 1)]
                    before = sum(map(len, chain))
                    for mapping, bound in transformations:
                        successor = [{mapping[q] for q in states} | (chain[j - 1] if j else set())
                                     for j, states in enumerate(chain)]
                        self.assertLessEqual(before - sum(map(len, successor)), bound)


if __name__ == "__main__":
    unittest.main()
