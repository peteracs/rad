"""Independent receiver algebra and exact constants for the written PDE proof."""
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
    seed = {tuple(sign*x for x in k):(s.Integer(32) if i not in [2,4] else -sign*32*s.I)*s.Matrix(p)
            for i,(k,p) in enumerate(zip(waves,polar)) for sign in [-1,1]}
    drift = force(seed,{})
    receiver = {k:-v/2048 for k,v in drift.items() if sum(x*x for x in k)>2 and v != s.zeros(3,1)}
    assert len(receiver) == 16 and receiver[(2,1,0)][2] == -1
    receiver_drift = force(receiver,{})
    loss = sum(sum(x*x for x in k)**2*s.conjugate(v).dot(v) for k,v in receiver.items())
    rate = sum(sum(x*x for x in k)*s.re(s.conjugate(v).dot(-receiver_drift[k])) for k,v in receiver.items())
    assert s.simplify(loss) == 443 and s.simplify(rate) == -450
    assert s.simplify(rate+loss) == -7
    for k,v in receiver.items():
        assert s.Matrix(k).dot(v) == 0
        assert receiver[tuple(-x for x in k)] == s.conjugate(v)
        assert all((60*x).is_Integer for x in list(s.re(v))+list(s.im(v)))
    norm7 = sum((1+sum(x*x for x in k))**7*s.conjugate(v).dot(v) for k,v in seed.items())
    assert norm7 == 6945*4096 < 8192**2
    # All-integer force exponent bound from a completed square.
    q = s.symbols('q',integer=True)
    assert s.expand(4*(15-(3-q*q+7*q))-((2*q-7)**2-1)) == 0
    # 2q-7 is odd, so its square is at least 1.
    T,root = F(1,2**98),F(1,2**49)
    B = 16384
    assert root**2 == T
    assert 8192+32768*T+2048*root*B**2 < B
    assert 2*2048*root*B < F(1,2)
    assert B+256*B**2+32768 < 2**37
    assert 128*32768*2**37/2 == 2**58
    assert 512*8192**2 == 2**35
    assert 2**58+2**34 <= 2**59
    assert F(2**59,2048)*T == F(1,2**50)
    command = [str(HERE.parents[2]/'target/debug/rad.exe'),str(HERE/'receiver_profile.rad'),
               '--experimental-laws','--strict-types','--deny-warnings']
    run = subprocess.run(command,capture_output=True,text=True)
    assert run.returncode == 0,run.stdout+run.stderr
    row = json.loads(next(line[9:] for line in run.stdout.splitlines() if line.startswith('receiver ')))
    assert row['receiver_modes'] == 16 and row['receiver_self_production'] == -7
    assert row['receiver_standalone_rate'] == -450 and row['receiver_dissipation'] == 443
    assert row['normalized_profile_proved_by_written_lemmas'] and not row['repeatable_cascade_proved']
    assert 'resolver `CheckReceiver`' in run.stdout and 'law `SubmitReceiver`' in run.stdout
    for mutation in ['forge-production','forge-repeat']:
        bad = subprocess.run(command+['--',mutation],capture_output=True,text=True)
        assert bad.returncode != 0 and 'forged receiver repeatability' in bad.stdout+bad.stderr
    print('PASS: independent 16-mode receiver, negative standalone nonlinear production, H7 existence and O(t^2) profile-error arithmetic.')
    print('PASS: WHY provenance and forged production/repetition rejection. Whole receiver profile certified by written PDE lemmas, not a repeatable cascade.')

if __name__ == '__main__':
    main()
