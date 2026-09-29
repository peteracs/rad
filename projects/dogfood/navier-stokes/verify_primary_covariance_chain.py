"""Check universal polynomial certificates, isolated forks, replay, and mutations.

The analytic comparison/integration proof is in PRIMARY_COVARIANCE_CHAIN.md.
This runner does not claim to check real analysis or execute Lean.
"""
import hashlib
import itertools
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
SOURCE = Path('D:/Downloads/NavierStokesAndEuler-main/NavierStokes')
reports = {}
for name, rejected in [('growing_cone', 'modal cone boundary identity'),
                       ('covariance_margins', 'determinant perturbation bound')]:
    command = [RAD, str(HERE/(name+'.rad')), '--strict-types', '--deny-warnings', '--experimental-laws']
    one = subprocess.run(command, capture_output=True, text=True, check=True,
                         env={**os.environ, 'RAYON_NUM_THREADS':'1'})
    with tempfile.TemporaryDirectory(prefix='rad-primary-chain-') as directory:
        trace = str(Path(directory)/'trace.radtrace')
        four = subprocess.run(command+['--record',trace], capture_output=True, text=True, check=True,
                              env={**os.environ, 'RAYON_NUM_THREADS':'4'})
        assert one.stdout == four.stdout
        replay = subprocess.run([RAD,'replay',trace], capture_output=True, text=True, check=True)
        assert 'Replay verified: world digest matches the recorded run' in replay.stderr
    bad = subprocess.run(command+['--','mutate'], capture_output=True, text=True)
    assert bad.returncode != 0 and rejected in bad.stdout+bad.stderr
    reports[name] = json.loads(one.stdout.splitlines()[0])
    reports[name].update(deterministic_workers=[1,4], record_replay=True, mutation_rejected=True)
    (HERE/(name+'.why.txt')).write_text(one.stdout, encoding='utf-8')

p,r,lam,d,e11,e12,e21,e22,eps,C = s.symbols('p r lam d e11 e12 e21 e22 eps C')
for edge in (-1,1):
    q = edge*r*p
    dp = (lam-d+e11)*p+e12*q
    dq = e21*p+(-lam-d+e22)*q
    factor = edge*e21+r*(e22-e11)-edge*r*r*e12-2*lam*r
    assert s.expand(2*q*dq-2*r*r*p*dp-2*r*p*p*factor) == 0
    for signs in itertools.product((-1,1),repeat=4):
        noise = factor+2*lam*r
        noise = noise.subs(dict(zip((e11,e12,e21,e22),(eps*k for k in signs))))
        assert all(c>=0 for c in s.Poly(s.expand(eps*(1+r)**2-noise),eps,r).coeffs())
assert s.expand(4*(8*(C+1)-C*(1+r)**2)-(23*C+32)-C*(1-2*r)*(5+2*r)) == 0
M,rho = s.symbols('M rho')
for signs in itertools.product((-1,1),repeat=8):
    base = s.Matrix(2,2,[M*signs[2*j] for j in range(4)])
    error = s.Matrix(2,2,[rho*signs[2*j+1] for j in range(4)])
    delta = (base+error).det()-base.det()
    for direction in (-1,1):
        assert all(c>=0 for c in s.Poly(s.expand(4*M*rho+2*rho**2-direction*delta),M,rho).coeffs())

# Exact inverse-matrix jet recursion, independently checked at symbolic order 2.
t = s.Symbol('t')
H = s.Matrix(2,2,[s.Function('h'+str(i))(t) for i in range(4)])
U = H.inv()
assert all(s.cancel(z)==0 for z in U.diff(t)+U*H.diff(t)*U)
expected_second = 2*U*H.diff(t)*U*H.diff(t)*U-U*H.diff(t,2)*U
assert all(s.cancel(z)==0 for z in U.diff(t,2)-expected_second)
reports.update(independent_symbolic_checks=True, inverse_matrix_derivatives_checked=[1,2],
               lean_executed=False, full_navier_stokes_proof=False)
reports['source_sha256'] = {name:hashlib.sha256((SOURCE/name).read_bytes()).hexdigest() for name in [
    'GrowingMode.lean','PulseCovariance.lean','PrimaryCovarianceBounds.lean',
    'SignedCopyBounds.lean','PrimaryPulseBounds.lean']}
(HERE/'primary_covariance_chain.json').write_text(json.dumps(reports,indent=2)+'\n')
print(json.dumps({k:v for k,v in reports.items() if k!='source_sha256'}))
