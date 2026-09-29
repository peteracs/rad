"""Verify deterministic elimination of the entrance-force/join constraint block."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s
from verify_construction_why import force

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
RAD=str(ROOT/'target/debug/rad.exe')
t=s.Symbol('t',real=True)
ZERO=s.zeros(3,1)

def path(stage,part):
    result={}
    for degree,bank in enumerate(stage[part]):
        for mode in bank:
            k=tuple(mode['k'])
            coeff=s.Matrix([(s.Integer(a)+s.I*b)/2 for a,b in zip(mode['re'],mode['im'])])
            result[k]=result.get(k,ZERO)+t**degree*coeff
    return result

def main():
    run=subprocess.run([RAD,str(HERE/'elimination_check.rad'),'--experimental-laws','--strict-types','--deny-warnings'],
        capture_output=True,text=True)
    assert run.returncode==0,run.stdout+run.stderr
    cases=[json.loads(line) for line in run.stdout.splitlines() if line.startswith('{')]
    assert [row['case'] for row in cases]==[0,1,2,3]
    for row in cases:
        original,left,right=row['original'],row['left'],row['right']
        assert left['a']==original['a'],'prior initial germ changed'
        a,b,ra=path(left,'a'),path(left,'b'),path(right,'a')
        oa,ob=path(original,'a'),path(original,'b')
        ratio=s.Rational(right['inverse_duration'],left['inverse_duration'])
        for k in a:
            assert all(s.expand(x)==0 for x in a[k]+b[k]-ra[k].subs(t,ratio*(t-1))), 'full right endpoint germ'
            assert all(s.expand(x)==0 for x in (a[k]+b[k]-oa[k]-ob[k]).subs(t,1)), 'shared velocity changed'
        if row['case']<3:
            u={k:v.subs(t,0) for k,v in ra.items()}
            ut={k:s.diff(v,t).subs(t,0) for k,v in ra.items()}
            actual=force(u,ut); reference=force(u,{})
            for k in actual:
                assert all(s.simplify(x)==0 for x in actual[k]-row['case']*reference.get(k,ZERO))
    # This is an exact affine block, not a numerical Jacobian approximation.
    hl,hr,r=s.symbols('hl hr r',positive=True)
    matrix=s.Matrix([[hr,0],[-hr,hl]])
    correction=s.Matrix([-r/hr,-r/hl])
    assert s.simplify(matrix.det()-hl*hr)==0
    assert matrix*correction==s.Matrix([-r,0])
    command=[RAD,str(HERE/'constraint_elimination.rad'),'--experimental-laws','--strict-types','--deny-warnings']
    run=subprocess.run(command,capture_output=True,text=True,env={**os.environ,'RAYON_NUM_THREADS':'1'})
    assert run.returncode==0,run.stdout+run.stderr
    rows=[json.loads(line[9:]) for line in run.stdout.splitlines() if line.startswith('solution ')]
    assert [row['background_multiple'] for row in rows]==[0,1,2]
    for row in rows:
        assert all(row[key] for key in ['endpoint_equation_solved','prior_initial_germ_preserved','shared_velocity_preserved','all_order_join_closed'])
        assert not row['whole_interval_small_force_proved']
    assert 'resolver `VerifyElimination`' in run.stdout and 'law `SubmitJetSolution`' in run.stdout
    rejected=subprocess.run(command+['--','forge-small-force'],capture_output=True,text=True)
    assert rejected.returncode!=0 and 'forged elimination certificate' in rejected.stdout+rejected.stderr
    with tempfile.TemporaryDirectory(prefix='rad-elimination-') as temporary:
        trace=Path(temporary)/'elimination.radr'
        parallel=subprocess.run(command+['--record',str(trace)],capture_output=True,text=True,
            env={**os.environ,'RAYON_NUM_THREADS':'4'})
        assert parallel.returncode==0 and parallel.stdout==run.stdout
        replay=subprocess.run([RAD,'replay',str(trace)],capture_output=True,text=True)
        assert replay.returncode==0 and 'Replay verified: world digest matches the recorded run' in replay.stderr
    print('PASS: three prescribed force targets solved; initial germs and shared velocity preserved.')
    print('Independent endpoint equations and four full join-germ identities verified, including duration/curvature changes.')
    print('Exact triangular block, forged small-force rejection, deterministic forks and replay pass.')
    print('This solves the endpoint/join block only; whole-interval small forcing and blowup remain unproved.')

if __name__=='__main__':
    main()
