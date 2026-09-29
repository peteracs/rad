"""Repair an inconclusive symbolic query with an exactly checked LP support."""

import json

from dual_scout import propose_support
from investigate import HERE, OUT, digest, linear_dual, require, run, save
from modular import independent, verify


def main():
    modulus, boundary = 127, 1792
    rows = [json.loads(line) for line in (OUT / 'modular-127-b1792.stdout').read_text(encoding='utf-8').splitlines()]
    for row in rows:
        terms, bound = independent(row, modulus, boundary)
        require(terms == {k: v for k, v in row['terms'].items() if v} and bound == row['bound'], 'modular scout input differs')
    result, selected = propose_support(rows)
    result.update(modulus=modulus, boundary=boundary)
    if selected:
        try:
            terms = linear_dual(rows, selected, 30)
        except ValueError as error:
            result['exact_reconstruction_failure'] = str(error)
            save('modular-127-scout-result.json', result)
            print(json.dumps(result), flush=True)
            return
        proof = dict(modulus=modulus, boundary=boundary, rows=terms, collatz_proved=False)
        verify(proof)
        source = save('modular-127-scout-dual.json', proof)
        output, _ = run([HERE / 'check_modular.rad', '--strict-types', '--deny-warnings', '--', source], 'modular-127-scout-checked')
        require(json.loads(output.stdout)['contradiction'] == verify(proof), 'modular scout checkers differ')
        result.update(exact_obstruction=True, dual_size=len(proof['rows']), dual_sha256=digest(source))
    save('modular-127-scout-result.json', result)
    print(json.dumps(result), flush=True)


if __name__ == '__main__': main()
