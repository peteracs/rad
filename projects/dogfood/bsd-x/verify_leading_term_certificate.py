"""Cross-check conditional order certificates; no elliptic curve is computed."""
import json
import math
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
command = [RAD, str(HERE / 'leading_term_certificate.rad'),
           '--strict-types', '--deny-warnings', '--experimental-laws']
one = subprocess.run(command, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-bsd-order-') as directory:
    trace = str(Path(directory) / 'trace.radtrace')
    four = subprocess.run(command + ['--record', trace], capture_output=True,
                          text=True, check=True,
                          env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True,
                            text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(command + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0
assert 'unsupported leading-term equality' in bad.stdout + bad.stderr
cases = [json.loads(line[5:]) for line in one.stdout.splitlines()
         if line.startswith('case ')]
witnesses = [json.loads(line[8:]) for line in one.stdout.splitlines()
             if line.startswith('witness ')]
assert len(cases) == 400 and len(witnesses) == 6


def valuation(n, p):
    exponent = 0
    while n % p == 0:
        exponent += 1
        n //= p
    return exponent


def verify(row, exact_primes=()):
    # Independent enumeration of square integers, not RAD's factor rounding.
    divisor, target, bound = row['divisor'], row['target'], row['bound']
    least = next(root * root for root in range(1, divisor + 1)
                 if root * root % divisor == 0)
    candidates = [root * root for root in range(1, math.isqrt(bound) + 1)
                  if root * root % divisor == 0
                  and all(valuation(root * root, p) == valuation(target, p)
                          for p in exact_primes)]
    escape = next(j for j in range(2, 102)
                  if all(j % p for p in exact_primes))
    assert row['square_multiple'] == least
    assert row['escape_factor'] == escape
    assert row['next_order'] == escape * escape * least
    assert row['possible_count'] == len(candidates)
    assert row['unique_order'] == (candidates[0] if len(candidates) == 1 else 0)
    assert row['conditional_equality'] == (candidates == [target])
    assert row['inconsistent'] == (not candidates)


for row in cases:
    verify(row)
for i, row in enumerate(witnesses):
    verify(row, (2, 3) if i >= 4 else ())
assert [row['conditional_equality'] for row in witnesses] == [True, False, False, False, True, False]
assert witnesses[3]['unique_order'] == 36
assert witnesses[4]['next_order'] == 900
assert witnesses[5]['possible_count'] == 2
(HERE / 'leading_term_certificate.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(abstract_forks=len(cases), diagnostic_witnesses=len(witnesses),
              independent_square_enumeration=True, worker_counts=[1, 4],
              record_replay=True, forged_conclusion_rejected=True,
              actual_elliptic_curves_computed=0, unconditional_X_proofs_found=0,
              witnesses=witnesses)
(HERE / 'leading_term_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({key: value for key, value in report.items() if key != 'witnesses'}))
