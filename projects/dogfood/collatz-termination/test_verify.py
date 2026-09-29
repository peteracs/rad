import unittest

from verify import check, check_polynomial, known_natural_control, arctic_control, polynomial_control


class CertificateTests(unittest.TestCase):
    def test_published_control(self):
        self.assertEqual(check(known_natural_control())['strict_rules'],[8,9,10])

    def test_rational_rescaling_preserves_proof(self):
        row=known_natural_control()
        row['denominator']=2
        row['maximum']*=2
        row['matrices']=[[[2*x for x in r] for r in a] for a in row['matrices']]
        self.assertEqual(check(row)['strict_rules'],[8,9,10])

    def test_reinserted_missing_rule_rejects(self):
        row=known_natural_control();row['omitted']=-1
        with self.assertRaises(ValueError): check(row)

    def test_missing_rule_cannot_be_called_collatz_proof(self):
        row=known_natural_control();row['complete_proof']=True
        with self.assertRaises(ValueError): check(row)

    def test_bad_homogeneous_coordinate_rejected(self):
        row=known_natural_control();row['matrices'][0][-1][0]=1
        with self.assertRaises(ValueError): check(row)

    def test_arctic_negative_weights_and_infinity(self):
        self.assertEqual(check(arctic_control())['strict_rules'],[8,9,10])

    def test_arctic_carrier_escape_rejected(self):
        row=arctic_control();row['matrices'][0][0]=[-1,-1]
        with self.assertRaises(ValueError): check(row)

    def test_arctic_infinity_is_not_minus_one(self):
        row=arctic_control();row['matrices'][3][1][0]=-1
        with self.assertRaises(ValueError): check(row)

    def test_nonlinear_control(self):
        self.assertEqual(check_polynomial(polynomial_control())['strict_rules'],[9])

    def test_polynomial_missing_rule_rejected(self):
        row=polynomial_control();row['omitted']=-1
        with self.assertRaises(ValueError): check_polynomial(row)

    def test_polynomial_overflow_domain_rejected(self):
        row=polynomial_control();row['degree']=4
        with self.assertRaises(ValueError): check_polynomial(row)

    def test_polynomial_nonmonotone_coefficient_rejected(self):
        row=polynomial_control();row['polynomials'][0][1]=-1
        with self.assertRaises(ValueError): check_polynomial(row)


if __name__=='__main__': unittest.main()
