"""Exact Fourier and bound checks; the continuum proof is EVOLVING_TRANSFER.md."""
from fractions import Fraction as F
from pathlib import Path
import json
import subprocess
import sympy as s
from verify_construction_why import force

HERE = Path(__file__).resolve().parent

def main():
    waves = [(1,0,0),(0,1,0),(1,1,0),(0,0,1),(0,1,1)]
    polar = [(0,1,1),(1,0,1),(1,-1,1),(1,1,0),(1,1,-1)]
    u = {tuple(sign*x for x in k):(s.Integer(32) if i not in [2,4] else -sign*32*s.I)*s.Matrix(p)
         for i,(k,p) in enumerate(zip(waves,polar)) for sign in [-1,1]}
    target = (2,1,0)
    assert target not in u and max(sum(x*x for x in k) for k in u) == 2
    source = -force(u,{})[target]
    assert source == s.Matrix([-s.Rational(1024,5),s.Rational(2048,5),-2048])
    gradient_square = sum(sum(x*x for x in k)*s.conjugate(v).dot(v) for k,v in u.items())
    assert gradient_square == 192**2
    t = s.symbols('t',real=True)
    profile = source*(1-s.exp(-5*t))/5
    assert all(s.simplify(x)==0 for x in s.diff(profile,t)+5*profile-source)
    assert profile.subs(t,0) == s.zeros(3,1)

    T,delta = F(1,2**98),F(1,2**18)
    source_error = 3*4096*delta
    assert source_error == F(3,64) < 1
    assert 5*T < F(1,4)
    assert 2047*F(3,4) > 1024
    assert 2047-5*2049*T > 1024
    assert 191**2 > 2048
    assert 128*4096 == 2**19
    assert 4352*F(1,2**49) <= 2**29
    assert 2**49*T == F(1,2**49)
    assert 1024**2*T**2 == F(1,2**176)
    command = [str(HERE.parents[2]/'target/debug/rad.exe'),str(HERE/'evolving_transfer.rad'),
               '--experimental-laws','--strict-types','--deny-warnings']
    run = subprocess.run(command,capture_output=True,text=True)
    assert run.returncode == 0,run.stdout+run.stderr
    row = json.loads(next(line[9:] for line in run.stdout.splitlines() if line.startswith('transfer ')))
    assert row['target'] == list(target) and row['target_z_source'] == -2048
    assert row['total_energy_decreases'] and row['nonlinear_transfer_proved_by_written_lemmas']
    assert not row['repeated_scale_transfer_proved']
    assert 'resolver `CheckTransfer`' in run.stdout and 'law `SubmitTransfer`' in run.stdout
    for mutation in ['forge-repeat','forge-source']:
        bad = subprocess.run(command+['--',mutation],capture_output=True,text=True)
        assert bad.returncode != 0 and 'forged evolving-transfer certificate' in bad.stdout+bad.stderr
    print('PASS: independent generated-mode source, exact evolving heat profile, modal/all-mode errors and energy margins.')
    print('PASS: WHY provenance and forged source/repeatability rejection. Continuum lemmas are documented, not RAD-formalized.')

if __name__ == '__main__':
    main()
