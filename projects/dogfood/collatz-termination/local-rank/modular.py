"""Ask whether weights depending on the entire prefix modulo m can rank."""

from collections import Counter
import json
import time

import z3

from investigate import DIGITS, HERE, OUT, digest, linear_dual, require, run, save

LEFT = [[0], [1], [0,4], [0,5], [0,6], [1,4], [1,5], [1,6], [4], [5], [6]]
RIGHT = [[], [6], [4,0], [4,1], [5,0], [5,1], [6,0], [6,1], [1], [0,0], [0,1]]


def independent(row, modulus, boundary):
    rule, residue, symbol = row['rule'], row['residue'], row['symbol']
    require(1 <= modulus <= 256 and 0 <= residue < modulus and -1 <= rule < 11 and 0 < boundary < 2048, 'modular domain')
    def advance(state, digit):
        require(digit in DIGITS, 'invalid digit')
        return (2*state+digit if digit<2 else 3*state+digit-4) % modulus
    def cost(word):
        state = residue
        coefficients = Counter()
        for digit in word:
            coefficients[f'w:{state}:{digit}'] += 1
            state = advance(state, digit)
        coefficients[f't:{state}'] += 1
        return coefficients, state
    if rule == -1:
        result = Counter({f'w:{residue}:{symbol}': 1})
        result[f'h:{residue}'] += 1
        result[f'h:{advance(residue,symbol)}'] -= 1
    else:
        require(rule < 8 or residue == 1 % modulus, 'wrong leading state')
        result, left_end = cost(LEFT[rule])
        other, right_end = cost(RIGHT[rule])
        require(rule < 2 or left_end == right_end, 'residue changed on conversion')
        result.subtract(other)
    return {k:v for k,v in result.items() if v}, int(rule>=0 and bool(boundary & (1<<rule)))


def verify(proof):
    totals = Counter(); right = 0
    for term in proof['rows']:
        weight = term['multiplier']
        require(type(weight) is int and 0 < weight <= 10**9, 'invalid modular multiplier')
        coefficients, bound = independent(term['constraint'], proof['modulus'], proof['boundary'])
        for k,v in coefficients.items(): totals[k] += weight*v
        right += weight*bound
    require(right > 0 and all(v==0 for v in totals.values()), 'invalid modular dual')
    return right


def investigate(modulus, boundary=1792):
    name=f'modular-{modulus}-b{boundary}'
    output, elapsed=run([HERE/'generate_modular.rad','--strict-types','--deny-warnings','--',modulus,boundary],name)
    rows=[json.loads(line) for line in output.stdout.splitlines()]
    for row in rows:
        terms,bound=independent(row,modulus,boundary)
        require(terms=={k:v for k,v in row['terms'].items() if v} and bound==row['bound'],'modular generators differ')
    variables={k:z3.Real(k) for row in rows for k,v in row['terms'].items() if v}
    solver=z3.SolverFor('QF_LRA');solver.set(timeout=30000,unsat_core=True)
    for i,row in enumerate(rows):
        expression=sum((v*variables[k] for k,v in row['terms'].items() if v),z3.RealVal(0))
        solver.assert_and_track(expression>=row['bound'],f'row_{i}')
    formula=solver.to_smt2().replace('(check-sat)','\n'.join(f'(assert row_{i})' for i in range(len(rows)))+'\n(check-sat)')
    query=OUT/f'{name}.smt2';query.write_text(formula,encoding='utf-8',newline='\n')
    start=time.perf_counter();status=solver.check()
    result=dict(modulus=modulus,boundary=boundary,constraints=len(rows),variables=len(variables),status=str(status),
                solver_seconds=time.perf_counter()-start,generation_seconds=elapsed,query_sha256=digest(query),collatz_proved=False)
    if status==z3.unsat:
        selected=[int(str(label).split('_')[1]) for label in solver.unsat_core()]
        proof=dict(modulus=modulus,boundary=boundary,rows=linear_dual(rows,selected,30),collatz_proved=False)
        verify(proof);source=save(f'{name}-dual.json',proof)
        checked,_=run([HERE/'check_modular.rad','--strict-types','--deny-warnings','--',source],f'{name}-checked')
        require(json.loads(checked.stdout)['contradiction']==verify(proof),'modular checkers differ')
        result.update(dual_size=len(proof['rows']),independently_verified=True,dual_sha256=digest(source))
    elif status==z3.sat:
        result['candidate']={k:str(solver.model().eval(v,model_completion=True)) for k,v in variables.items()}
    else:result['reason']=solver.reason_unknown()
    save(f'{name}-result.json',result);print(json.dumps(result),flush=True)
    return result


if __name__=='__main__':
    for modulus in [2,3,5,7,15,31,127]:investigate(modulus)
