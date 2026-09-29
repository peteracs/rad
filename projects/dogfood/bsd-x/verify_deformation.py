"""Independent deformation algebra checks. No arithmetic binding is assumed."""
from fractions import Fraction
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'deformation_pairing.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-derived-pairing-') as directory:
    trace = str(Path(directory) / 'deformation.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0 and 'unsupported arithmetic vanishing identification' in bad.stdout + bad.stderr


def determinant(matrix):
    a = [[Fraction(value) for value in row] for row in matrix]
    product = Fraction(1)
    for col in range(3):
        pivot = next((i for i in range(col, 3) if a[i][col]), None)
        if pivot is None:
            return 0
        if pivot != col:
            a[pivot], a[col] = a[col], a[pivot]
            product = -product
        product *= a[col][col]
        for i in range(col + 1, 3):
            factor = a[i][col] / a[col][col]
            a[i] = [x-factor*y for x, y in zip(a[i], a[col])]
    return product


def parse(prefix):
    return [json.loads(line[len(prefix):]) for line in one.stdout.splitlines() if line.startswith(prefix)]


requests, cases, witnesses = parse('request '), parse('case '), parse('witness ')
assert len(requests) == len(cases) == 243 and len(witnesses) == 4
for d, row in zip(requests, cases):
    coefficients = row['determinant']
    assert len(coefficients) == 10
    for t in range(10):
        matrix = [[1, d['b']*t, 0],
                  [d['c']*t, d['h']*t+d['k']*t*t+d['a']*t**3, 0], [0, 0, t]]
        expected = determinant(matrix)
        assert sum(a*t**i for i, a in enumerate(coefficients)) == expected
        # Independently apply one nontrivial pair of determinant-one shears.
        matrix[1] = [x + t*y for x, y in zip(matrix[1], matrix[0])]
        for i in range(3):
            matrix[i][1] -= t*matrix[i][0]
        assert determinant(matrix) == expected
    order = next((i for i, value in enumerate(coefficients) if value), -1)
    assert row['vanishing_order'] == order and row['identically_zero'] == (order == -1)
    assert row['constant_corank'] == 2 and (order == -1 or order >= 2)
    assert row['raw_second'] == d['k']
    assert row['elimination_correction'] == -d['b']*d['c']
    assert row['effective_second'] == coefficients[3]
    assert row['first_pairing_rank'] == (1 if d['h'] == 0 else 2)
    assert row['gauge_invariant'] and not row['arithmetic_identification'] and not row['x3_proved']
assert [row['vanishing_order'] for row in witnesses] == [2, 3, 4, -1]
(HERE / 'deformation_pairing.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(synthetic_deformations=243, rad_basis_shear_checks=2187,
              exact_interpolation_checks=2430, independent_shear_checks=2430,
              worker_counts=[1, 4], record_replay=True, forged_identification_rejected=True,
              web_search_used=False, arithmetic_deformations_constructed=0,
              universal_X_proofs_found=0)
(HERE / 'deformation_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report))
