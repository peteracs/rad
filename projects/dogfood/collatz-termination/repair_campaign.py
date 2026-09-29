"""Prepare RAD search lanes; capture outputs without taking over the search."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import time

from verify import RULES,known_natural_control,require

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
OUT=HERE/'out'
RAD=ROOT/'target/release/rad.exe'


def request(dimension=2,reverse=False,target=1792,style=0,trials=20000):
    base=known_natural_control()['matrices']
    d=dimension
    matrices=[]
    for source in base:
        a=[[0]*(d+1) for _ in range(d+1)]
        a[d][d]=1
        for i in range(2):
            for j in range(2): a[i][j]=source[i][j]
            a[i][d]=source[i][2]
        for i in range(2,d): a[i][i]=1
        matrices.append(a)
    ground=2 if reverse else 3
    for i in range(d):
        for j in range(d): matrices[ground][i][j]=0
    return dict(dimension=d,maximum=7 if style==0 else 15,matrices=matrices,
                rules=[[[ord(c)-97 for c in l],[ord(c)-97 for c in r]] for l,r in RULES],
                active_mask=2047,strict_mask=target,reverse=reverse,ground=ground,
                seed=1937+d*100003+target*11+style*15331+int(reverse),trials=trials,
                strict_weight=8 if style==0 else 128,temperature=8 if style==0 else 128,
                stop_on_zero=True,full_rescore=False)


def configuration(rounds,trials,dimensions):
    lanes=[]
    for d in dimensions:
        for reverse in (False,True):
            for target in ([1,2,3] if reverse else [256,512,1024,1792]):
                for style in (0,1):
                    lanes.append(dict(label=f'd{d}-r{int(reverse)}-s{target}-v{style}',omitted=-1,
                                      request=request(d,reverse,target,style,trials)))
    return dict(rounds=rounds,lanes=lanes)


def run(config,name,workers=4,record=False):
    OUT.mkdir(exist_ok=True)
    source=OUT/f'{name}.json'
    source.write_text(json.dumps(config,indent=2)+'\n',encoding='utf-8',newline='\n')
    args=[str(RAD),str(HERE/'repair.rad'),'--strict-types','--deny-warnings','--experimental-laws']
    if record: args+=['--record',str(OUT/f'{name}.radr')]
    args+=['--',str(source)]
    start=time.perf_counter()
    count=0
    with (OUT/f'{name}.stdout').open('wb') as output,(OUT/f'{name}.stderr').open('wb') as errors:
        process=subprocess.Popen(args,cwd=ROOT,stdout=subprocess.PIPE,stderr=errors,
                                 env={**os.environ,'RAYON_NUM_THREADS':str(workers)})
        for line in process.stdout:
            output.write(line)
            row=json.loads(line)
            if row['kind']=='round':
                print(json.dumps(row),flush=True)
                count=row['evaluated']
        code=process.wait()
    require(code==0,(OUT/f'{name}.stderr').read_text(encoding='utf-8'))
    result=dict(elapsed_seconds=time.perf_counter()-start,evaluated=count,workers=workers)
    (OUT/f'{name}-timing.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(result),flush=True)
    return result


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--rounds',type=int,default=16)
    parser.add_argument('--trials',type=int,default=20000)
    parser.add_argument('--dimensions',type=int,nargs='+',default=[2,3,4,6,8])
    parser.add_argument('--workers',type=int,default=4)
    parser.add_argument('--name',default='repair-campaign')
    args=parser.parse_args()
    run(configuration(args.rounds,args.trials,args.dimensions),args.name,args.workers)


if __name__=='__main__':main()
