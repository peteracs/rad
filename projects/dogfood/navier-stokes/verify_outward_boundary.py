"""Independent exact full-Fourier verification of failure of the proposed region."""
import json
from pathlib import Path
import subprocess
import sympy as s
from build_amplification import calculate

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
amps, nu, modes, derivative, E, R, D = calculate()
inner = lambda x,y: s.re(s.conjugate(x).dot(y))
K = s.expand(sum(inner(u,u)/2 for u in modes.values()))
Kdot = s.expand(sum(inner(u,derivative[k]) for k,u in modes.items()))
Ddot = s.expand(2*sum(sum(x*x for x in k)**2*inner(u,derivative[k]) for k,u in modes.items()))
Qdot = (Kdot*D+K*Ddot)/E**2-2*K*D*R/E**3
run = subprocess.run([str(ROOT/'target/debug/rad.exe'), str(HERE/'spectral_rate.rad'),
    '--strict-types','--deny-warnings','--','outward-boundary'], capture_output=True,text=True)
assert run.returncode == 0, run.stdout+run.stderr
rows = [json.loads(line) for line in run.stdout.splitlines() if line.startswith('{')]
assert [r['amplitude'] for r in rows] == [24,30]
for row in rows:
    a = row['amplitude']
    values = dict(zip(amps,[a,a,a//2,a,a//2,0])) | {nu:1}
    for key,expr in dict(k8=8*K,e8=8*E,d4=4*D,kdot80=80*Kdot,rate8=8*R,ddot4=4*Ddot).items():
        assert row[key] == expr.subs(values), (key,row)
    assert s.Rational(row['Qdot_n'],row['Qdot_d']) == Qdot.subs(values) > 0
    assert s.simplify((K*D/E**2).subs(values)) == s.Rational(20,9)
    assert (R-nu*D).subs(values) >= 0

# Original candidate's fixed nu=1/30, K0=3: strict production interior.
values = dict(zip(amps,[1,1,s.Rational(1,2),1,s.Rational(1,2),0])) | {nu:s.Rational(1,30)}
assert K.subs(values) == s.Rational(15,8) < 3
assert (R-nu*D).subs(values) == s.Rational(1,10)
assert Qdot.subs(values) == s.Rational(16,135)
t = s.Symbol('t',real=True)
q = (K*D/E**2).subs(dict(zip(amps,[1,1,t,1,t,0])))
assert s.diff(q,t).subs(t,s.Rational(1,2)) == s.Rational(8,27)
print('PASS: direct RAD convolution and independent Fourier algebra agree on two outward witnesses.')
print('At nu=1/30: K=15/8<3, B=1/10>0, Q=20/9, Qdot=16/135>0.')
print('Nonzero shape derivative 8/27 supports the written implicit-function tail extension.')
