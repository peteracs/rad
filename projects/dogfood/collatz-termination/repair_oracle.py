"""Unbounded-integer oracle for native repair reports and targeted SMT jobs."""

import argparse
from concurrent.futures import ProcessPoolExecutor,as_completed
import json

from search import HERE,synthesize
from verify import check,require


def diagnostics(request,matrices):
    d=request['dimension']
    def word(text):
        result=[[int(i==j) for j in range(d+1)] for i in range(d+1)]
        for s in text[::-1] if request['reverse'] else text:
            a=matrices[s]
            result=[[sum(result[i][k]*a[k][j] for k in range(d+1)) for j in range(d+1)] for i in range(d+1)]
        return result
    weak,strict=0,0
    failures=[]
    strict_rules=[]
    for number,(left,right) in enumerate(request['rules']):
        if not request['active_mask'] & (1<<number):continue
        a,b=word(left),word(right)
        for i in range(d):
            for j in range(d+1):
                deficit=max(0,b[i][j]-a[i][j])
                weak+=deficit
                if deficit:failures.append(dict(rule=number,row=i,column=j,deficit=deficit))
        boundary=number in ([0,1] if request['reverse'] else [8,9,10])
        if boundary and a[0][d]>b[0][d]:strict_rules.append(number)
        if request['strict_mask'] & (1<<number):strict+=max(0,1+b[0][d]-a[0][d])
    return dict(score=dict(weak=weak,strict=strict,total=weak+request['strict_weight']*strict),
                failures=failures,strict_rules=strict_rules)


def audit_report(row):
    request,report=row['request'],row['report']
    answer=diagnostics(request,report['matrices'])
    require(answer['score']==report['best'],'best native score differs')
    require(diagnostics(request,request['matrices'])['score']==report['initial'],'initial native score differs')
    require(report['seed']==request['seed']*pow(48271,3*report['evaluated'],2147483647)%2147483647,'RNG advancement differs')
    require(0<=report['accepted']<=report['evaluated']<=request['trials'],'trial accounting differs')
    if answer['score']['total']==0:
        boundary=[i for i in ([0,1] if request['reverse'] else [8,9,10]) if i!=row['omitted']]
        model=dict(label=row['label'],semiring='natural',dimension=request['dimension'],maximum=request['maximum'],
                   denominator=1,reverse=request['reverse'],omitted=row['omitted'],matrices=report['matrices'],
                   strict_rules=answer['strict_rules'],complete_proof=row['omitted']==-1 and answer['strict_rules']==boundary)
        check(model)
    return answer


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--source',default='repair-campaign.stdout')
    parser.add_argument('--seconds',type=int,default=30)
    args=parser.parse_args()
    rows=[json.loads(line) for line in (HERE/'out'/args.source).read_text(encoding='utf-8').splitlines()]
    latest={}
    for row in rows:
        if row['kind']=='lane':
            row['diagnostics']=audit_report(row)
            latest[row['label']]=row
    candidates=sorted((r for r in latest.values() if r['request']['dimension']<=4),
                      key=lambda r:(r['report']['best']['strict']>0,r['report']['best']['total'],r['label']))
    selected=[]
    groups=set()
    for row in candidates:
        group=(row['request']['dimension'],row['request']['reverse'])
        if group not in groups:
            groups.add(group);selected.append(row)
    (HERE/'out/repair-finalists.json').write_text(json.dumps(selected,indent=2)+'\n',encoding='utf-8')
    jobs=[]
    for row in selected:
        req=row['request']
        for radius in (4,12):
            repair=dict(matrices=row['report']['matrices'],radius=radius,strict_mask=req['strict_mask'])
            jobs.append(('natural',req['dimension'],req['maximum'],req['reverse'],args.seconds*1000,-1,1,True,repair))
    with ProcessPoolExecutor(max_workers=4) as pool:
        for future in as_completed([pool.submit(synthesize,job) for job in jobs]):
            result=future.result()
            print(json.dumps({k:v for k,v in result.items() if k not in ('matrices','repair')}),flush=True)


if __name__=='__main__':main()
