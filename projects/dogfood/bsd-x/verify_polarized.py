"""Independent exact mixed-coefficient verification; no arithmetic identity."""
from fractions import Fraction
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'polarized_comparison.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-polarized-') as directory:
    trace = str(Path(directory) / 'polarized.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0 and 'a mixed coefficient is not an arithmetic reciprocity proof' in bad.stdout + bad.stderr


def elimination(matrix):
    a = [[Fraction(x) for x in row] for row in matrix]
    n = len(a)
    rank, product = 0, Fraction(1)
    for col in range(n):
        pivot = next((i for i in range(rank, n) if a[i][col]), None)
        if pivot is None:
            continue
        if pivot != rank:
            a[pivot], a[rank] = a[rank], a[pivot]
            product = -product
        product *= a[rank][col]
        for i in range(rank+1, n):
            factor = a[i][col]/a[rank][col]
            a[i] = [x-factor*y for x,y in zip(a[i], a[rank])]
        rank += 1
    return rank, product if rank == n else Fraction(0)


def diagonal_order(a):
    # Exact Lagrange interpolation at 0,1,2,3,4 for a degree-four polynomial.
    polynomial = [Fraction(0)]*5
    for t in range(5):
        value = elimination([[a[i][j]+(t if i == j else 0) for j in range(4)] for i in range(4)])[1]
        basis, denominator = [Fraction(1)], 1
        for u in range(5):
            if u == t:
                continue
            out = [Fraction(0)]*(len(basis)+1)
            for i, x in enumerate(basis):
                out[i] -= u*x
                out[i+1] += x
            basis = out
            denominator *= t-u
        for i, x in enumerate(basis):
            polynomial[i] += value*x/denominator
    return next(i for i, x in enumerate(polynomial) if x)


def check(flat, m, row):
    a = [flat[4*i:4*i+4] for i in range(4)]
    rank, _ = elimination(a)
    assert row['lowest_mixed_degree'] == 4-rank
    assert row['diagonal_order'] == diagonal_order(a)
    assert row['target_nonvanishing'] == (rank >= 4-m)
    assert not row['analytic_reciprocity_verified']
    if row['target_nonvanishing']:
        positions = row['target_positions']
        assert len(positions) == m
        coefficient = Fraction(0)
        for mask in range(1 << m):
            b = [line[:] for line in a]
            for i, pos in enumerate(positions):
                if mask & (1 << i):
                    b[pos//4][pos%4] += 1
            coefficient += (-1)**(m-mask.bit_count())*elimination(b)[1]
        assert coefficient == row['target_coefficient'] != 0
        minor = [[a[i][j] for j in row['minor_cols']] for i in row['minor_rows']]
        assert abs(elimination(minor)[1]) == abs(coefficient)


def parse(prefix):
    return [json.loads(line[len(prefix):]) for line in one.stdout.splitlines() if line.startswith(prefix)]


requests, cases, witnesses = parse('request '), parse('case '), parse('witness ')
assert len(requests) == len(cases) == 25 and len(witnesses) == 3
for request, row in zip(requests, cases):
    check(request['matrix'], request['analytic'], row)
for flat, row in zip([[1,0,0,0,0,1,0,0,0,0,0,0,0,0,0,0],
                       [0,1,0,0,0,0,1,0,0,0,0,1,0,0,0,0], [0]*16], witnesses):
    check(flat, 2, row)
(HERE / 'polarized_comparison.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(symbolic_matrix_forks=25, diagnostic_witnesses=3,
              independent_rational_elimination_and_finite_differences=True,
              worker_counts=[1,4], record_replay=True, forged_reciprocity_rejected=True,
              web_search_used=False, arithmetic_reciprocity_identities_proved=0,
              universal_X_proofs_found=0)
(HERE / 'polarized_verification.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(report))
