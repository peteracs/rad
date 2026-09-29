"""Check second jets independently by interpolation of exact determinants."""
import json
import os
from pathlib import Path
from fractions import Fraction as Q
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'selmer_jet_source.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-selmer-jets-') as directory:
    trace = str(Path(directory) / 'jets.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0
assert 'jet algebra does not certify arithmetic provenance' in bad.stdout + bad.stderr

def determinant(entries):
    rows = [[Q(x) for x in entries[4*i:4*i+4]] for i in range(4)]
    answer = Q(1)
    for i in range(4):
        pivot = next((j for j in range(i, 4) if rows[j][i]), None)
        if pivot is None:
            return Q(0)
        if pivot != i:
            rows[i], rows[pivot] = rows[pivot], rows[i]
            answer = -answer
        value = rows[i][i]
        answer *= value
        for j in range(i+1, 4):
            factor = rows[j][i]/value
            rows[j] = [x-factor*y for x,y in zip(rows[j], rows[i])]
    return answer

weights = [Q(-25,12), Q(4), Q(-3), Q(4,3), Q(-1,4)]
# These weights extract the linear coefficient from every polynomial of degree <=4.
for degree in range(5):
    assert sum(w * i**degree for i,w in enumerate(weights)) == (degree == 1)

def coefficient(a,b,c,d):
    return sum(weights[s]*weights[t]*determinant(
        [aa+s*bb+t*cc+s*t*dd for aa,bb,cc,dd in zip(a,b,c,d)])
        for s in range(5) for t in range(5))

requests = [json.loads(x[8:]) for x in one.stdout.splitlines() if x.startswith('request ')]
rows = [json.loads(x[5:]) for x in one.stdout.splitlines() if x.startswith('case ')]
assert len(rows) == len(requests) == 243
boundary_changes = 0
nonzero_threshold = 0
for request,row in zip(requests, rows):
    code, rank = request['code'], request['rank']
    a = [int(i//4 == i%4 and i//4 < rank) for i in range(16)]
    b = [(i+code)%3-1 for i in range(16)]
    c = [(2*i+code)%5-2 for i in range(16)]
    d = [(3*i+code)%7-3 for i in range(16)]
    digits = code
    for pos in (10,11,14,15):
        b[pos] = digits%3-1
        digits //= 3
    full = coefficient(a,b,c,d)
    linear = coefficient(a,b,c,[0]*16)
    assert row['full_coefficient'] == full
    assert row['linear_coefficient'] == linear
    assert row['higher_correction'] == full-linear
    assert not row['arithmetic_source_verified']
    if rank == 1:
        assert full == linear == 0
    if rank == 2:
        assert full == linear == row['induced_coefficient']
        nonzero_threshold += full != 0
    if rank == 3:
        boundary_changes += full != linear
assert boundary_changes and nonzero_threshold
(HERE/'selmer_jet_source.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(matrix_jet_forks=243, independent_exact_interpolation=True,
              worker_counts=[1,4], replay_verified=True, forged_provenance_rejected=True,
              corank_two_nonzero_coefficients=nonzero_threshold,
              corank_one_higher_jet_changes=boundary_changes,
              actual_Galois_deformations_constructed=0, universal_X3_proved=False)
(HERE/'selmer_jet_verification.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(report))
