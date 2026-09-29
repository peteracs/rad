"""Independent algebra audit; deliberately no claim to verify the Euler PDF."""
import hashlib
import json
from pathlib import Path
import subprocess
import sympy as s

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PDF = Path('C:/Users/pxp12/Desktop/euler.pdf')
EXPECTED_HASH = '714dd538ed6cdb5c4d7e80e436d2a3347e6246437fc9dc7da017c44b0caa6703'
assert hashlib.sha256(PDF.read_bytes()).hexdigest() == EXPECTED_HASH

# Universal positivity, not just the 16 sampled exponents in RAD.
j, c, n = s.symbols('j c n')
margin = (j-1)**7 - 2*c*j**2*(j-1)**3 - j**2
shifted = s.Poly(s.expand(margin.subs(j, 4*(c+1)+n)), c, n)
assert all(coef > 0 for coef in shifted.coeffs())
assert shifted.eval({c: 0, n: 0}) > 0

# Exact scalar damping conjugation of the transverse principal ODE.
t, d = s.symbols('t d')
v = s.Matrix(s.symbols('v0:3'))
m = s.Matrix(s.symbols('m0:3'))
M = s.Matrix(3, 3, s.symbols('a0:9'))
L = lambda w: -M*w + 2*m*(m.dot(M*w))/m.dot(m)
factor = s.exp(-d*t)
assert s.simplify(s.diff(factor,t)*v + factor*L(v) - (L(factor*v)-d*factor*v)) == s.zeros(3,1)

command = [str(ROOT/'target/debug/rad.exe'),str(HERE/'desktop_euler_audit.rad'),
           '--strict-types','--deny-warnings','--experimental-laws']
run = subprocess.run(command,capture_output=True,text=True,check=True)
rows = [json.loads(line.removeprefix('packet ')) for line in run.stdout.splitlines() if line.startswith('packet ')]
assert len(rows) == 16
for row in rows:
    assert row['margin'] == int(margin.subs({j:row['j'],c:row['map_power']}))
    assert row['scale_identity_checked'] and not row['navier_stokes_proved']
assert 'CheckPacket' in run.stdout and 'SubmitPacket' in run.stdout
bad = subprocess.run(command+['--','forge'],capture_output=True,text=True)
assert bad.returncode != 0 and 'forged PDE conclusion' in bad.stdout+bad.stderr, bad.stdout+bad.stderr
(HERE/'desktop_euler_audit.why.txt').write_text(run.stdout,encoding='utf-8')
audit = dict(pdf_sha256=EXPECTED_HASH,rad_cases=len(rows),
    universal_positive_polynomial_terms=len(shifted.terms()),
    principal_damping_identity=True,forged_proof_rejected=True,
    euler_paper_verified=False,navier_stokes_proved=False)
(HERE/'desktop_euler_audit.json').write_text(json.dumps(audit,indent=2)+'\n',encoding='utf-8')
print(json.dumps(audit))
