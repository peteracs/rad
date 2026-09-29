"""Independent tests of repair scores and proof-relevant accounting."""

from copy import deepcopy
import unittest

from repair_campaign import request
from repair_oracle import audit_report,diagnostics


class RepairAccounting(unittest.TestCase):
    def test_reduced_control_and_full_obstruction(self):
        req=request()
        self.assertEqual(diagnostics(req,req['matrices'])['score'],dict(weak=94,strict=0,total=94))
        req['active_mask']^=8
        self.assertEqual(diagnostics(req,req['matrices'])['score'],dict(weak=0,strict=0,total=0))

    def row(self):
        req=request(trials=0)
        score=diagnostics(req,req['matrices'])['score']
        return dict(label='accounting-probe',omitted=-1,request=req,
                    report=dict(matrices=deepcopy(req['matrices']),initial=score,best=score,
                                seed=req['seed'],evaluated=0,accepted=0))

    def test_exact_zero_trial_accounting(self):
        audit_report(self.row())

    def test_forged_best_score(self):
        row=self.row();row['report']['best']=dict(weak=0,strict=0,total=0)
        with self.assertRaisesRegex(ValueError,'best native score'):
            audit_report(row)

    def test_wrong_rng_receipt(self):
        row=self.row();row['report']['seed']+=1
        with self.assertRaisesRegex(ValueError,'RNG'):
            audit_report(row)

    def test_impossible_trial_count(self):
        row=self.row();row['report']['accepted']=1
        with self.assertRaisesRegex(ValueError,'trial accounting'):
            audit_report(row)

    def test_failed_inequalities_account_for_every_weak_deficit(self):
        for reverse in (False,True):
            req=request(3,reverse,target=1 if reverse else 256)
            result=diagnostics(req,req['matrices'])
            self.assertEqual(sum(x['deficit'] for x in result['failures']),result['score']['weak'])


if __name__=='__main__':unittest.main()
