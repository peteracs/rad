"""Actual schedule identities, independent full RHS, and causal control receipts."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s
from verify_construction_why import force,ZERO

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
RAD=str(ROOT/'target/debug/rad.exe')

def cyclic(amplitudes):
    result={}
    for axis in range(3):
        for polarity in [-1,1]:
            k=[0,0,0]; k[axis]=polarity
            value=s.zeros(3,1); value[(axis+1)%3]=s.Rational(amplitudes[axis],2)
            result[tuple(k)]=value
    return result

def main():
    run=subprocess.run([RAD,str(HERE/'force_schedule_check.rad'),'--strict-types','--deny-warnings'],capture_output=True,text=True)
    assert run.returncode==0,run.stdout+run.stderr
    atoms=[json.loads(line[5:]) for line in run.stdout.splitlines() if line.startswith('atom ')]
    assert [a['q'] for a in atoms]==[1,2,3,8,64,1000000]
    for atom in atoms:
        q=atom['q']
        assert atom==dict(q=q,start_gap_exponent=q-1,end_gap_exponent=q,frequency_exponent=q-1,
                         amplitude_exponent=5-q*q,coefficients=[1,1,1])
    q,d,x=s.symbols('q d x',real=True)
    assert s.expand((2*q-d)**2-(4*q*q-4*d*q+d*d))==0
    assert s.simplify(1/x+1/(1-x)-4-(2*x-1)**2/(x*(1-x)))==0
    beta=s.exp(4-1/x-1/(1-x))
    assert beta.subs(x,s.Rational(1,2))==1
    assert s.diff(beta,x).subs(x,s.Rational(1,2))==0
    summaries=[json.loads(line[8:]) for line in run.stdout.splitlines() if line.startswith('summary ')]
    assert summaries[0]['actual_force_budget_checks']==676 and summaries[0]['H2_ball_radius']==42
    assert s.Rational(64,16384)==s.Rational(1,256)
    assert s.Rational(2*42,256)<s.Rational(1,2)
    u=cyclic([8,8,8]); prescribed=cyclic([16,16,16])
    drift=force(u,{})
    expected={k:prescribed.get(k,ZERO)-drift.get(k,ZERO) for k in set(drift)|set(prescribed)}
    actual={}
    for line in run.stdout.splitlines():
        if not line.startswith('rhs '): continue
        mode=json.loads(line[4:]); k=tuple(mode['k'])
        assert k not in actual
        actual[k]=s.Matrix([s.Rational(r,mode['denominator'])+s.I*s.Rational(im,mode['denominator'])
                            for r,im in zip(mode['re'],mode['im'])])
    assert actual.keys()==expected.keys()
    for k in expected: assert all(s.simplify(v)==0 for v in actual[k]-expected[k])
    command=[RAD,str(HERE/'force_first.rad'),'--experimental-laws','--strict-types','--deny-warnings']
    run=subprocess.run(command,capture_output=True,text=True,env={**os.environ,'RAYON_NUM_THREADS':'1'})
    assert run.returncode==0,run.stdout+run.stderr
    rows=[json.loads(line[8:]) for line in run.stdout.splitlines() if line.startswith('control ')]
    assert [row['lane'] for row in rows]==list(range(27))
    for row in rows:
        lane=row['lane']; controls=[lane//9-1,(lane//3)%3-1,lane%3-1]
        f=cyclic([16*c for c in controls])
        du={k:f.get(k,ZERO)-drift.get(k,ZERO) for k in set(drift)|set(f)}
        rate=s.expand(sum(sum(t*t for t in k)*s.re(s.conjugate(uk).dot(du[k])) for k,uk in u.items()))
        assert row['controls']==controls and row['enstrophy_rate8']==8*rate
        assert row['forcing_production8']==512*sum(controls)
        assert row['nonlinear_production8']==0 and row['viscous_loss8']==768
        assert row['generated_velocity_modes']==sum(v!=ZERO for k,v in du.items() if k not in u)==12
        assert not row['velocity_trajectory_proved'] and not row['repeated_amplification_proved']
    decision=[json.loads(line[9:]) for line in run.stdout.splitlines() if line.startswith('decision ')][0]
    assert decision['selected_controls']==[1,1,1] and not decision['probe_is_solution_trajectory']
    assert 'resolver `SelectControl`' in run.stdout and 'law `SubmitControl`' in run.stdout
    rejected=subprocess.run(command+['--','forge-cascade'],capture_output=True,text=True)
    assert rejected.returncode!=0 and 'forged force-first certificate' in rejected.stdout+rejected.stderr
    with tempfile.TemporaryDirectory(prefix='rad-force-first-') as temporary:
        trace=Path(temporary)/'force-first.radr'
        parallel=subprocess.run(command+['--record',str(trace)],capture_output=True,text=True,
            env={**os.environ,'RAYON_NUM_THREADS':'4'})
        assert parallel.returncode==0 and parallel.stdout==run.stdout
        replay=subprocess.run([RAD,'replay',str(trace)],capture_output=True,text=True)
        assert replay.returncode==0 and 'Replay verified: world digest matches the recorded run' in replay.stderr
    print('PASS: actual schedule descriptors and 676 derivative-budget instances; exact dictated full Fourier RHS.')
    print('27 bounded-control probes agree with independent algebra; direct forcing is separated from nonlinear production.')
    print('Local Picard arithmetic, forged cascade rejection, deterministic workers and replay pass.')
    print('Global force smoothness and local PDE existence use the documented analytic lemmas; no blowup certified.')

if __name__=='__main__': main()
