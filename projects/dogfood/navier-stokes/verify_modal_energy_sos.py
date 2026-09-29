"""RAD proof certificates for every vertex of the modal coefficient box."""
import json
from pathlib import Path
import subprocess
import tempfile
HERE=Path(__file__).resolve().parent
RAD=str(HERE.parents[2]/'target/debug/rad.exe')
command=[RAD,str(HERE/'modal_energy_sos.rad'),'--strict-types','--deny-warnings','--experimental-laws']
with tempfile.TemporaryDirectory(prefix='rad-modal-energy-') as directory:
    trace=str(Path(directory)/'trace.radtrace')
    run=subprocess.run(command+['--record',trace],capture_output=True,text=True,check=True)
    replay=subprocess.run([RAD,'replay',trace],capture_output=True,text=True,check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad=subprocess.run(command+['--','factor-one'],capture_output=True,text=True)
assert bad.returncode != 0 and 'proposed uniform energy bound is false' in bad.stdout+bad.stderr
result=json.loads(run.stdout.splitlines()[0])
assert result['vertices']==16 and result['quadratic_error_factor']==2
assert result['coefficient_box_certified'] and not result['full_construction_certified']
(HERE/'modal_energy_sos.why.txt').write_text(run.stdout,encoding='utf-8')
result.update(record_replay=True,invalid_stronger_bound_rejected=True)
(HERE/'modal_energy_sos.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
print(json.dumps(result))
