"""Use sparse floating-point LP only to propose support for an exact proof."""

import argparse
import json
import sys
import time

from investigate import HERE, OUT, digest, exact_dual, independent_row, require, run, save, verify_dual

# Optional project-local dependency; it never enters the RAD proof checker.
sys.path.insert(0, str(OUT / 'deps'))
import numpy as np
import scipy
from scipy.optimize import linprog
from scipy.sparse import coo_matrix


def propose_support(rows, seconds=30):
    symbols = sorted({k for row in rows for k, v in row['terms'].items() if v})
    indexes = {k: i for i, k in enumerate(symbols)}
    data, coordinates, columns = [], [], []
    for j, row in enumerate(rows):
        for k, value in row['terms'].items():
            if value:
                data.append(value); coordinates.append(indexes[k]); columns.append(j)
        if row['bound']:
            data.append(row['bound']); coordinates.append(len(symbols)); columns.append(j)
    matrix = coo_matrix((data, (coordinates, columns)), shape=(len(symbols)+1, len(rows))).tocsc()
    rhs = np.zeros(len(symbols)+1)
    rhs[-1] = 1
    started = time.perf_counter()
    result = linprog(np.ones(len(rows)), A_eq=matrix, b_eq=rhs, bounds=(0, None),
                     method='highs', options={'time_limit': seconds})
    receipt = dict(scipy_version=scipy.__version__, numpy_version=np.__version__,
                   constraints=len(rows), lp_status=int(result.status), lp_message=result.message,
                   elapsed_seconds=time.perf_counter()-started, exact_obstruction=False, collatz_proved=False)
    selected = [i for i, value in enumerate(result.x) if value > 1e-8] if result.success else []
    return receipt, selected


def scout(rows, memory, boundary, name, seconds=30):
    for row in rows:
        expected, bound = independent_row(row, memory, boundary)
        require(expected == {k: v for k, v in row['terms'].items() if v} and bound == row['bound'], 'scout input differs')
    receipt, selected = propose_support(rows, seconds)
    receipt.update(memory=memory, boundary=boundary)
    if selected:
        proof = exact_dual(rows, selected, memory, boundary, seconds)
        source = save(f'{name}-dual.json', proof)
        checked, _ = run([HERE / 'check_dual.rad', '--strict-types', '--deny-warnings', '--', source], f'{name}-checked')
        require(json.loads(checked.stdout)['contradiction'] == verify_dual(proof), 'scouted exact proof differs')
        receipt.update(exact_obstruction=True, support=len(selected), dual_size=len(proof['rows']), dual_sha256=digest(source))
    save(f'{name}-result.json', receipt)
    print(json.dumps(receipt), flush=True)
    return receipt


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', default='m4-c2-b1792')
    parser.add_argument('--memory', type=int, default=4)
    parser.add_argument('--boundary', type=int, default=1792)
    parser.add_argument('--refinements', type=int, default=2)
    args = parser.parse_args()
    rows = [json.loads(line) for line in (OUT / f'{args.source}.stdout').read_text(encoding='utf-8').splitlines()]
    seen = set()
    for i in range(1, args.refinements+1):
        for line in (OUT / f'refine-{i}.stdout').read_text(encoding='utf-8').splitlines():
            row = json.loads(line)
            if row.get('kind') == 'summary': continue
            identity = json.dumps(row, sort_keys=True)
            if identity not in seen: rows.append(row); seen.add(identity)
    scout(rows, args.memory, args.boundary, 'sparse-dual')


if __name__ == '__main__': main()
