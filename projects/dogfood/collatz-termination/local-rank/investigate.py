"""Question -> RAD inequalities -> exact solve -> checked dual obstruction."""

import argparse
from collections import Counter
from datetime import datetime, timezone
from fractions import Fraction
import hashlib
import itertools
import json
import math
import os
from pathlib import Path
import subprocess
import time

import z3

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
OUT = HERE / 'out'
RAD = ROOT / 'target/release/rad.exe'
DIGITS = (0, 1, 4, 5, 6)
RULES = [('ad', 'd'), ('bd', 'gd'), ('ae', 'ea'), ('af', 'eb'), ('ag', 'fa'),
         ('be', 'fb'), ('bf', 'ga'), ('bg', 'gb'), ('ce', 'cb'), ('cf', 'caa'), ('cg', 'cab')]


def require(condition, message):
    if not condition: raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(name, value):
    path = OUT / name
    path.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8', newline='\n')
    return path


def run(args, name, workers=4, success=True, timeout=120):
    OUT.mkdir(exist_ok=True)
    start = time.perf_counter()
    result = subprocess.run([str(RAD), *map(str, args)], cwd=ROOT, capture_output=True,
                            timeout=timeout, env={**os.environ, 'RAYON_NUM_THREADS': str(workers)})
    (OUT / f'{name}.stdout').write_bytes(result.stdout)
    (OUT / f'{name}.stderr').write_bytes(result.stderr)
    require((result.returncode == 0) == success, result.stderr.decode('utf-8', errors='replace'))
    return result, time.perf_counter() - start


def key(word):
    code = 1
    for symbol in word: code = 8 * code + symbol
    return str(code)


def counts(word, memory):
    return Counter(key(word[i:i+length]) for length in range(1, memory+1)
                   for i in range(len(word)-length+1))


def independent_row(row, memory, boundary):
    if row['kind'] == 0:
        rule = row['rule']
        require(0 <= rule < 11, 'invalid rewrite rule')
        before, after = row['before'], row['after']
        require(all(x in DIGITS for x in before + after), 'invalid context digit')
        require(rule < 8 or not before, 'invalid left context')
        require(rule >= 2 or not after, 'invalid right context')
        left, right = ([ord(c)-97 for c in word] for word in RULES[rule])
        prefix = [] if rule >= 8 else [2] + before
        suffix = [] if rule < 2 else after + [3]
        result = counts(prefix + left + suffix, memory)
        result.subtract(counts(prefix + right + suffix, memory))
        bound = int(bool(boundary & (1 << rule)))
    else:
        require(row['kind'] == 1, 'invalid row kind')
        state, symbol = row['state'], row['symbol']
        require(len(state) == memory-1 and all(x in DIGITS for x in state) and symbol in DIGITS, 'invalid suffix edge')
        word = state + [symbol]
        result = Counter(key(word[i:]) for i in range(len(word)))
        result['h:' + key(state)] += 1
        result['h:' + key(word[1:])] -= 1
        bound = 0
    return {k: v for k, v in result.items() if v}, bound


def verify_dual(proof):
    totals = Counter()
    bound = 0
    require(1 <= proof['memory'] <= 6 and 0 < proof['boundary'] < 2048, 'invalid proof domain')
    for term in proof['rows']:
        weight = term['multiplier']
        require(type(weight) is int and 0 < weight <= 10**9, 'invalid dual multiplier')
        coefficients, right = independent_row(term['constraint'], proof['memory'], proof['boundary'])
        for k, v in coefficients.items(): totals[k] += weight * v
        bound += weight * right
    require(all(v == 0 for v in totals.values()) and bound > 0, 'invalid dual identity')
    return bound


def linear_dual(rows, selected, seconds):
    used = [rows[i] for i in selected]
    variables = sorted({k for row in used for k, v in row['terms'].items() if v})
    weights = [z3.Real(f'dual_{i}') for i in range(len(used))]
    dual = z3.SolverFor('QF_LRA')
    dual.set(timeout=seconds*1000)
    dual.add(*(w >= 0 for w in weights))
    for k in variables:
        dual.add(z3.Sum([w * row['terms'][k] for w, row in zip(weights, used) if row['terms'].get(k, 0)]) == 0)
    dual.add(z3.Sum([w * row['bound'] for w, row in zip(weights, used) if row['bound']]) == 1)
    require(dual.check() == z3.sat, 'could not reconstruct an exact dual witness')
    model = dual.model()
    values = [Fraction(model.eval(w).numerator_as_long(), model.eval(w).denominator_as_long())
              for w in weights]
    denominator = math.lcm(*(v.denominator for v in values))
    integers = [int(v * denominator) for v in values]
    common = math.gcd(*integers)
    return [dict(multiplier=w//common, constraint=row) for w, row in zip(integers, used) if w]


def exact_dual(rows, selected, memory, boundary, seconds):
    proof = dict(memory=memory, boundary=boundary, rows=linear_dual(rows, selected, seconds), collatz_proved=False)
    verify_dual(proof)
    return proof


def investigate(memory, boundary, seconds, context_depth=None, extra_rows=None, suffix='', omitted=-1):
    depth = memory - 1 if context_depth is None else context_depth
    name = (f'm{memory}-b{boundary}' if depth == memory-1 else f'm{memory}-c{depth}-b{boundary}') + suffix
    require(-1 <= omitted < 11, 'invalid omitted rule')
    if omitted >= 0: name += f'-omit{omitted}'
    generated, generation_time = run([HERE / 'generate.rad', '--strict-types', '--deny-warnings',
                                      '--', memory, boundary, depth], name, timeout=180)
    rows = [json.loads(line) for line in generated.stdout.splitlines()]
    rows += extra_rows or []
    rows = [row for row in rows if row['kind'] != 0 or row['rule'] != omitted]
    for row in rows:
        coefficients, bound = independent_row(row, memory, boundary)
        require(coefficients == {k: v for k, v in row['terms'].items() if v} and bound == row['bound'], 'RAD inequalities differ')
    symbols = sorted({k for row in rows for k, v in row['terms'].items() if v})
    variables = {k: z3.Real(f'v_{k.replace(":", "_")}') for k in symbols}
    solver = z3.SolverFor('QF_LRA')
    solver.set(timeout=seconds*1000, unsat_core=True)
    labels = []
    for i, row in enumerate(rows):
        expression = sum((v * variables[k] for k, v in row['terms'].items() if v), z3.RealVal(0))
        label = z3.Bool(f'row_{i}')
        labels.append(label)
        solver.assert_and_track(expression >= row['bound'], label)
    # SMT-LIB must assert the tracking literals; to_smt2 alone does not encode
    # the assumptions which assert_and_track supplies to the in-process check.
    formula = solver.to_smt2().replace('(check-sat)', '\n'.join(f'(assert {label})' for label in labels) + '\n(check-sat)')
    query = OUT / f'{name}.smt2'
    query.write_text(formula, encoding='utf-8', newline='\n')
    start = time.perf_counter()
    status = solver.check()
    result = dict(memory=memory, context_depth=depth, complete_contexts=depth==memory-1, omitted=omitted,
                  boundary=boundary, constraints=len(rows), variables=len(symbols),
                  status=str(status), generation_seconds=generation_time, solver_seconds=time.perf_counter()-start,
                  query_sha256=digest(query), collatz_proved=False)
    if extra_rows: result['added_constraints'] = len(extra_rows)
    if status == z3.unsat:
        core = sorted(int(str(label).split('_')[1]) for label in solver.unsat_core())
        proof = exact_dual(rows, core, memory, boundary, seconds)
        source = save(f'{name}-dual.json', proof)
        checked, _ = run([HERE / 'check_dual.rad', '--strict-types', '--deny-warnings', '--', source], f'{name}-checked')
        require(json.loads(checked.stdout)['contradiction'] == verify_dual(proof), 'dual checkers differ')
        result.update(core_size=len(core), dual_size=len(proof['rows']), dual_sha256=digest(source), independently_verified=True)
    elif status == z3.sat:
        model = solver.model()
        result['candidate'] = {k: str(model.eval(v, model_completion=True)) for k, v in variables.items()}
        result['scope'] = 'Candidate requires independent primal and lower-bound checking before any theorem claim.'
    else:
        result['reason'] = solver.reason_unknown()
    save(f'{name}-result.json', result)
    print(json.dumps({k: v for k, v in result.items() if k != 'candidate'}), flush=True)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--memories', type=int, nargs='+', default=[1, 2, 3])
    parser.add_argument('--boundary', type=int, default=1792)
    parser.add_argument('--seconds', type=int, default=30)
    parser.add_argument('--context-depth', type=int)
    args = parser.parse_args()
    for memory in args.memories: investigate(memory, args.boundary, args.seconds, args.context_depth)


if __name__ == '__main__': main()
