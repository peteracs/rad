"""Independent polarization and projection checks for the coupled PDE argument."""
from fractions import Fraction as F
from pathlib import Path
import json
import subprocess
import sympy as s
from verify_construction_why import force,ZERO

HERE = Path(__file__).resolve().parent

def nonlinear(u):
    return {k:-x+sum(t*t for t in k)*u.get(k,ZERO) for k,x in force(u,{}).items()}

def pair(u,v):
    return s.simplify(sum(s.re(s.conjugate(x).dot(v.get(k,ZERO))) for k,x in u.items()))

def main():
    waves = [(1,0,0),(0,1,0),(1,1,0),(0,0,1),(0,1,1)]
    polar = [(0,1,1),(1,0,1),(1,-1,1),(1,1,0),(1,1,-1)]
    v = {tuple(sign*x for x in k):(s.Integer(32) if i not in [2,4] else -sign*32*s.I)*s.Matrix(p)
         for i,(k,p) in enumerate(zip(waves,polar)) for sign in [-1,1]}
    nv = nonlinear(v)
    r = {k:x/2048 for k,x in nv.items() if sum(t*t for t in k)>2 and x!=ZERO}
    nr = nonlinear(r)
    combined = {k:v.get(k,ZERO)+r.get(k,ZERO) for k in set(v)|set(r)}
    nt = nonlinear(combined)
    mixed = {k:nt.get(k,ZERO)-nv.get(k,ZERO)-nr.get(k,ZERO) for k in set(nt)|set(nv)|set(nr)}
    V,R,C,D = pair(v,v),pair(r,r),pair(r,nv),pair(v,nr)
    assert (V,R,C,D) == (24576,s.Rational(117,5),s.Rational(239616,5),s.Rational(32,3))
    assert pair(v,nv) == pair(r,nr) == 0
    assert pair(v,mixed) == -C and pair(r,mixed) == -D
    assert s.simplify(C/V) == s.Rational(39,20)
    assert s.simplify(D/V) == s.Rational(1,2304)
    assert s.simplify(C/R) == 2048 and s.simplify(D/R) == s.Rational(160,351)
    grad_v = sum(sum(t*t for t in k)*s.conjugate(x).dot(x) for k,x in v.items())
    grad_r = sum(sum(t*t for t in k)*s.conjugate(x).dot(x) for k,x in r.items())
    assert grad_v/V == s.Rational(3,2) and grad_r/R == s.Rational(485,117)
    a,b = s.symbols('a b',real=True)
    da = -C/V*a*b+D/V*b*b
    db = C/R*a*a-D/R*a*b
    assert s.expand(V*a*da+R*b*db) == 0
    assert s.expand(R*b*db-a*b*(C*a-D*b)) == 0
    omitted = (1,0,1)
    assert omitted not in v and omitted not in r
    assert nv[omitted] == s.Matrix([0,-2048*s.I,0])
    T = F(1,2**98)
    assert 2**37*T < F(1,16)
    assert 8+216*32768 < 2**24
    remainder = 2**61*T
    assert remainder == F(1,2**37)
    assert 2048*F(3,4)**2-F(485,117)/16-F(160,351)*F(5,4)/16-remainder > 1024
    assert -F(9,8)+1+F(1,256*2304)+remainder < 0
    command = [str(HERE.parents[2]/'target/debug/rad.exe'),str(HERE/'coupled_modes.rad'),
               '--experimental-laws','--strict-types','--deny-warnings']
    run = subprocess.run(command,capture_output=True,text=True)
    assert run.returncode == 0,run.stdout+run.stderr
    row = json.loads(next(line[8:] for line in run.stdout.splitlines() if line.startswith('coupled ')))
    assert F(row['donor_to_receiver_pair_n'],row['donor_to_receiver_pair_d']) == C
    assert F(row['receiver_to_donor_pair_n'],row['receiver_to_donor_pair_d']) == D
    assert row['exact_projection_with_remainders'] and row['donor_amplitude_decreases']
    assert not row['finite_mode_invariance'] and not row['repeatable_cascade_proved']
    assert 'resolver `CheckCoupled`' in run.stdout and 'law `SubmitCoupled`' in run.stdout
    for mutation in ['forge-truncation','forge-repeat']:
        bad = subprocess.run(command+['--',mutation],capture_output=True,text=True)
        assert bad.returncode != 0 and 'forged coupled closure' in bad.stdout+bad.stderr
    print('PASS: full polarized coupling, exact energy exchange, projection coefficients and nonzero omitted-mode witness.')
    print('PASS: all-mode remainder/growth arithmetic, WHY provenance, and forged truncation/repetition rejection.')
    print('Finite coupled evolution uses the written PDE lemmas; no repeatable cascade established.')

if __name__ == '__main__':
    main()
