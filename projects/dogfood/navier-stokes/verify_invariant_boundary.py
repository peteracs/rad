"""Compare RAD boundary arithmetic against the full, untruncated Fourier jet."""
import json
from pathlib import Path
import subprocess
import sympy as s
from build_invariant_boundary import boundary

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
row = boundary([1,1,1,1,1,0])
nu = s.Symbol('nu', positive=True)
expr = lambda name: s.sympify(row[name], locals={'nu': nu})
run = subprocess.run([str(ROOT/'target/debug/rad.exe'), str(HERE/'invariant_boundary.rad'),
                      '--strict-types', '--deny-warnings'], capture_output=True, text=True)
assert run.returncode == 0, run.stdout + run.stderr
records = [json.loads(line) for line in run.stdout.splitlines() if line.startswith('{')]
assert [r['n'] for r in records] == [30,32,48,64,96]
for r in records:
    value = s.Rational(1,r['n'])
    checks = {
        'Bdot': expr('Sdot')-2*nu*expr('Ddot'),
        'Qdot': expr('shape_rate'),
        'frequency': expr('R')/expr('K')+2*nu*expr('E')**2/expr('K')**2,
        'Eddot': expr('Rdot'),
    }
    for key, expected in checks.items():
        assert s.Rational(r[key+'_n'],r[key+'_d']) == expected.subs(nu,value), (key,r)
    assert s.Rational(r['B_n'], r['n']) == (expr('S')-2*nu*expr('D')).subs(nu,value)
assert expr('generated_enstrophy_acceleration') == s.Rational(113,4)
assert row['derivative_modes'] == 30
print('PASS: five RAD rational states agree with full Fourier boundary derivatives; 30 derivative modes included.')
