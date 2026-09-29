"""Check a causal local repair without permitting promotion to a stage proof."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
RAD=str(ROOT/'target/debug/rad.exe')
command=[RAD,str(HERE/'residual_why.rad'),'--experimental-laws','--strict-types','--deny-warnings']
run=subprocess.run(command,capture_output=True,text=True,env={**os.environ,'RAYON_NUM_THREADS':'1'})
assert run.returncode==0,run.stdout+run.stderr
rows=[json.loads(line[9:]) for line in run.stdout.splitlines() if line.startswith('evidence ')]
assert len(rows)==2
for i,row in enumerate(rows):
    assert row==dict(cleared=bool(i),denominator=8,lane=i,nonzero_force_modes=6-2*i,
        repeatability_proved=False,residual_n=16-16*i,time_n=-16*i,
        trajectory_proved=False,transport_n=16)
assert 'resolver `ChooseRepair`' in run.stdout and 'law `SubmitRepair`' in run.stdout
decisions=[json.loads(line[9:]) for line in run.stdout.splitlines() if line.startswith('decision ')]
assert decisions==[dict(local_coefficient_cleared=True,remaining_force_modes=4,selected_lane=1,stage_certified=False)]
rejected=subprocess.run(command+['--','forge-stage'],capture_output=True,text=True)
assert rejected.returncode!=0 and 'forged repair diagnosis' in rejected.stdout+rejected.stderr
# Independent physical-space calculation, with nu=1 and global periodic p.
x,y,z=s.symbols('x y z',real=True)
coords=[x,y,z]
u=s.Matrix([2*s.cos(z),2*s.cos(x)+2*s.cos(z),2*s.cos(x)])
ut=s.Matrix([0,4*s.sin(x+z),0])
p=4*s.sin(x)*s.sin(z)
adv=s.Matrix([sum(u[j]*s.diff(u[i],coords[j]) for j in range(3)) for i in range(3)])
lap=s.Matrix([sum(s.diff(u[i],c,2) for c in coords) for i in range(3)])
grad=s.Matrix([s.diff(p,c) for c in coords])
assert all(s.trigsimp(v)==0 for v in ut+adv+grad-lap-u)
with tempfile.TemporaryDirectory(prefix='rad-residual-why-') as temporary:
    trace=Path(temporary)/'repair.radr'
    parallel=subprocess.run(command+['--record',str(trace)],capture_output=True,text=True,
        env={**os.environ,'RAYON_NUM_THREADS':'4'})
    assert parallel.returncode==0 and parallel.stdout==run.stdout
    replay=subprocess.run([RAD,'replay',str(trace)],capture_output=True,text=True)
    assert replay.returncode==0 and 'Replay verified: world digest matches the recorded run' in replay.stderr
print('PASS: exact physical-space repair, two isolated proposals, causal selection, forged proof rejection and replay.')
print('Local force-coefficient repair only; the trajectory and repeatable-stage obligations remain open.')
