"""Independent exact diagnosis, including an actual interior residual witness."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
RAD=str(ROOT/'target/debug/rad.exe')
ZERO=s.zeros(3,1)
TARGET=(1,0,1)

def force(u,ut):
    conv={}
    for p,up in u.items():
        for q,uq in u.items():
            k=tuple(p[j]+q[j] for j in range(3))
            conv[k]=conv.get(k,ZERO)+s.I*up.dot(s.Matrix(q))*uq
    result={}
    for k in set(conv)|set(u)|set(ut):
        n=sum(t*t for t in k); wave=s.Matrix(k)
        adv=conv.get(k,ZERO)
        value=ut.get(k,ZERO)+adv-(wave*wave.dot(adv)/n if n else ZERO)+n*u.get(k,ZERO)
        result[k]=value.applyfunc(s.expand)
    return result

def main():
    u0={}
    for k,p in [((1,0,0),(0,1,1)),((0,0,1),(1,1,0))]:
        for polarity in [-1,1]:
            u0[tuple(polarity*t for t in k)]=s.Matrix(p)/2
    reference=force(u0,{})
    assert reference[TARGET][1]==s.I/2
    velocity_derivative={k:-v for k,v in reference.items() if v!=ZERO}
    tau=s.Symbol('tau',real=True)
    germ={k:u0.get(k,ZERO)+tau*velocity_derivative.get(k,ZERO) for k in set(u0)|set(velocity_derivative)}
    repaired_force=force(germ,velocity_derivative)
    repaired_jets=[s.simplify(s.diff(repaired_force[TARGET][1],tau,j).subs(tau,0)/s.I) for j in range(5)]
    assert repaired_jets==[0,-2,1,0,0]
    w1={TARGET:s.Matrix([0,-s.I,0]),(-1,0,-1):s.Matrix([0,s.I,0])}
    w2={k:v for k,v in u0.items() if k[0]!=0}
    support=set(u0)|set(w1)|set(w2)
    midpoint={k:u0.get(k,ZERO)+w1.get(k,ZERO)/4+w2.get(k,ZERO)/8 for k in support}
    mid_derivative={k:s.Rational(3,2)*w1.get(k,ZERO)+w2.get(k,ZERO) for k in support}
    increment=force(midpoint,mid_derivative)[TARGET][1]-reference[TARGET][1]
    assert increment==-31*s.I/16
    command=[RAD,str(HERE/'construction_why.rad'),'--experimental-laws','--strict-types','--deny-warnings']
    run=subprocess.run(command,capture_output=True,text=True,env={**os.environ,'RAYON_NUM_THREADS':'1'})
    assert run.returncode==0,run.stdout+run.stderr
    rows=[json.loads(line[11:]) for line in run.stdout.splitlines() if line.startswith('diagnostic ')]
    assert [r['lane'] for r in rows]==list(range(6))
    for row in rows:
        lane=row['lane']
        expected=[s.Rational(1,2),0,0,0,0] if lane<3 else repaired_jets if lane<5 else [0]*5
        assert [s.Rational(n,row['denominator']) for n in row['force_jet_n']]==expected
        assert row['seam_closed']==(lane!=3)
        assert row['first_excess_order']==(0 if lane<3 else 1 if lane<5 else -1)
        assert not row['whole_interval_budget_proved']
    assert s.Rational(rows[5]['midpoint_increment_n'],rows[5]['midpoint_denominator'])==increment/s.I
    assert rows[5]['force_reference']=='stationary_background'
    assert 'resolver `AssembleDiagnosis`' in run.stdout and 'law `SubmitDiagnosis`' in run.stdout
    rejected=subprocess.run(command+['--','forge-budget'],capture_output=True,text=True)
    assert rejected.returncode!=0 and 'forged construction diagnosis' in rejected.stdout+rejected.stderr
    with tempfile.TemporaryDirectory(prefix='rad-construction-why-') as temporary:
        trace=Path(temporary)/'diagnosis.radr'
        parallel=subprocess.run(command+['--record',str(trace)],capture_output=True,text=True,
            env={**os.environ,'RAYON_NUM_THREADS':'4'})
        assert parallel.returncode==0 and parallel.stdout==run.stdout
        replay=subprocess.run([RAD,'replay',str(trace)],capture_output=True,text=True)
        assert replay.returncode==0 and 'Replay verified: world digest matches the recorded run' in replay.stderr
    print('PASS: six causal diagnoses; independent exact endpoint jets and midpoint increment -31i/16.')
    print('Upstream dependency, reference-force distinction, forged budget rejection and replay verified.')
    print('The diagnoses concern this repair grammar and specified 1/16 test budget, not impossibility of smooth-forced blowup.')

if __name__=='__main__':
    main()
