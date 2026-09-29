"""Execution audit of the single-level implication, not BSD discovery."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'single_level_certificate.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-bsd-level-') as folder:
    trace = str(Path(folder) / 'level.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0 and 'unsupported single-level certificate' in bad.stdout + bad.stderr


def rows(prefix):
    return [json.loads(line[len(prefix):]) for line in one.stdout.splitlines()
            if line.startswith(prefix)]


requests, cases, witnesses = rows('request '), rows('case '), rows('witness ')
assert len(requests) == len(cases) == 720 and len(witnesses) == 3
for req, row in zip(requests, cases):
    n, r, t, m = (req[key] for key in ('level', 'rank', 'divisible', 'analytic'))
    # Count filtration layers rather than reuse RAD's sum-of-minima formula.
    layers = [r + t + sum(a >= j for a in req['spectrum']) for j in range(1, n + 42)]
    total = sum(layers[:n])
    passed = total < n * (m + 1)
    closed = passed and r >= m
    assert row == dict(exponent=total, average_floor=total // n,
                       last_growth=layers[n - 1], next_growth=layers[n],
                       threshold=n * (m + 1), certificate=passed,
                       rank_closed=closed, primary_closed=closed)
    assert all(value <= total // n for value in layers[n - 1:])
    if closed:
        assert r == m and t == 0 and all(a < n for a in req['spectrum'])
assert witnesses[0]['certificate'] and not witnesses[1]['certificate']
assert witnesses[1] == witnesses[2]
(HERE / 'single_level_certificate.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(abstract_forks=720, diagnostic_witnesses=3, independent_layer_counts=True,
              worker_counts=[1, 4], record_replay=True, forged_conclusion_rejected=True,
              actual_elliptic_curves_computed=0, universal_X_placeholders_filled=0,
              witnesses=witnesses)
(HERE / 'single_level_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({key: value for key, value in report.items() if key != 'witnesses'}))
