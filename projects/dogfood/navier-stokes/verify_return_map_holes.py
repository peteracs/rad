"""Independent inverse-metric calculation and proof-hole integrity checks."""
from fractions import Fraction as F
from pathlib import Path
import json
import subprocess
import sympy as s

HERE = Path(__file__).resolve().parent

def main():
    a,b = s.symbols('a b',real=True)
    G = s.Matrix([-s.Rational(3,2)*a-s.Rational(39,20)*a*b+b*b/2304,
                  2048*a*a-s.Rational(485,117)*b-s.Rational(160,351)*a*b])
    J = G.jacobian([a,b])
    q = s.Rational(117,5)
    p = s.simplify(4096*q/s.Rational(39,20))
    assert p == 49152
    P = s.diag(p,q)
    symmetric = (P*J+J.T*P).applyfunc(s.expand)
    assert symmetric[0,1] == symmetric[1,0] == 32*b
    assert symmetric[0,0] == 2*p*(-s.Rational(3,2)-s.Rational(39,20)*b)
    assert symmetric[1,1] == 2*q*(-s.Rational(485,117)-s.Rational(160,351)*a)
    # The written argument bounds the whole rectangle by this fixed quadratic form.
    comparison = s.Matrix([[-p,2],[2,-6*q]])
    assert comparison[0,0] < 0 and comparison.det() > 0
    h = s.Matrix([1,1])
    assert (h.T*(J+J.T).subs({a:1,b:0})*h)[0] > 0
    assert p+q < 256**2
    assert F(2**69,2) == 2**68
    T = F(1,2**98)
    assert 5*T < F(1,4) and 4000*T < F(1,16)
    # Upper bounds for the forced reference ODE on the rectangle.
    assert F(3,2)*F(5,4)+F(39,20)*F(5,4)/16+F(1,256*2304)+1 < 5
    assert 2048*F(5,4)**2+F(485,117)/16+F(160,351)*F(5,4)/16 < 4000
    command = [str(HERE.parents[2]/'target/debug/rad.exe'),str(HERE/'return_map_holes.rad'),
               '--experimental-laws','--strict-types','--deny-warnings']
    run = subprocess.run(command,capture_output=True,text=True)
    assert run.returncode == 0,run.stdout+run.stderr
    row = json.loads(next(line[6:] for line in run.stdout.splitlines() if line.startswith('holes ')))
    assert row['metric_donor_weight'] == p
    assert F(row['metric_receiver_weight_n'],row['metric_receiver_weight_d']) == q
    assert row['projected_decay_rate'] == 1 and not row['full_PDE_return_map_proved']
    assert {hole['symbol'] for hole in row['open_bounds']} == {'Gamma','eta','kappa','zeta','theta0_and_L','all_stage_inclusion'}
    assert all(hole['proof_status'] == 'unproved' for hole in row['open_bounds'])
    assert 'resolver `CheckMapHoles`' in run.stdout and 'law `SubmitMapHoles`' in run.stdout
    for mutation in ['forge-tail','forge-map']:
        bad = subprocess.run(command+['--',mutation],capture_output=True,text=True)
        assert bad.returncode != 0 and 'unproved return bound promoted to proof' in bad.stdout+bad.stderr
    print('PASS: inverse metric, symbolic Jacobian and uniform contraction comparison, finite remainder and reference-ODE bounds.')
    print('PASS: WHY exposes six named proof holes; forged tail/map proof rejected. Full Navier-Stokes return map remains unproved.')

if __name__ == '__main__':
    main()
