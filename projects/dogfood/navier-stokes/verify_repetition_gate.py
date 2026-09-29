"""Independent scalar quantifier elimination; no Navier-Stokes map is certified."""
from fractions import Fraction as F
from pathlib import Path
import json
import subprocess

HERE = Path(__file__).resolve().parent

def main():
    command = [str(HERE.parents[2]/'target/debug/rad.exe'),str(HERE/'repetition_gate.rad'),
               '--experimental-laws','--strict-types','--deny-warnings']
    run = subprocess.run(command,capture_output=True,text=True)
    assert run.returncode == 0,run.stdout+run.stderr
    rows = [json.loads(line[6:]) for line in run.stdout.splitlines() if line.startswith('audit ')]
    assert len(rows) == 80
    for record in rows:
        theta,rho,defect = F(record['theta'],64),F(record['rho'],4),F(record['defect'],64)
        row = record['evidence']
        feasible = rho < 1 and defect < (1-rho)*(theta-F(1,4))
        assert row['scalar_design_feasible'] == feasible
        if feasible:
            lower,upper = defect/(1-rho),theta-F(1,4)
            radius = F(row['error_radius_n'],row['error_radius_d'])
            retention = F(row['energy_retention_n'],row['energy_retention_d'])
            growth = F(row['enstrophy_factor_n'],row['enstrophy_factor_d'])
            assert radius == (lower+upper)/2 and lower < radius < upper
            assert rho*radius+defect < radius
            assert retention == theta-radius and growth == 4*retention > 1
        assert not row['actual_solution_map_proved'] and not row['blowup_proved']
    row = json.loads(next(line[5:] for line in run.stdout.splitlines() if line.startswith('gate ')))
    assert F(row['error_radius_n'],row['error_radius_d']) == F(3,16)
    assert F(row['energy_retention_n'],row['energy_retention_d']) == F(5,16)
    assert F(row['enstrophy_factor_n'],row['enstrophy_factor_d']) == F(5,4)
    assert F(1,1-F(1,4)) == F(4,3)
    for q in range(50):
        assert 4**q*F(5,16)**q == F(5,4)**q
    assert 'resolver `CheckReturnDesign`' in run.stdout and 'law `SubmitReturnDesign`' in run.stdout
    bad = subprocess.run(command+['--','forge-map'],capture_output=True,text=True)
    assert bad.returncode != 0 and 'forged solution-map proof' in bad.stdout+bad.stderr
    print('PASS: 80 exact return-design gates, radius elimination, growth/error margins, geometric identities and WHY provenance.')
    print('PASS: scalar feasibility cannot be promoted to a solution-map or blowup proof. Actual PDE inclusion remains unproved.')

if __name__ == '__main__':
    main()
