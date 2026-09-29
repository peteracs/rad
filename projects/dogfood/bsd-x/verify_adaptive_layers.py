"""Independent partition oracle for the adaptive algebraic decision; not BSD."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
from functools import lru_cache

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'adaptive_layer_decision.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-adaptive-') as directory:
    trace = str(Path(directory) / 'adaptive.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0
assert 'a terminating decision is not a universal positive answer' in bad.stdout + bad.stderr

@lru_cache(None)
def partitions(total, minimum=1):
    if total == 0:
        return ((),)
    return tuple((a,) + rest for a in range(minimum, total + 1)
                 for rest in partitions(total - a, a))

def possibilities(delta, m, prefix):
    # Enumerate ALL modules of this length, not only those in the RAD fork batch.
    return {len(parts) for parts in partitions(delta) if len(parts) % 2 == m % 2
            and all(sum(a > j for a in parts) == b
                    for j, b in enumerate(prefix, 1))}

def check(request, row):
    m, exponents = request['analytic'], request['exponents']
    delta, actual = sum(exponents), sum(a > 0 for a in exponents)
    prefix = row['observed']
    assert row['delta'] == delta
    assert row['cutoff'] == max(0, delta - m - 1)
    assert row['queries'] == len(prefix) <= row['cutoff']
    assert prefix == [sum(a > j for a in exponents) for j in range(1, len(prefix) + 1)]
    for depth in range(len(prefix)):
        choices = possibilities(delta, m, tuple(prefix[:depth]))
        assert min(choices) <= m < max(choices), 'unnecessary query'
    choices = possibilities(delta, m, tuple(prefix))
    assert row['lower'] == min(choices) and row['upper'] == max(choices)
    assert choices == set(range(row['lower'], row['upper'] + 1, 2))
    assert row['status'] == (1 if actual <= m else -1)
    assert (max(choices) <= m) if row['status'] == 1 else (min(choices) > m)
    assert row['universal_X3_proved'] is False

lines = one.stdout.splitlines()
requests = [json.loads(line[8:]) for line in lines if line.startswith('request ')]
rows = [json.loads(line[5:]) for line in lines if line.startswith('case ')]
witnesses = [json.loads(line[8:]) for line in lines if line.startswith('witness ')]
assert len(requests) == len(rows) == 625 and len(witnesses) == 2
for request, row in zip(requests, rows):
    check(request, row)
for exponents, row in zip(([1, 9], [1, 1, 1, 7]), witnesses):
    check(dict(analytic=2, exponents=exponents), row)
assert [row['queries'] for row in witnesses] == [7, 7]
assert [row['status'] for row in witnesses] == [1, -1]

# Supplement the written universal sharpness proof with exact family checks.
for m in range(2, 9):
    for cutoff in range(1, 13):
        good = [1] * (m - 1) + [cutoff + 2]
        bad_parts = [1] * (m + 1) + [cutoff]
        assert sum(good) == sum(bad_parts) == m + cutoff + 1
        assert len(good) == m and len(bad_parts) == m + 2
        for j in range(1, cutoff):
            assert sum(a > j for a in good) == sum(a > j for a in bad_parts) == 1
        assert sum(a > cutoff for a in good) == 1
        assert sum(a > cutoff for a in bad_parts) == 0

(HERE / 'adaptive_layer_decision.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(forks=625, sharpness_witnesses=2, independent_all_partition_oracle=True,
              worker_counts=[1, 4], record_replay=True, forged_claim_rejected=True,
              universal_X3_proved=False, web_search_used=False,
              conclusion='Fixed-module decision terminates by max(0,delta-m-1); positivity is not universal.')
(HERE / 'adaptive_layer_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report))
