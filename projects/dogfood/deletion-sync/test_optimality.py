"""Adversarial checks of the new proof rules, objectives, and evidence."""

import copy
from itertools import combinations
import unittest

from backward_oracle import verify_rows, weighted_search
from oracle import solve, square_word


def interval_left(states, n):
    endpoints = [q for q in states if (q - 1) % n not in states]
    if len(endpoints) != 1:
        raise ValueError("not a proper nonempty interval")
    return endpoints[0]


class OptimalityTests(unittest.TestCase):
    def test_backward_agrees_with_forward_and_separate_objectives(self):
        for n in range(3, 9):
            self.assertEqual(weighted_search(n, 1, 1)["cost"], solve(n, 1)["length"])
            for a, b in ((1, 0), (0, 1), (3, 7), (7, 3)):
                self.assertEqual(weighted_search(n, a, b)["cost"],
                                 a * ((n - 1) ** 2 + 1) + b * (2 * n - 2))

    def test_evidence_mutations_and_missing_rows_fail(self):
        rows = [weighted_search(n, a, b) for n in (3, 4)
                for a, b in ((1, 1), (1, 0), (0, 1))]
        self.assertEqual(verify_rows(rows, 4), rows)
        for field, value in (("cost", -1), ("states", 0), ("a_cost", True), ("n", 5)):
            mutant = copy.deepcopy(rows)
            mutant[0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                verify_rows(mutant, 4)
        for mutant in (rows[:-1], rows + rows[:1]):
            with self.assertRaises(ValueError):
                verify_rows(mutant, 4)

    def test_interval_rules_for_arbitrary_outer_supersets(self):
        for n in range(3, 8):
            full = set(range(n))
            for left in range(n):
                for size in range(1, n):
                    inner = {(left + j) % n for j in range(size)}
                    rest = sorted(full - inner)
                    for count in range(len(rest) + 1):
                        for extra in combinations(rest, count):
                            outer = inner | set(extra)
                            for letter in (0, 1):
                                def step(q):
                                    return (q + 1) % n if letter == 0 else (0 if q == n - 1 else q)
                                successor = {q for q in outer if step(q) in inner}
                                if not successor or successor == full:
                                    continue
                                new_left = interval_left(successor, n)
                                delta = len(successor) - size
                                if letter == 0:
                                    self.assertIn(delta, (-1, 0))
                                    self.assertEqual(new_left, left if delta == -1 else (left - 1) % n)
                                else:
                                    self.assertIn(delta, (-1, 0, 1))
                                    if delta == 1:
                                        self.assertEqual((left, new_left), (0, n - 1))
                                    else:
                                        self.assertEqual(new_left, left)

    def test_both_counting_cases_on_resilient_words(self):
        # An extra final b makes the immediate-growth case occur; it is still
        # resilient. W itself realizes the delayed-growth case.
        for n in range(3, 26):
            for early in (False, True):
                word = square_word(n) + ("b" if early else "")
                outer = inner = {0}
                rotations = growths = stationary = 0
                before_growth = None
                for letter in reversed(word):
                    def step(q):
                        return (q + 1) % n if letter == "a" else (0 if q == n - 1 else q)
                    next_inner = {q for q in outer if step(q) in inner}
                    next_outer = {q for q in range(n) if step(q) in outer}
                    if letter == "a":
                        rotations += 1
                        stationary += len(next_inner) < len(inner)
                    if len(next_inner) > len(inner):
                        growths += 1
                        if before_growth is None:
                            before_growth = rotations
                    outer, inner = next_outer, next_inner
                    self.assertTrue(inner)
                    if len(inner) == n:
                        break
                self.assertEqual(len(inner), n)
                self.assertEqual(before_growth, 0 if early else n)
                self.assertGreaterEqual(growths, n if early else n - 1)
                self.assertGreaterEqual(stationary, int(early))
                self.assertGreaterEqual(rotations, before_growth + (growths - 1) * (n - 1) + stationary)
                self.assertEqual(rotations, (n - 1) ** 2 + 1)


if __name__ == "__main__":
    unittest.main()
