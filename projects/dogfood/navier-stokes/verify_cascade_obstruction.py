"""Independent exact witnesses; written continuum lemmas remain external."""
from fractions import Fraction as F
from pathlib import Path
import json
import subprocess

HERE = Path(__file__).resolve().parent

def main():
    waves = [(1,0,0),(0,1,0),(1,1,0),(0,0,1),(0,1,1)]
    polar = [(0,1,1),(1,0,1),(1,-1,1),(1,1,0),(1,1,-1)]
    support = {tuple(sign*x for x in k) for k in waves for sign in [-1,1]}
    for scale in [2,3,4,8,64]:
        assert support.isdisjoint({tuple(scale*x for x in k) for k in support})
    l2 = sum(F(64**2,2)*sum(x*x for x in p) for p in polar)
    h3 = sum(F(64**2,2)*sum(x*x for x in p)*(1+sum(x*x for x in k))**3 for k,p in zip(waves,polar))
    assert l2 == 24576 and h3 == 430080
    assert l2 < 157**2 and 4*l2 > 312**2 and h3 > 640**2
    assert 16+F(1,1-F(1,64)) < 18
    for q in range(2,1000):
        assert (5-(q+1)**2-(q+1))-(5-q*q-q) == -2*q-2 <= -6
    assert 312-1 > 157+18
    assert 640-F(1,2**18) > 1
    assert F(1,2)*2+1 <= 2
    command = [str(HERE.parents[2]/'target/debug/rad.exe'),str(HERE/'cascade_obstruction.rad'),
               '--experimental-laws','--strict-types','--deny-warnings']
    run = subprocess.run(command,capture_output=True,text=True)
    assert run.returncode == 0,run.stdout+run.stderr
    row = json.loads(next(line[8:] for line in run.stdout.splitlines() if line.startswith('cascade ')))
    assert row['velocity_L2_upper'] == 175 and row['force_L1L2_upper'] == 18
    assert not any(row[k] for k in ['radius_reset_proved','bounded_tube_can_blow_up','doubled_periodic_seed_reachable','blowup_proved'])
    assert 'resolver `CheckCascade`' in run.stdout and 'law `SubmitCascade`' in run.stdout
    for mutation in ['forge-transfer','forge-reset']:
        bad = subprocess.run(command+['--',mutation],capture_output=True,text=True)
        assert bad.returncode != 0 and 'forged cascade implication' in bad.stdout+bad.stderr
    print('PASS: Fourier support separation, exact seed norms, global force impulse bound, energy exclusion and WHY provenance.')
    print('PASS: forged transfer/reset rejected. No repeatable cascade or blowup proved.')

if __name__ == '__main__':
    main()
