"""Verify symbolic elimination execution, not the remaining arithmetic premise."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2] / 'target/debug/rad.exe')
cmd = [RAD, str(HERE / 'eliminate_transfer_budget.rad'), '--strict-types',
       '--deny-warnings', '--experimental-laws']
one = subprocess.run(cmd, capture_output=True, text=True, check=True,
                     env={**os.environ, 'RAYON_NUM_THREADS': '1'})
with tempfile.TemporaryDirectory(prefix='rad-budget-elimination-') as directory:
    trace = str(Path(directory) / 'elimination.radtrace')
    four = subprocess.run(cmd + ['--record', trace], capture_output=True, text=True,
                          check=True, env={**os.environ, 'RAYON_NUM_THREADS': '4'})
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(cmd + ['--', 'forge'], capture_output=True, text=True)
assert bad.returncode != 0 and 'eliminating a parameter did not prove the remaining arithmetic premise' in bad.stdout + bad.stderr
rows = [json.loads(line[5:]) for line in one.stdout.splitlines() if line.startswith('case ')]
assert len(rows) == 16
for row in rows:
    assert row == dict(remaining_rank_condition=[1,-1,0,0],
                       remaining_length_condition=[0,0,1,0],
                       universal_arithmetic_bound_proved=False)
(HERE / 'eliminate_transfer_budget.why.txt').write_text(one.stdout, encoding='utf-8')
report = dict(symbolic_presentation_forks=16, worker_counts=[1,4], record_replay=True,
              forged_arithmetic_claim_rejected=True, web_search_used=False,
              universal_arithmetic_bounds_proved=0,
              conclusion='joint source/correction existence is equivalent to s_p<=m')
(HERE / 'budget_elimination_verification.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(report))
