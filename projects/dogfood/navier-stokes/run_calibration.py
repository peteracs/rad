"""Adaptive batches of isolated RAD forks, followed by exhaustive finite-box coverage."""
from fractions import Fraction
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess
import time

HERE = Path(__file__).resolve().parent
RAD = str(HERE.parents[2]/'target/debug/rad.exe')
OUT = HERE/'calibration_run'

def score(row):
    if not row['finite_transfer_bounds_pass']:
        return (10**9,Fraction(10**9))
    return row['time_exponent'],Fraction(row['receiver_loss_n'],row['receiver_production_n'])

def source(batch,round_id,forge=False):
    lines=['use "../calibration_round.rad"','fn main() -> nil {','    let requests: list<CalibrationRequest> = [']
    lines += [f'        CalibrationRequest {{ round:{round_id},lane:{i},p:{p},q:{q},radius_eighths:{r},method:{method} }}'+(',' if i+1<len(batch) else '')
              for i,(p,q,r,method) in enumerate(batch)]
    return '\n'.join(lines+['    ]',f'    run_calibration_round(requests,{str(forge).lower()})','}'])+'\n'

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--max-rounds',type=int,default=200)
    parser.add_argument('--resume',action='store_true')
    args=parser.parse_args()
    OUT.mkdir(exist_ok=True)
    if not (OUT/'manifest.json').exists():
        files=['calibration_round.rad','calibration_bounds.rad','polarization_polynomials.rad','polarization_polynomials.json']
        manifest={name:hashlib.sha256((HERE/name).read_bytes()).hexdigest() for name in files}
        manifest['rad_executable_sha256']=hashlib.sha256(Path(RAD).read_bytes()).hexdigest()
        (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
    prior=[]
    if args.resume and (OUT/'candidates.jsonl').exists():
        prior=[json.loads(line) for line in (OUT/'candidates.jsonl').read_text().splitlines()]
    elif (OUT/'candidates.jsonl').exists():
        raise SystemExit('Existing calibration receipts: use --resume or preserve them before a fresh run.')
    keys=lambda row:(row['p'],row['q'],row['radius_eighths'],row['method'])
    unseen={(p,q,r,m) for p in range(-8,9) for q in range(-8,9) for r in range(9,25) for m in [0,1]}
    for row in prior: unseen.discard(keys(row))
    best=min(prior,key=score) if prior else None
    round_id=len(list(OUT.glob('why_*.txt')))
    started=time.monotonic()
    records=OUT/'candidates.jsonl'
    summary=dict(rounds=round_id,checked=9248-len(unseen),remaining=len(unseen),best=best,
                 elapsed_this_invocation_seconds=0,return_map_proved=False)
    while unseen and round_id<args.max_rounds:
        # WHY's limiting estimate steers local priority; one quarter explores unseen shapes.
        if best and best['finite_transfer_bounds_pass']:
            p0,q0,r0,m0=keys(best)
            prefer_small_radius=best['limiting_bound']=='receiver relative profile error'
            ranked=sorted(unseen,key=lambda x:(abs(x[0]-p0)+abs(x[1]-q0),x[3]!=m0,
                x[2] if prefer_small_radius else abs(x[2]-r0),x))
        else:
            ranked=sorted(unseen,key=lambda x:(abs(x[0]-4)+abs(x[1]-4),abs(x[2]-16),x[3],x))
        batch=ranked[:36]
        available=unseen-set(batch)
        batch+=sorted(available,key=lambda x:(x[0],x[1],x[3],x[2]))[:12]
        round_id+=1
        launcher=OUT/'launch.rad'
        launcher.write_text(source(batch,round_id),encoding='utf-8')
        command=[RAD,str(launcher),'--experimental-laws','--strict-types','--deny-warnings']
        run=subprocess.run(command,capture_output=True,text=True,env={**os.environ,'RAYON_NUM_THREADS':'4'})
        if run.returncode:
            (OUT/f'error_{round_id:03}.txt').write_text(run.stdout+run.stderr,encoding='utf-8')
            raise RuntimeError(run.stdout+run.stderr)
        rows=[json.loads(line[10:]) for line in run.stdout.splitlines() if line.startswith('candidate ')]
        assert len(rows)==len(batch) and [keys(row) for row in rows]==batch
        why='\n'.join(line for line in run.stdout.splitlines() if not line.startswith('candidate '))
        (OUT/f'why_{round_id:03}.txt').write_text(why+'\n',encoding='utf-8')
        with records.open('a',encoding='utf-8') as stream:
            for row in rows: stream.write(json.dumps(row,sort_keys=True)+'\n')
        for row in rows:
            unseen.remove(keys(row))
            if best is None or score(row)<score(best):
                best=row
                print(json.dumps(dict(round=round_id,new_best=best)),flush=True)
        if round_id%20==0: print(json.dumps(dict(round=round_id,checked=9248-len(unseen),remaining=len(unseen))),flush=True)
        summary=dict(rounds=round_id,checked=9248-len(unseen),remaining=len(unseen),best=best,
                     elapsed_this_invocation_seconds=round(time.monotonic()-started,3),return_map_proved=False)
        (OUT/'summary.json').write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(summary),flush=True)

if __name__=='__main__':
    main()
