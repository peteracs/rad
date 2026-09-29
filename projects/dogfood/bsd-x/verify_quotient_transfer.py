"""Verify transfer receipts through actual Jordan-matrix operations."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'quotient_comparison_transfer.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-quotient-transfer-') as directory:
    trace = str(Path(directory) / 'transfer.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0 and 'unsupported transfer or arithmetic identification' in bad.stdout + bad.stderr


def parse(prefix):
    return [json.loads(line[len(prefix):]) for line in one.stdout.splitlines() if line.startswith(prefix)]


def partial_permutation_rank(matrix):
    occupied = set()
    for row in matrix:
        columns = [i for i, entry in enumerate(row) if entry]
        assert len(columns) <= 1
        if columns:
            col = columns[0]
            assert row[col] == 1 and col not in occupied
            occupied.add(col)
    # Nonzero rows have distinct unit pivots, so this is rank over any field.
    return len(occupied)


def filtration(exponents, depth):
    n = sum(exponents)
    matrix = [[0]*n for _ in range(n)]
    offset = 0
    for a in exponents:
        for j in range(a-1):
            matrix[offset+j][offset+j+1] = 1
        offset += a
    power = [[int(i == j) for j in range(n)] for i in range(n)]
    ranks = [n]
    for _ in range(max(depth+1, 1)):
        power = [[sum(power[i][k]*matrix[k][j] for k in range(n))
                  for j in range(n)] for i in range(n)]
        ranks.append(partial_permutation_rank(power))
    return n, ranks[1], [ranks[j]-ranks[j+1] for j in range(1, depth+1)]


requests, cases, witnesses = parse('request '), parse('case '), parse('witness ')
assert len(requests) == len(cases) == 1024 and len(witnesses) == 5
for req, row in zip(requests, cases):
    delta, epsilon, layers = filtration(req['exponents'], req['depth'])
    lower = sum(layers)
    source = delta <= req['analytic'] + req['budget']
    covered = lower >= req['budget']
    assert row['original_length'] == delta and row['discarded_length'] == epsilon
    assert row['quotient_length'] == delta-epsilon
    assert row['tail_layers'] == layers and row['tail_lower_bound'] == lower
    assert row['source_upper_bound_holds'] == source and row['budget_covered'] == covered
    assert row['conditional_bound_transferred'] == (req['comparison_kind'] == 1 and source and covered)
    assert row['exact_target_holds'] == (delta-epsilon <= req['analytic'])
    assert not row['arithmetic_instantiation_verified']
assert [row['conditional_bound_transferred'] for row in witnesses] == [True, True, False, False, False]
(HERE / 'quotient_comparison_transfer.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(module_forks=1024, diagnostic_witnesses=5, independent_Jordan_matrix_checks=True,
              worker_counts=[1, 4], record_replay=True, forged_transfer_rejected=True,
              web_search_used=False, actual_analytic_comparisons_verified=0,
              universal_X_proofs_found=0)
(HERE / 'quotient_transfer_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report))
