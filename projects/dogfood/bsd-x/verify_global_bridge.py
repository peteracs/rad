"""Verify the symbolic inference audit; no elliptic-curve realization claim."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'global_bridge_audit.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-global-bridge-') as folder:
    trace = str(Path(folder) / 'bridge.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True,
                          text=True, check=True,
                          env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0 and 'unsupported universal bridge conclusion' in bad.stdout + bad.stderr
rows = [json.loads(line[5:]) for line in one.stdout.splitlines() if line.startswith('case ')]
assert len(rows) == 4
for i, row in enumerate(rows):
    assert row['premises_hold'] and row['universal_failure']
    assert row['missing_comparison'] == [2, 0, 2]
    assert row['divisible_corank'] == ([0, 0, 0] if i >= 2 else [2, 0, 2])
    assert row['rank_excess'] == ([2, 0, 2] if i >= 2 else [0, 0, 0])
(HERE / 'global_bridge_audit.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(symbolic_forks=4, parameter_domain='all integers u,v>=0',
              interpretation='listed numerical premises only; not elliptic curves',
              worker_counts=[1, 4], record_replay=True, forged_evidence_rejected=True,
              universal_X_proofs_found=0, foundational_formalization=False)
(HERE / 'global_bridge_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report))
