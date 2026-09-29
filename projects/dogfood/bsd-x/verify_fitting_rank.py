"""Audit exact rank criteria, not universal arithmetic comparisons."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'fitting_rank_target.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-fitting-') as directory:
    trace = str(Path(directory) / 'fitting.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
for forged in ('analytic', 'same-module'):
    bad = subprocess.run(cmd + ['--', forged], capture_output=True, text=True)
    assert bad.returncode != 0 and 'unsupported analytic comparison or module equivalence' in bad.stdout + bad.stderr


def parse(prefix):
    return [json.loads(line[len(prefix):]) for line in one.stdout.splitlines() if line.startswith(prefix)]


requests, cases, witnesses = parse('request '), parse('case '), parse('witness ')
assert len(requests) == len(cases) == 1250 and len(witnesses) == 3
for request, row in zip(requests, cases):
    exponents, m = request['exponents'], request['analytic']
    n = len(exponents)
    dimension = n-exponents.count(0)
    minor_size = max(n-m, 0)
    assert row['fitting_valuation'] == sum(sorted(exponents)[:minor_size])
    assert row['selmer_dimension'] == dimension and row['determinant_order'] == sum(exponents)
    assert row['flattened_exponents'] == [int(a > 0) for a in exponents]
    assert row['flattened_order'] == dimension
    assert row['rank_bound'] == row['flat_bound'] == (dimension <= m)
    assert row['determinant_bound'] == (sum(exponents) <= m)
    assert row['same_module'] == all(a <= 1 for a in exponents)
    assert not row['analytic_comparison_proved']
    indices = row['unit_minor_indices']
    if row['fitting_valuation'] == 0:
        assert len(indices) == len(set(indices)) == minor_size
        assert all(0 <= i < n and exponents[i] == 0 for i in indices)
    else:
        assert not indices
assert [row['determinant_order'] for row in witnesses] == [4, 4, 4]
assert [row['selmer_dimension'] for row in witnesses] == [2, 4, 1]
assert [row['rank_bound'] for row in witnesses] == [True, False, True]
(HERE / 'fitting_rank_target.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(module_forks=1250, same_determinant_witnesses=3,
              independent_invariant_factor_checks=True, worker_counts=[1, 4],
              record_replay=True, forged_comparison_rejected=True,
              forged_module_equivalence_rejected=True, web_search_used=False,
              arithmetic_background='supplied by user, not reverified', universal_X_proofs_found=0)
(HERE / 'fitting_rank_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report))
