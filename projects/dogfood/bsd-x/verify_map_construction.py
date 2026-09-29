"""Check recipe algebra independently, without assuming arithmetic validity."""
import json
from math import comb
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'map_construction_lab.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-map-recipes-') as directory:
    trace = str(Path(directory) / 'recipes.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0 and 'unsupported arithmetic map construction' in bad.stdout + bad.stderr


def parse(prefix):
    return [json.loads(line[len(prefix):]) for line in one.stdout.splitlines()
            if line.startswith(prefix)]


requests, cases, witnesses = parse('request '), parse('case '), parse('witness ')
assert len(requests) == len(cases) == 206 and len(witnesses) == 4
for req, row in zip(requests, cases):
    assert not row['actual_arithmetic_construction'] and not row['full_map_injective']
    if req['kind'] == 0:
        coefficients = req['polynomial']
        defect = {(i, d-i): a*comb(d, i)
                  for d, a in enumerate(coefficients, 1) for i in range(1, d) if a}
        assert row['additive'] == (not defect)
        d = max((sum(power) for power in defect), default=0)
        assert row['defect_degree'] == d
        assert row['defect_coefficient'] == (defect[(d-1, 1)] if d else 0)
        assert row['generic_differential_rank'] == int(any(coefficients))
        assert row['hidden_direction'] == [1, -1, 0]
    else:
        u, v, w = req['u'], req['v'], req['w']
        vector = row['hidden_direction']
        H = [[1, 0, u], [0, 1, v], [u, v, w]]
        action = [sum(a*b for a, b in zip(line, vector)) for line in H]
        assert vector == [-u, -v, 1] and action == [0, 0, w-u*u-v*v]
        assert row['schur_residual'] == action[2]
        assert row['full_pairing_nondegenerate'] == (action[2] != 0)
        assert row['generic_differential_rank'] == 2 and row['additive']
assert witnesses[1]['defect_coefficient'] == 2
assert witnesses[2]['full_pairing_nondegenerate'] and not witnesses[2]['full_map_injective']
assert witnesses[3]['schur_residual'] == 0
(HERE / 'map_construction_lab.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(polynomial_recipes=81, pairing_recipes=125, diagnostic_witnesses=4,
              independent_algebra_checks=True, worker_counts=[1, 4], record_replay=True,
              forged_map_claim_rejected=True, web_search_used=False,
              actual_arithmetic_maps_constructed=0, universal_X_proofs_found=0)
(HERE / 'map_construction_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report))
