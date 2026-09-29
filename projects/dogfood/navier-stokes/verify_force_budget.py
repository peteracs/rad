"""Finite RAD arithmetic checks plus symbolic identity behind the written bound."""
import json
from pathlib import Path
import subprocess
import sympy as s

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
run = subprocess.run([str(ROOT/'target/debug/rad.exe'), str(HERE/'force_budget.rad'),
    '--strict-types','--deny-warnings'], capture_output=True,text=True)
assert run.returncode == 0,run.stdout+run.stderr
rows = [json.loads(line) for line in run.stdout.splitlines() if line.startswith('{')]
assert len(rows) == 1
assert rows[0] == dict(checks=845,finite_decay_order=40,uncontrolled_spatial_order=14,
    core_exposure_exponent=-2,carrier_exposure_exponent=2,
    actual_force_bound_supplied=False,repeatable_transfer_proved=False,blowup_proved=False)
d,h = s.symbols('d h',nonnegative=True,integer=True)
q=d+1+h
assert s.expand(q*q-d*q-q-q*h) == 0
Q = s.symbols('Q',integer=True,positive=True)
j = s.symbols('j',integer=True,nonnegative=True)
assert s.simplify(s.summation(2**(-Q-j),(j,0,s.oo)) - 2**(1-Q)) == 0
print('PASS: 845 finite RAD budget checks; symbolic majorant identity and geometric tail sum.')
print('Conditional force-smoothness budget only: actual residual estimates and transfer remain missing.')
