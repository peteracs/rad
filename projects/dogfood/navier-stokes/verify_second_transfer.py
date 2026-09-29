"""Independent actual second Taylor coefficient and the analytic-bound arithmetic."""
from fractions import Fraction as F
from pathlib import Path
import json
import subprocess
import sympy as s
from verify_coupled_modes import nonlinear
from verify_construction_why import ZERO

HERE = Path(__file__).resolve().parent

def mixed(a,b):
    joined = {k:a.get(k,ZERO)+b.get(k,ZERO) for k in set(a)|set(b)}
    nj,na,nb = nonlinear(joined),nonlinear(a),nonlinear(b)
    return {k:(nj.get(k,ZERO)-na.get(k,ZERO)-nb.get(k,ZERO)).applyfunc(s.simplify)
            for k in set(nj)|set(na)|set(nb)}

def main():
    waves = [(1,0,0),(0,1,0),(1,1,0),(0,0,1),(0,1,1)]
    polar = [(0,1,1),(1,0,1),(1,-1,1),(1,1,0),(1,1,-1)]
    v = {tuple(sign*x for x in k):(s.Integer(32) if i not in [2,4] else -sign*32*s.I)*s.Matrix(p)
         for i,(k,p) in enumerate(zip(waves,polar)) for sign in [-1,1]}
    nv = nonlinear(v)
    f1 = {k:x-sum(t*t for t in k)*v.get(k,ZERO) for k,x in nv.items()}
    f1 = {k:x for k,x in f1.items() if x!=ZERO}
    assert max(sum(x*x for x in k) for k in f1) == 6
    full_mixed = mixed(v,f1)
    f2 = {k:full_mixed.get(k,ZERO)-sum(t*t for t in k)*f1.get(k,ZERO)
          for k in set(full_mixed)|set(f1)}
    q2 = {k:x/2 for k,x in f2.items() if sum(t*t for t in k)>6 and x!=ZERO}
    r = {k:x/2048 for k,x in nv.items() if sum(t*t for t in k)>2 and x!=ZERO}
    direct = mixed(v,r)
    second_direct = {k:1024*x for k,x in direct.items() if sum(t*t for t in k)>6 and x!=ZERO}
    assert q2 == second_direct and len(q2) == 32
    assert max(sum(x*x for x in k) for k in q2) == 14
    assert q2[(1,3,2)] == 65536*s.I*s.Matrix([-1,1,-1])
    norm11 = sum((1+sum(t*t for t in k))**11*s.conjugate(x).dot(x) for k,x in v.items())
    assert norm11 == 537585*4096 < 65536**2
    T,root,B = F(1,2**98),F(1,2**49),2**17
    assert 65536+2**31*T+2**15*root*B**2 < B
    assert 2*2**15*root*B < F(1,2)
    assert 2*81*4 < 2**10 and 81*(4*256+4*27) < 2**17
    assert B+2**11*B**2+2**15 < 2**46
    assert 2**46+2*2**9*B*2**46+2**23 < 2**74
    assert 2**74+2*2**7*B*2**74+2*2**7*2**92+2**28 < 2**102
    assert F(2**102,2**16)*T == F(1,2**12)
    assert 2**16-2**102*T >= 2**15
    assert 2**30*T**4 == F(1,2**362)
    # Uniform force exponents, verified by completed squares for all integers.
    q = s.symbols('q',integer=True)
    for base,d,upper in [(1,11,31),(3,7,15),(14,6,23),(22,5,28)]:
        assert s.expand(4*(upper-(base-q*q+d*q))-(2*q-d)**2) == 4*(upper-base)-d*d
        assert 4*(upper-base)-d*d >= -(d%2)
    command = [str(HERE.parents[2]/'target/debug/rad.exe'),str(HERE/'second_transfer.rad'),
               '--experimental-laws','--strict-types','--deny-warnings']
    run = subprocess.run(command,capture_output=True,text=True)
    assert run.returncode == 0,run.stdout+run.stderr
    actual = {}
    for line in run.stdout.splitlines():
        if line.startswith('mode '):
            mode = json.loads(line[5:]); k=tuple(mode['k'])
            assert k not in actual
            actual[k] = 65536*s.Matrix([s.Rational(re,mode['denominator'])+s.I*s.Rational(im,mode['denominator'])
                                        for re,im in zip(mode['re'],mode['im'])])
    assert actual == q2
    row = json.loads(next(line[7:] for line in run.stdout.splitlines() if line.startswith('second ')))
    assert row['mode_count'] == 32 and row['normalized_profile_error_negative_power_two'] == 12
    assert row['actual_second_transfer_by_written_lemmas'] and not row['repeated_scale_closure']
    assert 'resolver `CheckSecondTransfer`' in run.stdout and 'law `SubmitSecondTransfer`' in run.stdout
    for mutation in ['forge-repeat','forge-error']:
        bad = subprocess.run(command+['--',mutation],capture_output=True,text=True)
        assert bad.returncode != 0 and 'forged second-transfer closure' in bad.stdout+bad.stderr
    print('PASS: all 32 coefficients match the complete actual second Taylor derivative and the coupled source.')
    print('PASS: H11 existence, force derivatives, normalized remainder, positive second-band energy and WHY rejection checks.')
    print('Second finite transfer proved by written analytic lemmas; no scale-uniform repetition theorem.')

if __name__ == '__main__':
    main()
