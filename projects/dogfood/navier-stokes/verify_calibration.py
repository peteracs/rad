"""Audit every fork receipt, the symbolic model, selected full Fourier field and replay."""
from fractions import Fraction as F
from pathlib import Path
import json
import os
import re
import subprocess
import tempfile
import sympy as s
from verify_coupled_modes import nonlinear,pair
from verify_construction_why import force,ZERO
from run_calibration import source,score

HERE = Path(__file__).resolve().parent
OUT = HERE/'calibration_run'
RAD = str(HERE.parents[2]/'target/debug/rad.exe')

def main():
    data=json.loads((HERE/'polarization_polynomials.json').read_text())
    x,y=s.symbols('x y',real=True)
    polynomials={name:sum(s.Integer(coefficient)*x**powers[0]*y**powers[1] for powers,coefficient in info['terms'])
                 for name,info in data.items()}
    waves=[(1,0,0),(0,1,0),(1,1,0),(0,0,1),(0,1,1)]
    polar=[(0,1,1),(1,0,1),(1,-1,x/4),(1,1,0),(y/4,1,-1)]
    d={tuple(sign*t for t in k):(s.Rational(1,2) if i not in [2,4] else -sign*s.I/2)*s.Matrix(p)
       for i,(k,p) in enumerate(zip(waves,polar)) for sign in [-1,1]}
    nd=nonlinear(d)
    r={k:2*v for k,v in nd.items() if sum(t*t for t in k)>2 and v!=ZERO}
    nr=nonlinear(r)
    prod=lambda u,nu:s.expand(sum(sum(t*t for t in k)*s.re(s.conjugate(v).dot(nu.get(k,ZERO))) for k,v in u.items()))
    norm=lambda u,order:s.expand(sum((1+sum(t*t for t in k))**order*s.conjugate(v).dot(v) for k,v in u.items()))
    loss=lambda u:s.expand(sum(sum(t*t for t in k)**2*s.conjugate(v).dot(v) for k,v in u.items()))
    reference=dict(receiver_production=prod(r,nr),receiver_loss=loss(r),receiver_norm=norm(r,0),
                   donor_production=prod(d,nd),donor_loss=loss(d),donor_h11=norm(d,11),donor_h7=norm(d,7),donor_norm=norm(d,0))
    for name,expr in reference.items():
        assert s.expand(polynomials[name]/data[name]['denominator']-expr)==0,name
    def evaluate(name,p,q):
        return sum(coefficient*p**powers[0]*q**powers[1] for powers,coefficient in data[name]['terms'])
    rows=[json.loads(line) for line in (OUT/'candidates.jsonl').read_text().splitlines()]
    keys={(row['p'],row['q'],row['radius_eighths'],row['method']) for row in rows}
    assert len(rows)==len(keys)==9248
    assert keys=={(p,q,b,m) for p in range(-8,9) for q in range(-8,9) for b in range(9,25) for m in [0,1]}
    accepted=[]
    for row in rows:
        p,q=row['p'],row['q']
        donor=evaluate('donor_production',p,q)
        receiver=evaluate('receiver_production',p,q)
        rloss=evaluate('receiver_loss',p,q)
        assert (row['donor_production_n'],row['receiver_production_n'],row['receiver_loss_n'])==(donor,receiver,rloss)
        assert row['finite_transfer_bounds_pass']==(donor>0 and receiver>0)
        assert not row['receiver_autonomous_amplification_reached'] and not row['return_map_proved']
        if not row['finite_transfer_bounds_pass']: continue
        accepted.append(row)
        norm7=F(4096*evaluate('donor_h7',p,q),32)
        M,B,n=row['seed_H7_upper'],row['H7_ball'],row['time_exponent']
        assert (M-1)**2 < norm7 <= M**2
        assert B==(M*row['radius_eighths']+7)//8
        T=F(1,2**n)
        if row['method']==0:
            root_upper=F(1,2**(n//2))
            assert M+32768*T+2048*root_upper*B**2 <= B
            assert 4096*root_upper*B <= F(1,2)
        else:
            assert 2*(3584*B**2+32768)*T <= B-M
        X=B+256*B**2+32768
        C=64*(B+M)*X+256*M**2
        assert C==row['profile_error_coefficient']
        assert C*T<=512 and X*T<=1
        assert evaluate('receiver_norm',p,q)>=1920
        assert 4*(128*M**2+512)*T<=1
        assert F(120*rloss,receiver)>1>2048*T
    assert len(accepted)==640
    assert len({(row['p'],row['q']) for row in accepted})==20
    summary=json.loads((OUT/'summary.json').read_text())
    assert score(summary['best'])==min(map(score,accepted))
    best=summary['best']
    assert (best['p'],best['q'],best['time_exponent'],best['method'])==(6,-1,44,1)
    assert len(list(OUT.glob('why_*.txt')))==summary['rounds']==193
    for path in OUT.glob('why_*.txt'):
        receipt=path.read_text()
        assert re.search(r'resolver `[^`]*ChooseCalibration`',receipt) and re.search(r'law `[^`]*SubmitCalibration`',receipt)

    witness=subprocess.run([RAD,str(HERE/'calibration_witness.rad'),'--strict-types','--deny-warnings'],capture_output=True,text=True)
    assert witness.returncode==0,witness.stdout+witness.stderr
    expected_r={k:v.subs({x:6,y:-1}) for k,v in r.items() if v.subs({x:6,y:-1})!=ZERO}
    actual_r={}; actual_drift={}
    for line in witness.stdout.splitlines():
        if line.startswith('receiver '):
            mode=json.loads(line[9:]); k=tuple(mode['k'])
            actual_r[k]=s.Matrix([s.Rational(a,960)+s.I*s.Rational(b,960) for a,b in zip(mode['re'],mode['im'])])
        if line.startswith('drift '):
            mode=json.loads(line[6:]); k=tuple(mode['k'])
            actual_drift[k]=s.Matrix([s.Rational(a,mode['denominator'])+s.I*s.Rational(b,mode['denominator']) for a,b in zip(mode['re'],mode['im'])])
    assert actual_r==expected_r and actual_drift==force(expected_r,{})
    assert prod(expected_r,nonlinear(expected_r))==s.Rational(193,160)
    assert loss(expected_r)==s.Rational(3565,16)
    bestT=F(1,2**44)
    B=best['H7_ball']; X=B+256*B**2+32768
    lower_rate=99840-(24*1025**2+2*1025)*X*bestT-1025
    assert lower_rate>65536

    batch=[(6,-1,9,1),(6,-1,16,0),(4,4,16,1),(8,0,9,1)]
    launcher=OUT/'verify_launch.rad'
    launcher.write_text(source(batch,194),encoding='utf-8')
    command=[RAD,str(launcher),'--experimental-laws','--strict-types','--deny-warnings']
    one=subprocess.run(command,capture_output=True,text=True,env={**os.environ,'RAYON_NUM_THREADS':'1'})
    assert one.returncode==0,one.stdout+one.stderr
    with tempfile.TemporaryDirectory(prefix='rad-calibration-replay-') as temporary:
        trace=Path(temporary)/'calibration.radr'
        four=subprocess.run(command+['--record',str(trace)],capture_output=True,text=True,env={**os.environ,'RAYON_NUM_THREADS':'4'})
        assert four.returncode==0 and four.stdout==one.stdout
        replay=subprocess.run([RAD,'replay',str(trace)],capture_output=True,text=True)
        assert replay.returncode==0 and 'Replay verified: world digest matches the recorded run' in replay.stderr
    launcher.write_text(source(batch,194,forge=True),encoding='utf-8')
    rejected=subprocess.run(command,capture_output=True,text=True)
    assert rejected.returncode!=0 and 'forged calibrated return map' in rejected.stdout+rejected.stderr
    launcher.write_text(source(batch,194),encoding='utf-8')
    audit=dict(checked=9248,rounds=193,accepted=640,positive_shapes=20,best=best,
        selected_receiver_production='193/160',selected_receiver_loss='3565/16',
        selected_activation_threshold='35650/193',selected_time='2^-44',
        selected_enstrophy_rate_lower=str(lower_rate),return_map_proved=False,
        validation='symbolic model, all receipts, full Fourier witness, deterministic forks, replay and forged-map rejection pass')
    (OUT/'audit.json').write_text(json.dumps(audit,indent=2)+'\n',encoding='utf-8')
    print('PASS: complete symbolic model, all 9,248 candidate receipts, 193 WHY rounds and selected full Fourier witness.')
    print('PASS: energy/existence/profile-error bounds, 1/4-worker determinism, replay and forged-return-map rejection.')
    print('Selected finite-transfer interval 2^-44; isolated receiver activation and full return-map closure remain unproved.')

if __name__=='__main__':
    main()
