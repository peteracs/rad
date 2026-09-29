"""RAD mixed-jet audit, independent symbolic check, replay, and mutations."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
RAD=str(ROOT/'target/debug/rad.exe')
flags=['--strict-types','--deny-warnings','--experimental-laws']
command=[RAD,str(HERE/'nonlinear_residual_audit.rad'),*flags]
one=subprocess.run(command,capture_output=True,text=True,check=True,
    env={**os.environ,'RAYON_NUM_THREADS':'1'})
with tempfile.TemporaryDirectory(prefix='rad-nonlinear-jets-') as temporary:
    trace=str(Path(temporary)/'trace.radtrace')
    four=subprocess.run(command+['--record',trace],capture_output=True,text=True,check=True,
        env={**os.environ,'RAYON_NUM_THREADS':'4'})
    assert one.stdout == four.stdout
    replay=subprocess.run([RAD,'replay',trace],capture_output=True,text=True,check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
for mutation,message in [('omit-cross','missing nonlinear cross term'),('forge','forged actual-stage bound')]:
    bad=subprocess.run(command+['--',mutation],capture_output=True,text=True)
    assert bad.returncode != 0 and message in bad.stdout+bad.stderr

# Independently evaluate formal jets on nonlinear spacetime polynomials.
# This tests key encoding and actual derivative semantics, not just two
# copies of the same symbolic residual implementation.
variables=s.symbols('t x y z')
t,x,y,z=variables
fields=[[t*x+y*z+x**3, t*y+z*x+y**3, t*z+x*y+z**3],
        [t**2*y+x*z**2,t*x*y+y*z,t*y*z+x**2*z],
        [t*x*y*z+x**2*y,0,0],[t**2*x*z+y**2*z,0,0]]
point={t:1,x:2,y:-1,z:1}
script='''use "residual_jets.rad"
fn main() -> nil {
    for comp in range(3) {
        let mut p: list<JetTerm> = combine(residual(comp,true,3),residual(comp,false,3),-1)
        for axis in [0,1,3] { p = differentiate(p,axis) }
        print("terms",json_stringify(p))
    }
}
'''
launch=HERE/'nonlinear_residual_verify_launch.rad'
launch.write_text(script,encoding='utf-8')
run=subprocess.run([RAD,str(launch),*flags],capture_output=True,text=True,check=True)
banks=[json.loads(line[6:]) for line in run.stdout.splitlines() if line.startswith('terms ')]
def jet(key):
    if key < 0: return 1
    f,rem=divmod(key,10000)
    field,comp=divmod(f,3)
    orders=[rem//1000,rem//100%10,rem//10%10,rem%10]
    expr=fields[field][comp]
    for variable,order in zip(variables,orders): expr=s.diff(expr,variable,order)
    return expr.subs(point)
def residual(u,p):
    return [s.diff(u[i],t)-3*sum(s.diff(u[i],v,2) for v in variables[1:])
        +s.diff(p,variables[i+1])+sum(u[j]*s.diff(u[i],variables[j+1]) for j in range(3)) for i in range(3)]
aug=residual([fields[0][i]+fields[1][i] for i in range(3)],fields[2][0]+fields[3][0])
base=residual(fields[0],fields[2][0])
for i,bank in enumerate(banks):
    actual=sum(term['c']*jet(term['a'])*jet(term['b']) for term in bank)
    expected=s.diff(aug[i]-base[i],t,x,z).subs(point)
    assert actual == expected, (i,actual,expected)
rows=[json.loads(line[4:]) for line in one.stdout.splitlines() if line.startswith('jet ')]
assert len(rows)==35 and all(row['identity'] and not row['actual_stage_bounds_verified'] for row in rows)
audit=dict(mixed_multiindices=35,component_identities=105,
    independent_spacetime_polynomial_check=True,record_replay=True,
    deterministic_workers=[1,4],omitted_cross_term_rejected=True,
    forged_stage_bound_rejected=True,actual_stage_bounds_verified=False)
(HERE/'nonlinear_residual_audit.why.txt').write_text(one.stdout,encoding='utf-8')
(HERE/'nonlinear_residual_audit.json').write_text(json.dumps(audit,indent=2)+'\n',encoding='utf-8')
print(json.dumps(audit))
