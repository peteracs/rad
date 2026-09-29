"""Compile an actual Collatz segment into an exact rewrite obstruction."""

from collections import Counter
import csv
import json

from investigate import HERE, RULES, independent_row, require, run, save, verify_dual
from orbit_alias import binary_word, verify_alias


def derive(witness):
    verify_alias(witness)
    memory = witness['memory']
    n = witness['left']
    word = binary_word(n)
    multiplicities, rule_counts = Counter(), Counter()
    descriptors = {}
    trace = [n]
    for _ in range(witness['steps']):
        previous = n
        n = (3*n+1)//2 if n%2 else n//2
        trace.append(n)
        rule = previous % 2
        while True:
            if rule < 2:
                start = len(word)-2
            else:
                ternary = next(i for i, x in enumerate(word) if x in (4, 5, 6))
                start = ternary-1
                rule = 8+word[ternary]-4 if ternary == 1 else 2+word[ternary-1]*3+word[ternary]-4
            lhs, rhs = ([ord(c)-97 for c in text] for text in RULES[rule])
            require(word[start:start+len(lhs)] == lhs, 'literal rewrite mismatch')
            before = [] if rule >= 8 else word[1:start]
            after = [] if rule < 2 else word[start+len(lhs):-1]
            raw = dict(kind=0, rule=rule, before=before[-(memory-1):] if memory > 1 else [],
                       after=after[:memory-1], state=[], symbol=-1)
            terms, bound = independent_row(raw, memory, 1792)
            raw.update(terms=terms, bound=bound)
            identity = json.dumps(raw, sort_keys=True)
            descriptors[identity] = raw
            multiplicities[identity] += 1
            rule_counts[rule] += 1
            word = word[:start]+rhs+word[start+len(lhs):]
            if not any(x in (4, 5, 6) for x in word): break
            rule = 2
        require(word == binary_word(n), 'literal binary normalization differs')
    require(n == witness['right'], 'literal endpoint differs')
    proof = dict(memory=memory, boundary=1792,
                 rows=[dict(multiplier=weight, constraint=descriptors[key]) for key, weight in multiplicities.items()],
                 collatz_proved=False)
    verify_dual(proof)
    return proof, dict(trace=trace, rule_counts=dict(sorted(rule_counts.items())), rewrite_steps=sum(rule_counts.values()))


def export_certificate(proof, path):
    # Sparse descriptors are the proof. Derived coefficient maps are rebuilt
    # by both checkers and need not be repeated in a durable artifact.
    with path.open('w', encoding='utf-8', newline='') as stream:
        writer = csv.writer(stream)
        if 'modulus' in proof:
            writer.writerow(['multiplier', 'rule', 'residue', 'symbol'])
            for term in proof['rows']:
                row = term['constraint']
                writer.writerow([term['multiplier'], row['rule'], row['residue'], row['symbol']])
        else:
            writer.writerow(['multiplier', 'kind', 'rule', 'before', 'after', 'state', 'symbol'])
            for term in proof['rows']:
                row = term['constraint']
                chars = lambda text: ''.join(chr(symbol+97) for symbol in text)
                writer.writerow([term['multiplier'], row['kind'], row['rule'], chars(row['before']),
                                 chars(row['after']), chars(row['state']), row['symbol']])


def main():
    witness = dict(left=674, right=650, steps=54, odds=34, memory=4, modulus=24)
    source = save('literal-alias.json', witness)
    output, _ = run([HERE/'check_alias.rad', '--strict-types', '--deny-warnings', '--', source], 'literal-alias')
    proof, derivation = derive(witness)
    proof_path = save('literal-dual.json', proof)
    run([HERE/'check_dual.rad', '--strict-types', '--deny-warnings', '--', proof_path], 'literal-dual')
    save('literal-derivation.json', derivation)
    directory = HERE/'certificates'
    directory.mkdir(exist_ok=True)
    export_certificate(proof, directory/'literal.csv')
    (directory/'orbit.json').write_text(json.dumps(witness, indent=2)+'\n', encoding='utf-8', newline='\n')
    print(output.stdout.decode('utf-8').strip())
    print(json.dumps(derivation))


if __name__ == '__main__': main()
