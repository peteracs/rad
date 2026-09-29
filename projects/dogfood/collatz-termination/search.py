"""Bounded exact synthesis of full Collatz rewriting termination certificates.

This independently implemented encoder uses the mixed-base system of Yolcu,
Aaronson and Heule (2023). SAT models are candidate proofs; unknown is never
treated as UNSAT, and an incomplete removal sequence is never a proof.
"""

import argparse
from concurrent.futures import ProcessPoolExecutor, as_completed
import hashlib
import json
from pathlib import Path
import time

import z3

HERE = Path(__file__).resolve().parent
RULES = [('ad', 'd'), ('bd', 'gd'), ('ae', 'ea'), ('af', 'eb'),
         ('ag', 'fa'), ('be', 'fb'), ('bf', 'ga'), ('bg', 'gb'),
         ('ce', 'cb'), ('cf', 'caa'), ('cg', 'cab')]
SYMBOLS = 'abcdefg'
MINUS_INFINITY = -1000000


def synthesize(job):
    semiring, dimension, maximum, reverse, timeout, omitted, denominator, *options = job
    ground = options[0] if options else False
    repair = options[1] if len(options)>1 else None
    if not (semiring in ('natural', 'arctic') and 1 <= dimension <= 16 and 1 <= maximum <= 31):
        raise ValueError('unsupported exact search domain')
    arctic = semiring == 'arctic'
    if not (1 <= denominator <= 31 and (not arctic or denominator == 1)):
        raise ValueError('invalid interpretation denominator')
    begin = time.perf_counter()
    size = dimension + 1
    # Homogeneous products of length <=3, including affine translations.
    upper = max(maximum, denominator) ** 3 * size ** 2 * denominator ** 3
    width = max(2, upper.bit_length() + 1)
    zero = z3.IntVal(MINUS_INFINITY) if arctic else z3.BitVecVal(0, width)
    one = z3.IntVal(0) if arctic else z3.BitVecVal(1, width)
    solver = z3.SolverFor('QF_LIA' if arctic else 'QF_BV')
    solver.set(timeout=timeout, random_seed=1937)
    letters = {}
    for symbol in SYMBOLS:
        matrix = []
        for i in range(size):
            row = []
            for j in range(size):
                if i == dimension:
                    value = (one if arctic else z3.BitVecVal(denominator, width)) if j == dimension else zero
                elif ground and symbol == ('c' if reverse else 'd') and j < dimension:
                    value = zero
                else:
                    value = z3.Int(f'{symbol}_{i}_{j}') if arctic else z3.BitVec(f'{symbol}_{i}_{j}', width)
                    if arctic:
                        solver.add(z3.Or(value == MINUS_INFINITY, z3.And(value >= -maximum, value <= maximum)))
                    else:
                        solver.add(z3.ULE(value, maximum))
                row.append(value)
            matrix.append(row)
        letters[symbol] = matrix
        if arctic:
            # Carrier: first coordinate is a nonnegative integer; others
            # belong to Z union {-infinity}. Every letter preserves it.
            solver.add(z3.Or(matrix[0][0] >= 0, matrix[0][dimension] >= 0))

    if repair is not None:
        if arctic or denominator!=1:
            raise ValueError('repair currently supports integer matrix interpretations')
        center=repair['matrices']
        if len(center)!=7 or any(len(a)!=size or any(len(r)!=size for r in a) for a in center):
            raise ValueError('repair matrix shape')
        if not 0<=repair['radius']<=7*dimension*size:
            raise ValueError('repair Hamming radius')
        changes=[]
        for k,symbol in enumerate(SYMBOLS):
            for i in range(dimension):
                for j in range(size):
                    value=center[k][i][j]
                    if type(value) is not int or not 0<=value<=maximum:
                        raise ValueError('repair center outside coefficient domain')
                    changes.append((letters[symbol][i][j]!=value,1))
        solver.add(z3.PbLe(changes,repair['radius']))

    def multiply(a, b):
        if arctic:
            result = []
            for i in range(size):
                row = []
                for j in range(size):
                    value = zero
                    for k in range(size):
                        term = z3.If(z3.Or(a[i][k] == zero, b[k][j] == zero), zero, a[i][k] + b[k][j])
                        value = z3.If(term > value, term, value)
                    row.append(z3.simplify(value))
                result.append(row)
            return result
        return [[z3.simplify(sum((a[i][k] * b[k][j] for k in range(size)), zero))
                 for j in range(size)] for i in range(size)]

    def interpret(word):
        if reverse:
            word = word[::-1]
        result = [[one if i == j else zero for j in range(size)] for i in range(size)]
        for letter in word:
            result = multiply(result, letters[letter])
        return result

    active = [i for i in range(len(RULES)) if i != omitted]
    boundary = [i for i in ([0, 1] if reverse else [8, 9, 10]) if i in active]
    pairs, strict = {}, {}
    for i in active:
        left, right = map(interpret, RULES[i])
        if not arctic:
            # Words of unequal lengths have different rational denominators.
            left = [[x * denominator ** len(RULES[i][1]) for x in row] for row in left]
            right = [[x * denominator ** len(RULES[i][0]) for x in row] for row in right]
        pairs[i] = left, right
        solver.add(*((left[a][b] >= right[a][b] if arctic else z3.UGE(left[a][b], right[a][b]))
                     for a in range(dimension) for b in range(size)))
        if i in boundary:
            if arctic:
                strict[i] = z3.And([z3.Or(left[0][j] > right[0][j],
                    z3.And(left[0][j] == zero, right[0][j] == zero)) for j in range(size)])
            else:
                strict[i] = z3.UGT(left[0][dimension], right[0][dimension])
    solver.add(z3.Or(list(strict.values())))
    if repair is not None:
        selected=[i for i in boundary if repair['strict_mask'] & (1<<i)]
        if not selected or sum(1<<i for i in selected)!=repair['strict_mask']:
            raise ValueError('repair strict mask outside active boundary')
        solver.add(*(strict[i] for i in selected))
    output = HERE / 'out'
    output.mkdir(exist_ok=True)
    label = f'{semiring}{"-ground" if ground else ""}-d{dimension}-m{maximum}-q{denominator}-r{int(reverse)}-omit{omitted}'
    if repair is not None:
        center_hash=hashlib.sha256(json.dumps(repair,sort_keys=True).encode()).hexdigest()
        label+=f'-repair-{center_hash[:16]}'
    query = output / f'{label}.smt2'
    query.write_text(solver.to_smt2(), encoding='utf-8', newline='\n')
    status = solver.check()
    result = dict(label=label, semiring=semiring, dimension=dimension, maximum=maximum, denominator=denominator,
                  reverse=reverse, omitted=omitted, width=width, ground_terminal=ground,
                  timeout_ms=timeout, status=str(status),
                  elapsed_seconds=time.perf_counter()-begin,
                  query_sha256=hashlib.sha256(query.read_bytes()).hexdigest(),
                  full_rule_set=omitted == -1, complete_proof=False)
    if repair is not None:
        result['repair']=repair
    if status == z3.sat:
        model = solver.model()
        result['matrices'] = [[[model.eval(x, model_completion=True).as_long() for x in row]
                               for row in letters[s]] for s in SYMBOLS]
        result['strict_rules'] = [i for i in boundary if z3.is_true(model.eval(strict[i]))]
        result['remaining_boundary'] = [i for i in boundary if i not in result['strict_rules']]
        result['complete_proof'] = omitted == -1 and not result['remaining_boundary']
    elif status == z3.unknown:
        result['reason'] = solver.reason_unknown()
    (output / f'{label}.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8', newline='\n')
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--seconds', type=int, default=30)
    parser.add_argument('--dimensions', type=int, nargs='+', default=[1, 2, 3, 4])
    parser.add_argument('--maximum', type=int, default=3)
    parser.add_argument('--workers', type=int, default=4)
    parser.add_argument('--semirings', choices=['natural', 'arctic'], nargs='+', default=['natural', 'arctic'])
    parser.add_argument('--denominator',type=int,default=1)
    parser.add_argument('--ground',action='store_true')
    args = parser.parse_args()
    jobs = [(ring, d, args.maximum, rev, args.seconds*1000, -1, args.denominator if ring == 'natural' else 1,args.ground)
            for ring in args.semirings for d in args.dimensions for rev in (False, True)]
    # A deliberately weakened system checks that the encoder can find proofs.
    jobs.insert(0, ('natural', 1, 3, False, args.seconds*1000, 3, 1,args.ground))
    rows = []
    with ProcessPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(synthesize, job) for job in jobs]
        for future in as_completed(futures):
            row = future.result()
            rows.append(row)
            print(json.dumps({k: v for k, v in row.items() if k != 'matrices'}), flush=True)
    rows.sort(key=lambda r: r['label'])
    (HERE / 'out/campaign.json').write_text(json.dumps(rows, indent=2)+'\n', encoding='utf-8', newline='\n')


if __name__ == '__main__':
    main()
