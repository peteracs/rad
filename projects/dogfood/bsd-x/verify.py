"""Verify abstract Selmer diagnostics, not elliptic-curve realizations."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

HERE=Path(__file__).resolve().parent
RAD=str(HERE.parents[2]/'target/debug/rad.exe')
command=[RAD,str(HERE/'selmer_model.rad'),'--strict-types','--deny-warnings','--experimental-laws']
one=subprocess.run(command,capture_output=True,text=True,check=True,
    env={**os.environ,'RAYON_NUM_THREADS':'1'})
with tempfile.TemporaryDirectory(prefix='rad-bsd-x-') as directory:
    trace=str(Path(directory)/'trace.radtrace')
    four=subprocess.run(command+['--record',trace],capture_output=True,text=True,check=True,
        env={**os.environ,'RAYON_NUM_THREADS':'4'})
    assert one.stdout==four.stdout
    replay=subprocess.run([RAD,'replay',trace],capture_output=True,text=True,check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad=subprocess.run(command+['--','forge'],capture_output=True,text=True)
assert bad.returncode!=0 and 'unproved X claim in submitted evidence' in bad.stdout+bad.stderr
rows=[json.loads(line) for line in one.stdout.splitlines() if line.startswith('{')]
assert len(rows)==4
assert not rows[0]['x1_dimension'] and rows[0]['primary_finite']
assert not rows[1]['x3_eventually'] and rows[1]['primary_finite']
assert rows[2]['residual_exponent']==6 and not rows[2]['primary_finite']
assert not rows[3]['x3_at_level'] and rows[3]['x3_eventually']

def exponent(r,t,spectrum,k):
    return k*(r+t)+2*sum(min(k,a) for a in spectrum)

checks=0
for m in range(2,7):
    for r in range(7):
        for t in range(5):
            if (r+t-m)%2:
                continue
            for k in range(4):
                for spectrum in ([],[1],[3],[1,3]):
                    growth=exponent(r,t,spectrum,k+1)-exponent(r,t,spectrum,k)
                    assert growth==r+t+2*sum(a>k for a in spectrum)
                    for d in range(r+1):
                        if (d-m)%2==0 and growth<=d+1:
                            assert r==d and t==0 and all(a<=k for a in spectrum)
                    checks+=1
assert checks==1408 and f'checked {checks}' in one.stdout
assert 'prefix_checks 90' in one.stdout
for cutoff in range(1,101):
    for k in range(cutoff+1):
        assert exponent(2,2,[],k)==exponent(2,0,[cutoff],k)

(HERE/'selmer_model.why.txt').write_text(one.stdout,encoding='utf-8')
report=dict(abstract_forks=checks,rad_prefix_checks=90,python_prefix_checks=5150,
    worker_counts=[1,4],record_replay=True,forged_X_rejected=True,
    actual_elliptic_curves_computed=0,unconditional_X_proofs_found=0,
    witnesses=rows)
(HERE/'verification.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k!='witnesses'}))
