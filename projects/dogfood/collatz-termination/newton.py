"""Integer-valued polynomial synthesis with a certified infinite Newton tail.

Each letter is sum(c[i] * binomial(x,i)), with bounded integer c[i].
Rule differences may have negative ordinary or Newton coefficients at zero.
Their finite prefix and all forward differences at a chosen tail origin
are constrained exactly. No sampled inequality is accepted as a proof.
"""

import argparse
from concurrent.futures import ProcessPoolExecutor, as_completed
import hashlib
import json
import time

import z3

from search import HERE, RULES, SYMBOLS


def synthesize(job):
    degree, maximum, tail, reverse, timeout, omitted, *options = job
    engine = options[0] if options else 'values'
    signed = options[1] if len(options)>1 else False
    prune_degrees = options[2] if len(options)>2 else True
    profile = options[3] if len(options)>3 else 'ground'
    if profile not in ('ground','quadratic-digits') or (profile=='quadratic-digits' and (degree!=4 or reverse)):
        raise ValueError('unsupported Newton degree profile')
    if engine not in ('values','expanded','bitvector'):
        raise ValueError('unknown Newton engine')
    if not (1 <= degree <= 4 and 1 <= maximum <= 15 and 0 <= tail <= 32 and -1 <= omitted < 11):
        raise ValueError('Newton search domain')
    bitvector = engine == 'bitvector'
    if bitvector and profile != 'quadratic-digits':
        raise ValueError('bitvector arithmetic bound requires quadratic-digits profile')
    if bitvector:
        from newton_expansion import profile_arithmetic_bound
        upper = profile_arithmetic_bound(maximum,tail,RULES)
        width = upper.bit_length()+2
        literal = lambda x:z3.BitVecVal(x,width)
        variable = lambda name:z3.BitVec(name,width)
    else:
        literal,variable = z3.IntVal,z3.Int
    solver = z3.SolverFor('QF_BV' if bitvector else 'QF_NIA')
    solver.set(timeout=timeout, random_seed=1937)
    letters = {s: [variable(f'c_{s}_{i}') for i in range(degree+1)] for s in SYMBOLS}
    # The terminal boundary is applied only to the ground input. Replacing
    # it by its value at zero preserves every required rule comparison.
    terminal = 'c' if reverse else 'd'
    letters[terminal][1:] = [literal(0)]*degree
    if profile == 'quadratic-digits':
        letters['c'] = [literal(int(i==1)) for i in range(degree+1)]
        for symbol in 'ab':
            letters[symbol][3:] = [literal(0)]*2
            solver.add(letters[symbol][2] > 0)
        for symbol in 'efg':
            solver.add(letters[symbol][4] > 0)
    for vector in letters.values():
        solver.add(*(z3.And(c >= (-maximum if signed else 0), c <= maximum) for c in vector))
        if signed:
            from newton_expansion import ordinary_numerators, polynomial_certificate
            prefix,coefficients = polynomial_certificate(ordinary_numerators(vector[1:]),tail)
            solver.add(vector[0] >= 0, *(v >= 0 for v in prefix+coefficients))
    cache = {}

    def evaluate(word, x):
        if not word:
            return z3.IntVal(x)
        if (word,x) in cache:
            return cache[word,x]
        inner = evaluate(word[1:], x)
        choose = [z3.IntVal(1), inner]
        for i in range(2, degree+1):
            value = z3.Int(f'choose_{word}_{x}_{i}')
            solver.add(value >= 0, i*value == choose[-1]*(inner-i+1))
            choose.append(value)
        value = z3.Int(f'value_{word}_{x}')
        solver.add(value >= 0, value == sum(c*b for c,b in zip(letters[word[0]], choose)))
        cache[word,x] = value
        return value

    strict = {}
    boundary = [0,1] if reverse else [8,9,10]
    if prune_degrees:
        degrees = {}
        for symbol,vector in letters.items():
            value = literal(0)
            for i,c in enumerate(vector[1:],1):
                value = z3.If(c != 0,literal(i),value)
            degrees[symbol] = value
        solver.add(degrees['d' if reverse else 'c'] > 0)

        def word_degree(word):
            result = literal(1)
            for symbol in word:
                result = result*degrees[symbol]
            return result

        for rule,(left,right) in enumerate(RULES):
            if rule != omitted:
                solver.add(word_degree(left) >= word_degree(right))
    if engine != 'values':
        from newton_expansion import rule_certificates
        certify = rule_certificates(letters,degree)
    for rule,(left,right) in enumerate(RULES):
        if rule == omitted:
            continue
        if reverse:
            left,right = left[::-1],right[::-1]
        if engine != 'values':
            prefix,coefficients = certify(left,right,tail)
            solver.add(*(v >= 0 for v in prefix+coefficients))
            if rule in boundary:
                strict[rule] = z3.And([v > 0 for v in prefix+coefficients[:1]])
            continue
        bound = degree ** max(len(left),len(right))
        values = [evaluate(left,x)-evaluate(right,x) for x in range(tail+bound+1)]
        solver.add(*(v >= 0 for v in values[:tail]))
        differences = values[tail:]
        positive = [v > 0 for v in values[:tail+1]]
        while differences:
            solver.add(differences[0] >= 0)
            # Name the differences to keep the saved SMT DAG linear in size.
            after = []
            for i in range(len(differences)-1):
                value = z3.Int(f'delta_{rule}_{len(differences)}_{i}')
                solver.add(value == differences[i+1]-differences[i])
                after.append(value)
            differences = after
        if rule in boundary:
            strict[rule] = z3.And(positive)
    solver.add((z3.And if profile=='quadratic-digits' else z3.Or)(list(strict.values())))
    label = f'newton-{engine}{"-signed" if signed else ""}{"-degrees" if prune_degrees else ""}-{profile}-p{degree}-m{maximum}-h{tail}-r{int(reverse)}-omit{omitted}'
    query = HERE/'out'/f'{label}.smt2'
    query.parent.mkdir(exist_ok=True)
    query.write_text(solver.to_smt2(),encoding='utf-8',newline='\n')
    start = time.perf_counter()
    status = solver.check()
    row = dict(label=label,status=str(status),semiring='newton',engine=engine,signed=signed,degree_pruning=prune_degrees,profile=profile,degree=degree,
               maximum=maximum,tail=tail,reverse=reverse,omitted=omitted,
               timeout_ms=timeout,elapsed_seconds=time.perf_counter()-start,
               full_rule_set=omitted == -1,complete_proof=False,
               query_sha256=hashlib.sha256(query.read_bytes()).hexdigest())
    if bitvector:
        row.update(width=width,absolute_arithmetic_bound=upper)
    if status == z3.sat:
        model = solver.model()
        row['polynomials'] = [[(model.eval(c,model_completion=True).as_signed_long() if bitvector else model.eval(c,model_completion=True).as_long()) for c in letters[s]] for s in SYMBOLS]
        row['strict_rules'] = [i for i,p in strict.items() if z3.is_true(model.eval(p))]
        row['complete_proof'] = omitted == -1 and row['strict_rules'] == boundary
    elif status == z3.unknown:
        row['reason'] = solver.reason_unknown()
    (HERE/'out'/f'{label}.json').write_text(json.dumps(row,indent=2)+'\n',encoding='utf-8',newline='\n')
    return row


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--seconds',type=int,default=90)
    parser.add_argument('--degrees',type=int,nargs='+',default=[2,3])
    parser.add_argument('--maximum',type=int,default=3)
    parser.add_argument('--tails',type=int,nargs='+',default=[0,2,8])
    parser.add_argument('--workers',type=int,default=4)
    parser.add_argument('--engine',choices=['values','expanded','bitvector'],default='expanded')
    parser.add_argument('--signed',action='store_true')
    parser.add_argument('--profile',choices=['ground','quadratic-digits'],default='ground')
    args = parser.parse_args()
    orientations = (False,) if args.profile=='quadratic-digits' else (False,True)
    jobs = [(d,args.maximum,h,r,args.seconds*1000,-1,args.engine,args.signed,True,args.profile)
            for d in args.degrees for h in args.tails for r in orientations]
    if args.engine != 'bitvector':
        jobs.insert(0,(2,3,0,False,args.seconds*1000,3,args.engine,args.signed,True,'ground'))
    rows = []
    with ProcessPoolExecutor(max_workers=args.workers) as pool:
        for future in as_completed([pool.submit(synthesize,job) for job in jobs]):
            row = future.result()
            rows.append(row)
            print(json.dumps(row),flush=True)
    (HERE/'out/newton-campaign.json').write_text(json.dumps(rows,indent=2)+'\n',encoding='utf-8',newline='\n')


if __name__ == '__main__':
    main()
