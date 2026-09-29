"""Independent Fourier algebra and exact arithmetic for the written segment lemma.

This does not formalize the infinite-dimensional analytic lemmas in RAD.
"""
from fractions import Fraction as F
import json
import subprocess
from pathlib import Path
import sympy as s
from verify_construction_why import force

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')

def main():
    waves = [(1,0,0),(0,1,0),(1,1,0),(0,0,1),(0,1,1)]
    polar = [(0,1,1),(1,0,1),(1,-1,1),(1,1,0),(1,1,-1)]
    u = {}
    for i,(k,p) in enumerate(zip(waves,polar)):
        for polarity in [-1,1]:
            coefficient = -polarity*32*s.I if i in [2,4] else s.Integer(32)
            u[tuple(polarity*x for x in k)] = coefficient*s.Matrix(p)
    drift = force(u,{})
    energy_rate = sum(sum(x*x for x in k)*s.re(s.conjugate(v).dot(-drift[k])) for k,v in u.items())
    dissipation = sum(sum(x*x for x in k)**2*s.conjugate(v).dot(v) for k,v in u.items())
    assert energy_rate == 200704 and dissipation == 61440
    assert energy_rate+dissipation == 262144
    for order,expected,bound in [(3,105*4096,1024),(5,825*4096,4096)]:
        square = sum((1+sum(x*x for x in k))**order*s.conjugate(v).dot(v) for k,v in u.items())
        assert square == expected and square < bound**2

    tau,root_upper,unit = F(1,2**99),F(1,2**49),F(1,2**19)
    assert tau <= root_upper**2
    B = 2048
    bilinear = 128*root_upper
    for entry_units in [0,1]:
        linear = 1024+entry_units*unit+256*tau
        assert linear+bilinear*B**2 < B
        assert 2*bilinear*B < F(1,2)
        endpoint_radius = entry_units*unit+4352*tau+bilinear*B**2
        assert endpoint_radius < (entry_units+1)*unit
    delta = 2*unit
    production_error = 24*B**2*delta
    loss_error = 2*B*delta
    assert production_error == 384 and loss_error < 1
    assert 262144-production_error-61440-loss_error-B > 2**17
    assert 2**17*tau == F(1,2**82)
    assert 2*tau < F(1,128) and 128*F(1,2**60) < 1
    # Full Fourier nonlinear output has modes outside the seed support.
    assert any(k not in u and v != s.zeros(3,1) for k,v in drift.items())

    command = [RAD,str(HERE/'solution_segment.rad'),'--experimental-laws','--strict-types','--deny-warnings']
    result = subprocess.run(command,capture_output=True,text=True)
    assert result.returncode == 0,result.stdout+result.stderr
    row = json.loads(next(line[8:] for line in result.stdout.splitlines() if line.startswith('segment ')))
    assert row['stages'] == 2 and row['endpoint_radius_units'] == 2
    assert row['nonlinear_lower_bound'] == 262144-production_error
    assert row['total_rate_lower_bound'] == 2**17
    assert row['analytic_lemmas_required'] and not row['same_region_closed'] and not row['blowup_proved']
    assert 'resolver `CheckSegment`' in result.stdout and 'law `SubmitSegment`' in result.stdout
    for mutation in ['forge-reset','forge-gain']:
        rejected = subprocess.run(command+['--',mutation],capture_output=True,text=True)
        assert rejected.returncode != 0 and 'forged segment closure certificate' in rejected.stdout+rejected.stderr
    print('PASS: independent full Fourier production, H3/H5 norms, two mild-solution tube steps, gain bounds and WHY provenance.')
    print('PASS: forged same-region closure and overstated gain rejected. Analytic PDE lemmas remain outside the RAD kernel.')

if __name__ == '__main__':
    main()
