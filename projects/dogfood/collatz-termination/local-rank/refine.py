"""Refine a local-factor hypothesis with literal RAD counterexamples."""

from fractions import Fraction
import json
import math

from investigate import HERE, OUT, independent_row, investigate, require, run, save


def scaled(result):
    values = {k: Fraction(v) for k, v in result['candidate'].items()}
    denominator = math.lcm(*(v.denominator for v in values.values()))
    weights = {k: int(v * denominator) for k, v in values.items()}
    require(denominator <= 10**9 and all(abs(v) <= 10**9 for v in weights.values()), 'candidate exceeds RAD exact domain')
    return dict(memory=result['memory'], boundary=result['boundary'], denominator=denominator, weights=weights)


def main():
    result = json.loads((OUT / 'm4-c2-b1792-result.json').read_text(encoding='utf-8'))
    extras = []
    seen = set()
    history = []
    for iteration in range(1, 7):
        if result['status'] != 'sat': break
        candidate = scaled(result)
        source = save(f'refine-{iteration}-candidate.json', candidate)
        output, elapsed = run([HERE / 'refine.rad', '--strict-types', '--deny-warnings', '--', source],
                              f'refine-{iteration}', timeout=180)
        rows = [json.loads(line) for line in output.stdout.splitlines()]
        summary = rows.pop()
        require(summary['kind'] == 'summary', 'missing refinement summary')
        for row in rows:
            coefficients, bound = independent_row(row, candidate['memory'], candidate['boundary'])
            require(coefficients == {k: v for k, v in row['terms'].items() if v}, 'counterexample coefficients differ')
            left = sum(v * candidate['weights'].get(k, 0) for k, v in coefficients.items())
            require(left < bound * candidate['denominator'], 'reported counterexample does not violate the candidate')
            identity = json.dumps(row, sort_keys=True)
            if identity not in seen: extras.append(row); seen.add(identity)
        history.append(dict(iteration=iteration, **summary, recorded_counterexamples=len(rows), elapsed_seconds=elapsed))
        print(json.dumps(history[-1]), flush=True)
        save('refinement-history.json', history)
        if summary['failures'] == 0:
            print('Full RAD context check passed; independent primal audit and theorem review required.', flush=True)
            break
        require(rows, 'violations without witnesses')
        result = investigate(4, 1792, 30, 2, extras, f'-refined-{iteration}')
    save('refinement-final.json', result)


if __name__ == '__main__': main()
