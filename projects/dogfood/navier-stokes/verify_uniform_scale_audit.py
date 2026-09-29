"""Run uniformity identities in RAD; do not certify their PDE premises."""
import json
from pathlib import Path
import subprocess
import tempfile

HERE=Path(__file__).resolve().parent
RAD=str(HERE.parents[2]/'target/debug/rad.exe')
command=[RAD,str(HERE/'uniform_scale_audit.rad'),'--strict-types','--deny-warnings','--experimental-laws']
with tempfile.TemporaryDirectory(prefix='rad-uniform-scale-') as directory:
    trace=str(Path(directory)/'trace.radtrace')
    run=subprocess.run(command+['--record',trace],capture_output=True,text=True,check=True)
    replay=subprocess.run([RAD,'replay',trace],capture_output=True,text=True,check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad=subprocess.run(command+['--','forge'],capture_output=True,text=True)
assert bad.returncode != 0 and 'source premise cannot be promoted' in bad.stdout+bad.stderr
rows=[json.loads(line[6:]) for line in run.stdout.splitlines() if line.startswith('scale ')]
assert len(rows)==6 and {r['polynomial_degree'] for r in rows}==set(range(1,7))
assert all(r['ratio_polynomial_nonnegative'] and r['inverse_exponent_algebra']
    and not r['actual_weighted_invariant_proved'] for r in rows)
assert 'ResolveScale' in run.stdout and 'SubmitScale' in run.stdout
(HERE/'uniform_scale_audit.why.txt').write_text(run.stdout,encoding='utf-8')
audit=dict(symbolic_forks=6,record_replay=True,forged_invariant_rejected=True,
    actual_weighted_invariant_proved=False)
(HERE/'uniform_scale_audit.json').write_text(json.dumps(audit,indent=2)+'\n',encoding='utf-8')
print(json.dumps(audit))
