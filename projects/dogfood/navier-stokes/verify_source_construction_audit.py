"""Run the RAD port and record source provenance. Never invokes Lean."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SOURCE = Path('D:/Downloads/NavierStokesAndEuler-main')
RAD = str(ROOT/'target/debug/rad.exe')
command = [RAD,str(HERE/'source_construction_audit.rad'),'--strict-types',
           '--deny-warnings','--experimental-laws']
one = subprocess.run(command,capture_output=True,text=True,
    env={**os.environ,'RAYON_NUM_THREADS':'1'},check=True)
with tempfile.TemporaryDirectory(prefix='rad-source-audit-') as temporary:
    trace = str(Path(temporary)/'trace.radtrace')
    four = subprocess.run(command+['--record',trace],capture_output=True,text=True,
        env={**os.environ,'RAYON_NUM_THREADS':'4'},check=True)
    assert one.stdout == four.stdout
    replay = subprocess.run([RAD,'replay',trace],capture_output=True,text=True,check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(command+['--','forge'],capture_output=True,text=True)
assert bad.returncode != 0 and 'unproved analytic estimate promoted to theorem' in bad.stdout+bad.stderr
rows = [json.loads(line[7:]) for line in one.stdout.splitlines() if line.startswith('source ')]
assert len(rows) == 36
selected = next(json.loads(line[9:]) for line in one.stdout.splitlines() if line.startswith('selected '))
assert [selected[k] for k in ['particular_margin','signed_margin','bar_margin','mean_margin','defect_margin']] == [30000,29999,998,7000,79996]
assert not selected['physical_estimates_verified'] and not selected['blowup_proved']

# Local import graph is a navigation map, not proof checking.
files = {'.'.join(p.relative_to(SOURCE).with_suffix('').parts):p for p in SOURCE.rglob('*.lean')}
edges = {name:re.findall(r'^import\s+([\w.]+)',p.read_text(encoding='utf-8'),re.M) for name,p in files.items()}
visited = set()
pending = ['NavierStokes.ComparatorSolution']
while pending:
    name = pending.pop()
    if name in visited or name not in files:
        continue
    visited.add(name)
    pending.extend(edges[name])
assert not any(name.startswith('ComparatorChallenges.') for name in visited)
manifest = {name:hashlib.sha256(files[name].read_bytes()).hexdigest() for name in sorted(visited)}
(HERE/'source_construction_imports.json').write_text(json.dumps(
    {'scope':'static import navigation only','files':manifest,'imports':{n:edges[n] for n in sorted(visited)}},indent=2)+'\n',encoding='utf-8')
(HERE/'source_construction_audit.why.txt').write_text(one.stdout,encoding='utf-8')
audit = {'rad_forks':36,'heat_integrand_polynomial_identity':True,
    'radial_laplacian_coefficient_identity':True,'deterministic_workers':[1,4],
    'record_replay':True,'forged_analytic_claim_rejected':True,
    'local_source_modules':len(files),'navier_stokes_import_closure':len(visited),
    'challenge_module_imported':False,'lean_executed':False,
    'physical_estimates_verified':False,'blowup_proved':False}
(HERE/'source_construction_audit.json').write_text(json.dumps(audit,indent=2)+'\n',encoding='utf-8')
print(json.dumps(audit))
