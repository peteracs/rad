"""Look for actual orbit segments invisible to a local factor-count rank."""

import json
import random

from investigate import HERE, counts, require, run, save


def binary_word(value):
    return [2] + [int(bit) for bit in bin(value)[3:]] + [3]


def verify_alias(row):
    require(row['left'] != row['right'] and row['steps'] > 0, 'trivial orbit alias')
    n = row['left']
    odds = 0
    for _ in range(row['steps']):
        require(n > 1, 'alias passes through terminal state')
        odds += n % 2
        n = (3*n+1)//2 if n % 2 else n//2
    require(n == row['right'] and odds == row['odds'] and odds > 0, 'alias trajectory differs')
    require(counts(binary_word(row['left']), row['memory']) == counts(binary_word(row['right']), row['memory']), 'factor signatures differ')
    modulus = row.get('modulus', 1)
    require(modulus > 0 and row['left'] % modulus == row['right'] % modulus, 'residue signatures differ')


def investigate_alias(questions, name='orbit-alias'):
    source = save(f'{name}-input.json', questions)
    output, elapsed = run([HERE / 'orbit_alias.rad', '--strict-types', '--deny-warnings', '--', source], name, timeout=180)
    rows = [json.loads(line) for line in output.stdout.splitlines()]
    found = [row for row in rows if row['found']]
    for row in found: verify_alias(row)
    minimal = {}
    for row in sorted(found, key=lambda r: (r['steps'], r['left'])):
        minimal.setdefault(f"{row['memory']}:{row['modulus']}", row)
    result = dict(questions=len(questions), elapsed_seconds=elapsed, aliases=len(found),
                  examined=sum(row['examined'] for row in rows), witnesses=minimal, collatz_proved=False)
    save(f'{name}-result.json', result)
    print(json.dumps(result, indent=2))
    return result


def main():
    rng = random.Random(1937)
    starts = [27, 37, 703, 1819, 4255, 9663, 23529, 77031, 626331, 837799, 670617279]
    starts += [2**k-1 for k in (8, 12, 16, 20, 24, 28, 32)]
    starts += [rng.randrange(2, 2**30) for _ in range(32)]
    questions = [dict(start=n, memory=k, limit=1200) for n in starts for k in (1, 2, 3, 4)]
    investigate_alias(questions)


if __name__ == '__main__': main()
