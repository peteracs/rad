"""Exact bivariate force identities, trajectory joins and causal search checks."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as sy

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
RAD=str(ROOT/'target/debug/rad.exe')
s,z,v=sy.symbols('s z v',real=True)
zero=sy.zeros(3,1)

def paths(stage,part):
    out={}
    for degree,bank in enumerate(stage[part]):
        for mode in bank:
            k=tuple(mode['k'])
            coeff=sy.Matrix([sy.Rational(a,2)+sy.I*sy.Rational(b,2)
                for a,b in zip(mode['re'],mode['im'])])
            out[k]=out.get(k,zero)+s**degree*coeff
    return out

def main():
    run=subprocess.run([RAD,str(HERE/'trajectory_check.rad'),'--strict-types','--deny-warnings'],
        capture_output=True,text=True)
    assert run.returncode==0,run.stdout+run.stderr
    stages=[json.loads(line[6:]) for line in run.stdout.splitlines() if line.startswith('stage ')]
    rows=[json.loads(line[6:]) for line in run.stdout.splitlines() if line.startswith('force ')]
    assert len(stages)==4
    for index,stage in enumerate(stages):
        a,b=paths(stage,'a'),paths(stage,'b')
        h,nu=stage['inverse_duration'],stage['viscosity']
        u={k:a[k]+z*b[k] for k in a}
        nonlinear={}
        for p,up in u.items():
            for q,uq in u.items():
                k=tuple(p[j]+q[j] for j in range(3))
                nonlinear[k]=nonlinear.get(k,zero)+sy.I*up.dot(sy.Matrix(q))*uq
        actual={}
        for row in rows:
            if row['stage']!=index: continue
            mode=row['force']; k=tuple(mode['k'])
            assert k not in actual
            poly=sy.zeros(3,1)
            size=0
            for i in range(5):
                for j in range(3):
                    for part in range(3):
                        r,im=mode['re'][i][j][part],mode['im'][i][j][part]
                        size+=abs(r)+abs(im)
                        poly[part]+=s**i*z**j*(r+sy.I*im)/mode['denominator']
            assert size==mode['coefficient_l1_n']
            actual[k]=poly
        assert actual.keys()==set(u)|set(nonlinear),'complete force support'
        for k,polynomial in actual.items():
            wave=sy.Matrix(k); norm=sum(t*t for t in k)
            adv=nonlinear.get(k,zero)
            projected=adv-(wave*wave.dot(adv)/norm if norm else zero)
            expected=h*sy.diff(u.get(k,zero),s)+projected+nu*norm*u.get(k,zero)
            assert all(sy.expand(x)==0 for x in polynomial-expected),(index,k)
            # Restore the switching derivative; v stands for theta'(s).
            full_derivative=h*(sy.diff(u.get(k,zero),s)+v*b.get(k,zero))
            assert all(sy.expand(x)==0 for x in
                polynomial+h*v*b.get(k,zero)-full_derivative-projected-nu*norm*u.get(k,zero))
            assert sy.expand(wave.dot(polynomial))==0
        # Exact endpoint energy and all-interval monotonicity of this fixture.
        enstrophy=sy.expand(sum(sum(t*t for t in k)*sy.conjugate(uk).dot(uk)/2 for k,uk in u.items()))
        for derivative in [sy.diff(enstrophy,s),sy.diff(enstrophy,z)]:
            assert all(c>=0 for c in sy.Poly(derivative,s,z).coeffs())
    for li,ri in [(0,1),(2,3)]:
        left,right=stages[li],stages[ri]
        la,lb,ra=paths(left,'a'),paths(left,'b'),paths(right,'a')
        ratio=sy.Rational(left['inverse_duration'],right['inverse_duration'])
        for k in ra:
            assert all(sy.expand(x)==0 for x in (la[k]+lb[k]).subs(s,1+ratio*s)-ra[k]),'full endpoint germ'
    command=[RAD,str(HERE/'trajectory_search.rad'),'--experimental-laws','--strict-types','--deny-warnings']
    run=subprocess.run(command,capture_output=True,text=True,env={**os.environ,'RAYON_NUM_THREADS':'1'})
    assert run.returncode==0,run.stdout+run.stderr
    evidence=[json.loads(line[9:]) for line in run.stdout.splitlines() if line.startswith('evidence ')]
    assert len(evidence)==2
    assert [r['seam_closed'] for r in evidence]==[False,True]
    assert evidence[1]==dict(final_e8=292,finite_force_smooth=True,infinite_budget_proved=False,
        initial_e8=8,lane=1,left_force_bound=282,right_force_bound=340,seam_closed=True)
    assert 'resolver `ChooseTrajectory`' in run.stdout and 'law `SubmitTrajectory`' in run.stdout
    rejected=subprocess.run(command+['--','forge-infinite'],capture_output=True,text=True)
    assert rejected.returncode!=0 and 'forged trajectory certificate' in rejected.stdout+rejected.stderr
    with tempfile.TemporaryDirectory(prefix='rad-trajectory-') as temporary:
        trace=Path(temporary)/'trajectory.radr'
        parallel=subprocess.run(command+['--record',str(trace)],capture_output=True,text=True,
            env={**os.environ,'RAYON_NUM_THREADS':'4'})
        assert parallel.returncode==0 and parallel.stdout==run.stdout
        replay=subprocess.run([RAD,'replay',str(trace)],capture_output=True,text=True)
        assert replay.returncode==0 and 'Replay verified: world digest matches the recorded run' in replay.stderr
    print(f'PASS: {len(stages)} trajectory stages, {len(rows)} complete bivariate Fourier force polynomials.')
    print('Switching derivative retained; exact endpoint germs and whole-interval monotone enstrophy verified.')
    print('Finite trajectory selection, forged infinite proof rejection, deterministic forks and replay pass.')
    print('All-order smoothness uses the documented fixed-step analytic lemma; no infinite-stage budget proved.')

if __name__=='__main__':
    main()
