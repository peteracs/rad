"""Validate the shape-independent cost bound and its causal accounting guard."""
from fractions import Fraction as F
import json
from math import factorial
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s
from verify_construction_why import force, ZERO

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
RAD=str(ROOT/'target/debug/rad.exe')

def main():
    run=subprocess.run([RAD,str(HERE/'force_cost_check.rad'),'--experimental-laws','--strict-types','--deny-warnings'],
        capture_output=True,text=True)
    assert run.returncode==0,run.stdout+run.stderr
    boundary=[json.loads(line[9:]) for line in run.stdout.splitlines() if line.startswith('boundary ')]
    assert boundary==[dict(entry_n=1,exit_n=0,denominator=2,constant_entrance_germ=True)]
    u={}
    for k,p in [((1,0,0),(0,1,1)),((0,0,1),(1,1,0))]:
        for polarity in [-1,1]: u[tuple(polarity*t for t in k)]=s.Matrix(p)/2
    initial_force=force(u,{})
    ut={k:-v for k,v in initial_force.items() if v!=ZERO}
    terminal_force=force(u,ut)
    assert initial_force[(1,0,1)][1]==s.I/2
    assert all(v==ZERO for v in terminal_force.values())
    flat=[json.loads(line[5:]) for line in run.stdout.splitlines() if line.startswith('flat ')]
    assert len(flat)==40
    for row in flat:
        assert row['cost_n']==factorial(row['order'])*row['speed']**row['order']
    polynomial=[json.loads(line[11:]) for line in run.stdout.splitlines() if line.startswith('polynomial ')]
    assert [r['cost_n'] for r in polynomial]==[20,32,0]
    # Exact Taylor remainder kernel, independently checked at all tested orders.
    t,delta=s.symbols('t delta',positive=True)
    for m in range(1,9):
        kernel=(delta-t)**(m-1)/s.factorial(m-1)
        assert s.simplify(s.integrate(kernel,(t,0,delta))-delta**m/s.factorial(m))==0
    command=[RAD,str(HERE/'force_cost_why.rad'),'--experimental-laws','--strict-types','--deny-warnings']
    run=subprocess.run(command,capture_output=True,text=True,env={**os.environ,'RAYON_NUM_THREADS':'1'})
    assert run.returncode==0,run.stdout+run.stderr
    rows=[json.loads(line[5:]) for line in run.stdout.splitlines() if line.startswith('cost ')]
    assert [row['lane'] for row in rows]==list(range(8))
    for row in rows:
        inverse_allowance=factorial(row['derivative_order'])*row['inverse_duration']**row['derivative_order']
        assert row['lower_bound_n']==inverse_allowance and row['denominator']==2
        assert row['max_force_jump_for_unit_cap_den']==inverse_allowance
        assert row['background_jump_n']+row['residual_jump_n']==row['total_jump_n']==-1
        assert row['unit_derivative_cap_impossible']==(F(inverse_allowance,2)>1)
        assert not row['whole_interval_bound_proved']
    for i in range(0,8,2): assert rows[i]['lower_bound_n']==rows[i+1]['lower_bound_n']
    assert 'resolver `VerifyCost`' in run.stdout and 'law `SubmitCost`' in run.stdout
    rejected=subprocess.run(command+['--','hide-background-cost'],capture_output=True,text=True)
    assert rejected.returncode!=0 and 'forged force cost certificate' in rejected.stdout+rejected.stderr
    with tempfile.TemporaryDirectory(prefix='rad-force-cost-') as temporary:
        trace=Path(temporary)/'cost.radr'
        parallel=subprocess.run(command+['--record',str(trace)],capture_output=True,text=True,
            env={**os.environ,'RAYON_NUM_THREADS':'4'})
        assert parallel.returncode==0 and parallel.stdout==run.stdout
        replay=subprocess.run([RAD,'replay',str(trace)],capture_output=True,text=True)
        assert replay.returncode==0 and 'Replay verified: world digest matches the recorded run' in replay.stderr
    print('PASS: actual full-force boundary data, 40 flat-germ costs and three nonconstant-germ costs.')
    print('Taylor kernels, eight accounting diagnoses, hidden background-cost rejection and replay verified.')
    print('Necessary force-cost constraints only; no sufficient whole-interval bound or blowup proof.')

if __name__=='__main__': main()
