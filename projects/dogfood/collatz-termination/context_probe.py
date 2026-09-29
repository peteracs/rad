"""Extract concrete valid rewrite contexts where a proposed rank increases."""

import itertools
import json

from accept import OUT, HERE, RAD, run, write
from repair_oracle import diagnostics
from verify import RULES, require


def word_value(word):
    require(word[0] == 2 and word[-1] == 3, 'invalid boundaries')
    value = 1
    for digit in word[1:-1]:
        require(digit in (0, 1, 4, 5, 6), 'invalid digit')
        value = 2 * value + digit if digit < 2 else 3 * value + digit - 4
    return value


def rank(word, matrices):
    value = [0] * len(matrices[0])
    value[-1] = 1
    for letter in reversed(word):
        value = [sum(a * b for a, b in zip(row, value)) for row in matrices[letter]]
    return value[0]


def main():
    rows = [json.loads(line) for line in (OUT / 'repair-diverse.stdout').read_text(encoding='utf-8').splitlines()]
    candidates = {}
    for row in rows:
        if row['kind'] != 'lane' or row['request']['reverse'] or row['request']['strict_mask'] != 1792: continue
        if row['report']['best'] != dict(weak=1, strict=0, total=1): continue
        candidates.setdefault(row['label'], row)
    short_words = [list(word) for length in range(3) for word in itertools.product((0, 1, 4, 5, 6), repeat=length)]
    witnesses = []
    for label, row in sorted(candidates.items()):
        req, matrices = row['request'], row['report']['matrices']
        failures = diagnostics(req, matrices)['failures']
        found = None
        for rule in sorted({f['rule'] for f in failures}):
            left, right = ([ord(c) - 97 for c in word] for word in RULES[rule])
            prefixes = [[]] if rule >= 8 else [[2] + word for word in short_words]
            suffixes = [[]] if rule < 2 else [word + [3] for word in short_words]
            for prefix, suffix in itertools.product(prefixes, suffixes):
                lhs, rhs = prefix + left + suffix, prefix + right + suffix
                a, b = rank(lhs, matrices), rank(rhs, matrices)
                if b <= a: continue
                initial, after = word_value(lhs), word_value(rhs)
                require(after == (initial // 2 if rule == 0 else (3 * initial + 1) // 2 if rule == 1 else initial),
                        'rewrite and represented value differ')
                found = dict(label=label, rule=rule, prefix=prefix, suffix=suffix, left_rank=a, right_rank=b,
                             integer_before=initial, integer_after=after,
                             model=dict(label=label, dimension=req['dimension'], maximum=req['maximum'], matrices=matrices))
                break
            if found: break
        require(found is not None, f'{label}: no small witness; investigate reachable context invariants')
        witnesses.append(found)
    require(witnesses, 'no near-certificate candidates were examined')
    source = write('repair-context-witnesses.json', witnesses)
    result = run([RAD, HERE / 'context_probe.rad', '--strict-types', '--deny-warnings', '--', source], 'repair-context-probe')
    expected = [dict(label=w['label'], rule=w['rule'], integer_before=w['integer_before'], integer_after=w['integer_after'],
                     rank_before=w['left_rank'], rank_after=w['right_rank']) for w in witnesses]
    require([json.loads(line) for line in result.stdout.splitlines()] == expected, 'RAD context check differs')
    print(json.dumps(expected, indent=2))


if __name__ == '__main__': main()
