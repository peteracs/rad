"""Independent exact linear algebra audit; matrices are not arithmetic maps."""
from fractions import Fraction
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'selmer_observability.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-observability-') as directory:
    trace = str(Path(directory) / 'map.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0 and 'unproved arithmetic bridge in submitted evidence' in bad.stdout + bad.stderr


def rank(a, rows, cols, prime=None):
    mat = [[Fraction(a[3*i+j]) if prime is None else a[3*i+j] % prime
            for j in range(cols)] for i in range(rows)]
    pivot_row = 0
    for col in range(cols):
        pivot = next((i for i in range(pivot_row, rows) if mat[i][col]), None)
        if pivot is None:
            continue
        mat[pivot_row], mat[pivot] = mat[pivot], mat[pivot_row]
        factor = 1 / mat[pivot_row][col] if prime is None else pow(mat[pivot_row][col], -1, prime)
        mat[pivot_row] = [x*factor if prime is None else x*factor % prime for x in mat[pivot_row]]
        for i in range(rows):
            if i != pivot_row:
                factor = mat[i][col]
                mat[i] = [x-factor*y if prime is None else (x-factor*y) % prime
                          for x,y in zip(mat[i], mat[pivot_row])]
        pivot_row += 1
        if pivot_row == rows:
            break
    return pivot_row


def parse(prefix):
    return [json.loads(line[len(prefix):]) for line in one.stdout.splitlines() if line.startswith(prefix)]


matrices, cases, witnesses = parse('matrix '), parse('case '), parse('witness ')
assert len(matrices) == len(cases) == 729 and len(witnesses) == 4


def check(a, row, rows, cols):
    exact = rank(a, rows, cols)
    assert row['exact_rank'] == exact and row['mod5_rank'] == rank(a, rows, cols, 5)
    assert row['injective'] == (exact == cols)
    assert row['within_dimension_budget'] == (rows <= 2)
    assert not row['arithmetic_bridge_verified'] and not row['x3_proved']
    v = row['kernel']
    assert len(v) == 3
    if exact < cols:
        assert any(v[:cols]) and all(x == 0 for x in v[cols:])
        assert all(sum(a[3*i+j]*v[j] for j in range(cols)) == 0 for i in range(rows))
    else:
        assert v == [0, 0, 0]


for a, row in zip(matrices, cases):
    check(a, row, 2, 3)
for a, rows, cols, row in zip(
    [[1,0,1,0,1,1,0,0,0], [1,0,1,0,1,1,0,0,1],
     [5,0,0,0,5,0,0,0,0], [1,0,0,0,1,0,0,0,0]],
    [2,3,2,2], [3,3,2,2], witnesses):
    check(a, row, rows, cols)
(HERE / 'selmer_observability.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(synthetic_matrix_forks=729, diagnostic_witnesses=4,
              independent_fraction_and_modular_elimination=True, worker_counts=[1, 4],
              record_replay=True, fabricated_arithmetic_bridge_rejected=True,
              actual_selmer_comparison_maps_constructed=0, universal_X_proofs_found=0,
              web_search_used=False, witnesses=witnesses)
(HERE / 'observability_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({key: value for key, value in report.items() if key != 'witnesses'}))
