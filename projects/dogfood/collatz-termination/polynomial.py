"""Search nonlinear scalar interpretations of the complete rewriting system."""

import argparse
from concurrent.futures import ProcessPoolExecutor, as_completed
import hashlib
import json
from pathlib import Path
import time

import z3

from search import HERE, RULES, SYMBOLS


def synthesize(job):
    degree, maximum, reverse, timeout, engine = job
    if not (2 <= degree <= 4 and 1 <= maximum <= 7):
        raise ValueError('polynomial search domain')
    # Evaluation at x=1 bounds every nonnegative coefficient and every
    # intermediate sum in all words of length at most three.
    upper = 1
    for _ in range(3):
        upper = maximum * sum(upper ** k for k in range(degree+1))
    if upper >= 2 ** 62:
        raise ValueError('certificate would exceed RAD exact integer domain')
    width = upper.bit_length()+1
    integer = engine == 'integer'
    zero = z3.IntVal(0) if integer else z3.BitVecVal(0, width)
    one = z3.IntVal(1) if integer else z3.BitVecVal(1, width)
    solver = z3.SolverFor('QF_NIA' if integer else 'QF_BV')
    solver.set(timeout=timeout, random_seed=1937)
    letters = {s: [(z3.Int(f'{s}_{i}') if integer else z3.BitVec(f'{s}_{i}', width)) for i in range(degree+1)] for s in SYMBOLS}
    for vector in letters.values():
        solver.add(*((z3.And(v >= 0, v <= maximum) if integer else z3.ULE(v, maximum)) for v in vector))

    def mul(a,b):
        answer = [zero for _ in range(len(a)+len(b)-1)]
        for i,x in enumerate(a):
            for j,y in enumerate(b):
                answer[i+j] = answer[i+j]+x*y
        return [z3.simplify(x) for x in answer]

    def compose(a,b):
        answer = [zero]
        for coefficient in a[::-1]:
            answer = mul(answer,b)
            answer[0] = answer[0]+coefficient
        return [z3.simplify(x) for x in answer[:(len(a)-1)*(len(b)-1)+1]]

    def interpret(word):
        answer = [zero,one]
        for letter in word[::-1] if reverse else word:
            answer = compose(answer,letters[letter])
        return answer

    boundary = [0,1] if reverse else [8,9,10]
    strict = {}
    for i,(left,right) in enumerate(RULES):
        a,b=interpret(left),interpret(right)
        size=max(len(a),len(b)); a += [zero]*(size-len(a)); b += [zero]*(size-len(b))
        solver.add(*((x >= y if integer else z3.UGE(x,y)) for x,y in zip(a,b)))
        if i in boundary: strict[i] = a[0] > b[0] if integer else z3.UGT(a[0],b[0])
    solver.add(z3.Or(list(strict.values())))
    label=f'polynomial-{engine}-p{degree}-m{maximum}-r{int(reverse)}'
    query=HERE/'out'/f'{label}.smt2'
    query.parent.mkdir(exist_ok=True)
    query.write_text(solver.to_smt2(),encoding='utf-8',newline='\n')
    start=time.perf_counter(); status=solver.check()
    row=dict(label=label,status=str(status),semiring='polynomial',omitted=-1,degree=degree,maximum=maximum,reverse=reverse,engine=engine,
             width=width,intermediate_upper=upper,timeout_ms=timeout,
             elapsed_seconds=time.perf_counter()-start,full_rule_set=True,complete_proof=False,
             query_sha256=hashlib.sha256(query.read_bytes()).hexdigest())
    if status==z3.sat:
        model=solver.model()
        row['polynomials']=[[model.eval(c,model_completion=True).as_long() for c in letters[s]] for s in SYMBOLS]
        row['strict_rules']=[i for i in boundary if z3.is_true(model.eval(strict[i]))]
        row['complete_proof']=row['strict_rules']==boundary
    elif status==z3.unknown: row['reason']=solver.reason_unknown()
    (HERE/'out'/f'{label}.json').write_text(json.dumps(row,indent=2)+'\n',encoding='utf-8',newline='\n')
    return row


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--seconds',type=int,default=90)
    parser.add_argument('--engine',choices=['integer','bitvector'],default='integer')
    args=parser.parse_args()
    jobs=[(degree,maximum,reverse,args.seconds*1000,args.engine)
          for degree,maximum in ((2,3),(3,3),(4,2)) for reverse in (False,True)]
    rows=[]
    with ProcessPoolExecutor(max_workers=4) as pool:
        for future in as_completed([pool.submit(synthesize,job) for job in jobs]):
            row=future.result();rows.append(row);print(json.dumps(row),flush=True)
    (HERE/'out/polynomial-campaign.json').write_text(json.dumps(rows,indent=2)+'\n',encoding='utf-8',newline='\n')


if __name__=='__main__': main()
