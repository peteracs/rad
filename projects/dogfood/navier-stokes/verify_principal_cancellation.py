"""Execute the source principal-equation port and test a physical-sign omission."""
import json
from pathlib import Path
import subprocess
import tempfile

HERE=Path(__file__).resolve().parent
RAD=str(HERE.parents[2]/'target/debug/rad.exe')
command=[RAD,str(HERE/'principal_cancellation.rad'),'--strict-types','--deny-warnings','--experimental-laws']
with tempfile.TemporaryDirectory(prefix='rad-principal-') as directory:
    trace=str(Path(directory)/'trace.radtrace')
    run=subprocess.run(command+['--record',trace],capture_output=True,text=True,check=True)
    replay=subprocess.run([RAD,'replay',trace],capture_output=True,text=True,check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad=subprocess.run(command+['--','omit-damping'],capture_output=True,text=True)
assert bad.returncode != 0 and 'physical principal residual does not cancel' in bad.stdout+bad.stderr
row=json.loads(run.stdout.splitlines()[0])
assert row['pressure_cancelled'] and row['tangency_defect_damped']
assert not row['full_localized_residual_zero'] and not row['actual_derivative_estimates_proved']
(HERE/'principal_cancellation.why.txt').write_text(run.stdout,encoding='utf-8')
row.update(symbolic_variables=16,record_replay=True,omitted_damping_rejected=True)
(HERE/'principal_cancellation.json').write_text(json.dumps(row,indent=2)+'\n',encoding='utf-8')
print(json.dumps(row))
