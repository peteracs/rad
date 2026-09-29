"""Verify an independent hypothesis experiment, not unconditional BSD."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'termination_synthesis.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-termination-') as directory:
    trace = str(Path(directory) / 'run.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0 and 'unsupported termination proof' in bad.stdout + bad.stderr
assert 'candidates 729 accepted 0' in one.stdout
blocks = [tuple(map(int, line.split()[1:])) for line in one.stdout.splitlines()
          if line.startswith('block ')]
assert len(blocks) == 110
for height, delay, mass in blocks:
    assert mass == delay * sum(range(1, height + 1))
    for m in range(2, 7):
        n = mass + 1
        total = sum(m + max(height - k // delay, 0) for k in range(n))
        assert total < n * (m + 1)
(HERE / 'termination_synthesis.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(web_search_used=False, independent_hypotheses_tested=2,
              potential_candidates=729, potential_candidates_accepted=0,
              conditional_block_checks=110, worker_counts=[1, 4], record_replay=True,
              forged_acceptance_rejected=True, universal_X_proofs_found=0,
              conclusion='block decrease suffices but its existence is equivalent to the missing termination claim')
(HERE / 'termination_verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report))
