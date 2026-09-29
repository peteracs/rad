"""Checks beyond nested chains, and counterexamples to overstrong variants."""

from itertools import combinations, permutations, product
import unittest

from capacity_oracle import decrease, power_image


class CapacityTests(unittest.TestCase):
    def test_upper_bound_does_not_require_nesting(self):
        for n in (2, 3):
            subsets = [{q for q in range(n) if mask & (1 << q)} for mask in range(1 << n)]
            for mapping in product(range(n), repeat=n):
                for budget in range(3):
                    capacity = n - len(power_image(mapping, budget + 1))
                    for chain in product(subsets, repeat=budget + 1):
                        self.assertLessEqual(decrease(mapping, chain), capacity)

    def test_rank_of_first_power_is_not_a_valid_general_bound(self):
        mapping = (0, 0, 1)
        chain = [{0, 1}, {0, 1, 2}]
        self.assertEqual(decrease(mapping, chain), 2)
        self.assertEqual(3 - len(power_image(mapping, 1)), 1)
        self.assertEqual(3 - len(power_image(mapping, 2)), 2)

    def test_permutation_fault_extension(self):
        n = 3
        for fault in permutations(range(n)):
            inverse = [fault.index(q) for q in range(n)]
            for mapping in product(range(n), repeat=n):
                relative = tuple(inverse[mapping[q]] for q in range(n))
                for budget in range(3):
                    capacity = n - len(power_image(relative, budget + 1))
                    maximum = 0
                    for thresholds in product(range(budget + 2), repeat=n):
                        chain = [{q for q in range(n) if thresholds[q] <= j}
                                 for j in range(budget + 1)]
                        maximum = max(maximum, decrease(mapping, chain, fault))
                    self.assertEqual(maximum, capacity)
                    witness = [power_image(relative, budget - j) for j in range(budget + 1)]
                    self.assertEqual(decrease(mapping, witness, fault), capacity)

    def test_audit_supplied_whole_word_sharpness_example(self):
        for n in range(2, 7):
            for budget in range(4):
                word = [i for i in range(n - 1, 0, -1) for _ in range(budget + 1)]
                self.assertEqual(len(word), (budget + 1) * (n - 1))
                for count in range(budget + 1):
                    for deleted in combinations(range(len(word)), count):
                        deleted = set(deleted)
                        for initial in range(n):
                            q = initial
                            for position, letter in enumerate(word):
                                if position not in deleted and q == letter:
                                    q -= 1
                            self.assertEqual(q, 0)


if __name__ == "__main__":
    unittest.main()
