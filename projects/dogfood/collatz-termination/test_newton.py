"""Tests for all-integer proof obligations, exact arithmetic and encoding."""

from copy import deepcopy
from math import comb
import random
import unittest

import z3

from newton_expansion import profile_arithmetic_bound, rule_certificates
from newton_verify import check_newton, evaluate, newton_control, signed_control, tail_coefficients, word_value
from verify import RULES


class NewtonCertificates(unittest.TestCase):
    def test_all_degree_controls(self):
        for degree in (2,3,4):
            self.assertFalse(check_newton(newton_control(degree))['complete_proof'])

    def test_missing_rule_is_essential(self):
        row = newton_control()
        row['omitted'] = -1
        with self.assertRaisesRegex(ValueError,'negative Newton'):
            check_newton(row)

    def test_reduced_system_cannot_claim_completion(self):
        row = newton_control()
        row['complete_proof'] = True
        with self.assertRaisesRegex(ValueError,'completeness'):
            check_newton(row)

    def test_shape_and_domain_mutations(self):
        original = newton_control()
        for key,bad in (('degree',5),('tail',-1),('tail',33),('maximum',16)):
            row = deepcopy(original)
            row[key] = bad
            with self.assertRaises(ValueError):
                check_newton(row)
        row = deepcopy(original)
        row['polynomials'][0][1] = -1
        with self.assertRaisesRegex(ValueError,'monotonicity'):
            check_newton(row)

    def test_signed_nonconvex_control(self):
        row = signed_control()
        self.assertFalse(check_newton(row)['complete_proof'])
        self.assertEqual([evaluate(row['polynomials'][2],x) for x in range(5)],[0,7,8,9,16])
        row['tail'] = 0
        with self.assertRaisesRegex(ValueError,'monotonicity'):
            check_newton(row)

    def test_shifted_tail_adds_power(self):
        # x^2-3x+3 is positive on every integer. Its origin-zero
        # Newton certificate fails; origin two succeeds, including prefix.
        polynomial = lambda x: x*x-3*x+3
        self.assertEqual(tail_coefficients([polynomial(x) for x in range(3)]),[3,-2,2])
        self.assertEqual(tail_coefficients([polynomial(x) for x in range(2,5)]),[1,2,2])
        self.assertTrue(all(polynomial(x)>0 for x in (0,1)))

    def test_positive_samples_do_not_certify_tail(self):
        self.assertEqual(tail_coefficients([100,99]),[100,-1])
        self.assertTrue(any(c<0 for c in tail_coefficients([0,1,2,2])))

    def test_pascal_identity_and_integer_domain(self):
        for n in range(25):
            for k in range(1,8):
                self.assertEqual(comb(n+1,k)-comb(n,k),comb(n,k-1))
        self.assertEqual(evaluate([0,0,1],0),0)
        self.assertEqual(evaluate([0,0,1],1),0)
        self.assertEqual(evaluate([0,0,1],2),1)

    def test_values_beyond_native_integer_range(self):
        row = newton_control(4)
        result = word_value('ccc',row,64)
        self.assertEqual(result,88597259123193965850611463382958470855819567111205535250935377064261699023551558853625)
        self.assertGreater(result,2**256)

    def test_expansion_matches_unbounded_direct_evaluation(self):
        # Fixed random coefficients, both word orders and unequal lengths.
        rng = random.Random(1937)
        for degree in (2,3,4):
            row = newton_control(degree)
            row['polynomials'] = [[rng.randrange(4) for _ in range(degree+1)] for _ in range(7)]
            letters = {s:[z3.IntVal(v) for v in p] for s,p in zip('abcdefg',row['polynomials'])}
            certificate = rule_certificates(letters,degree)
            for left,right in (('ad','d'),('fc','aac'),('cg','cab')):
                prefix,tail = certificate(left,right,2)
                actual = [word_value(left,row,x)-word_value(right,row,x)
                          for x in range(2+degree**max(len(left),len(right))+1)]
                expected = actual[:2]+tail_coefficients(actual[2:])
                encoded = [v.as_long() for v in prefix+tail]
                encoded += [0]*(len(expected)-len(encoded))
                # Cross multiplication scales all coefficients by one
                # positive denominator, preserving zero, signs and ratios.
                nonzero = next(i for i,v in enumerate(expected) if v)
                scale = encoded[nonzero]//expected[nonzero]
                self.assertGreater(scale,0)
                self.assertEqual(encoded,[scale*v for v in expected])

    def test_signed_bitvector_expansion_matches_exact_integers(self):
        rng = random.Random(286)
        for tail in (0,2,8):
            upper = profile_arithmetic_bound(7,tail,RULES)
            width = upper.bit_length()+2
            data = {s:[rng.randrange(-7,8) for _ in range(d+1)]+[0]*(4-d)
                    for s,d in dict(a=2,b=2,c=1,d=0,e=4,f=4,g=4).items()}
            data['c'] = [0,1,0,0,0]
            exact = rule_certificates({s:list(map(z3.IntVal,p)) for s,p in data.items()},4)
            bounded = rule_certificates({s:[z3.BitVecVal(v,width) for v in p] for s,p in data.items()},4)
            for left,right in RULES:
                a,b = exact(left,right,tail)
                c,d = bounded(left,right,tail)
                expected = [v.as_long() for v in a+b]
                self.assertTrue(all(abs(v)<=upper for v in expected))
                self.assertEqual([v.as_signed_long() for v in c+d],expected)


if __name__ == '__main__':
    unittest.main()
