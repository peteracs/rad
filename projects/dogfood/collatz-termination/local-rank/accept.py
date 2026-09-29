"""Recheck durable proofs without trusting search outcomes or floating point."""

from copy import deepcopy
import csv
from datetime import datetime, timezone
import json
import os
import subprocess
import sys

from investigate import HERE, OUT, RAD, ROOT, digest, independent_row, investigate, key, require, run, save, verify_dual
from literal import derive
from modular import verify as verify_modular
from orbit_alias import verify_alias


def load_certificate(metadata):
    terms = []
    with (HERE/'certificates'/metadata['file']).open(encoding='utf-8', newline='') as stream:
        for row in csv.DictReader(stream):
            if metadata['model'] == 'factors':
                word = lambda field: [ord(c)-97 for c in row[field]]
                constraint = dict(kind=int(row['kind']), rule=int(row['rule']), before=word('before'), after=word('after'),
                                  state=word('state'), symbol=int(row['symbol']))
            else:
                require(metadata['model'] == 'prefix-modulus', 'unknown certificate model')
                constraint = {key: int(row[key]) for key in ('rule', 'residue', 'symbol')}
            terms.append(dict(multiplier=int(row['multiplier']), constraint=constraint))
    return {key: value for key, value in metadata.items() if key not in ('file', 'model')} | dict(rows=terms, collatz_proved=False)


def main():
    OUT.mkdir(exist_ok=True)
    manifest = json.loads((HERE/'certificates/manifest.json').read_text(encoding='utf-8'))
    checked = []
    for metadata in manifest:
        proof = load_certificate(metadata)
        factor = metadata['model'] == 'factors'
        expected = verify_dual(proof) if factor else verify_modular(proof)
        source = save('accepted-'+metadata['file']+'.json', proof)
        output, _ = run([HERE/('check_dual.rad' if factor else 'check_modular.rad'), '--strict-types', '--deny-warnings',
                         '--', source], 'accepted-'+metadata['file'])
        require(json.loads(output.stdout)['contradiction'] == expected, 'certificate checkers differ')
        checked.append(dict(**metadata, rows=len(proof['rows']), contradiction=expected))

    witness = json.loads((HERE/'certificates/orbit.json').read_text(encoding='utf-8'))
    verify_alias(witness)
    derived, trajectory = derive(witness)
    literal = load_certificate(next(row for row in manifest if row['file'] == 'literal.csv'))
    # The trace compiler may include derived terms; the durable certificate
    # contains only descriptors. Equality of all mathematical rows is sufficient.
    require([(t['multiplier'], independent_row(t['constraint'], 4, 1792)) for t in derived['rows']] ==
            [(t['multiplier'], independent_row(t['constraint'], 4, 1792)) for t in literal['rows']], 'literal certificate differs from trajectory')
    source = save('accepted-orbit.json', witness)
    run([HERE/'check_alias.rad', '--strict-types', '--deny-warnings', '--', source], 'accepted-orbit')

    generated = []
    for workers in (1, 4):
        trace = OUT/f'accepted-generation-{workers}.radr'
        output, _ = run([HERE/'generate.rad', '--strict-types', '--deny-warnings', '--record', trace, '--', 2, 1792],
                        f'accepted-generation-{workers}', workers)
        rows = [json.loads(line) for line in output.stdout.splitlines()]
        for row in rows:
            coefficients, bound = independent_row(row, 2, 1792)
            require(coefficients == {k:v for k,v in row['terms'].items() if v} and bound == row['bound'], 'generator oracle differs')
        generated.append(rows)
        replay, _ = run(['replay', trace], f'accepted-replay-{workers}', 5-workers)
        require(b'Replay verified: world digest matches' in replay.stdout+replay.stderr, 'generation replay differs')
    require(generated[0] == generated[1], 'workers changed the question')

    # A concrete positive control removes the only rule that creates ternary
    # digits. Restoring it must invalidate this proposed ranking.
    control = dict(memory=1, boundary=1792, denominator=1, weights={key([s]):1 for s in (4,5,6)})
    for omitted in (1, -1):
        source = save(f'accepted-control-{omitted}.json', control | dict(omitted=omitted))
        output, _ = run([HERE/'refine.rad', '--strict-types', '--deny-warnings', '--', source], f'accepted-control-{omitted}')
        summary = json.loads(output.stdout.splitlines()[-1])
        require(summary['failures'] == (0 if omitted == 1 else 1), 'control classification differs')

    mutants = []
    bad = deepcopy(literal); bad['rows'][0]['multiplier'] += 1
    mutants.append(('weight', 'check_dual.rad', bad, 'coefficients do not cancel'))
    bad = deepcopy(literal); bad['boundary'] = 0
    mutants.append(('strictness', 'check_dual.rad', bad, 'invalid rewrite request'))
    bad = deepcopy(literal); bad['rows'][0]['constraint']['before'] = [2]
    mutants.append(('context', 'check_dual.rad', bad, 'invalid context digit'))
    bad = deepcopy(literal); bad['memory'] = 5
    mutants.append(('memory', 'check_dual.rad', bad, 'coefficients do not cancel'))
    for field, value, message in [('right',649,'literal trajectory differs'), ('memory',5,'factor counts differ'),
                                  ('modulus',5,'endpoint residues differ'), ('odds',33,'literal trajectory differs')]:
        mutants.append(('alias-'+field, 'check_alias.rad', witness | {field:value}, message))
    modular = load_certificate(next(row for row in manifest if row.get('modulus') == 5))
    bad = deepcopy(modular); bad['rows'][0]['constraint']['residue'] = 5
    mutants.append(('modular-state', 'check_modular.rad', bad, 'invalid modular state'))
    for name, program, data, message in mutants:
        source = save(f'accepted-mutant-{name}.json', data)
        output, _ = run([HERE/program, '--strict-types', '--deny-warnings', '--', source], f'accepted-mutant-{name}', success=False)
        require(message.encode() in output.stderr, 'mutation failed for an unrelated reason')

    # Produces a tiny fresh standalone SMT-LIB query for the export regression.
    investigate(1, 1792, 5)
    tests = subprocess.run([sys.executable, '-O', '-m', 'unittest', 'discover', '-s', str(HERE), '-p', 'test_*.py'],
                           cwd=ROOT, capture_output=True, timeout=60)
    (OUT/'accepted-tests.stdout').write_bytes(tests.stdout)
    (OUT/'accepted-tests.stderr').write_bytes(tests.stderr)
    require(tests.returncode == 0, tests.stderr.decode('utf-8', errors='replace'))
    evidence = dict(format='rad-collatz-question-led-research-v1', completed_utc=datetime.now(timezone.utc).isoformat(),
                    checked_certificates=checked, literal_orbit=witness, literal_rewrite_steps=trajectory['rewrite_steps'],
                    literal_rule_counts=trajectory['rule_counts'], mutations_rejected=len(mutants), python_tests=14,
                    source_sha256={p.name:digest(p) for p in HERE.iterdir() if p.suffix in ('.rad','.py','.md')},
                    certificates_sha256={p.name:digest(p) for p in (HERE/'certificates').iterdir() if p.is_file()},
                    artifacts_sha256={p.name:digest(p) for p in OUT.iterdir() if p.is_file()},
                    rad_binary_sha256=digest(RAD), collatz_proved=False,
                    scope='Exact finite identities exclude the specified entire ranking families. The orbit witness also excludes arbitrary functions of its equal feature summaries. It is not a Collatz counterexample or convergence proof. Solver timeouts and floating-point outcomes are not proofs. Raw history may predate the final source; current acceptance recomputes all durable certificates.')
    (HERE/'evidence.json').write_text(json.dumps(evidence,indent=2)+'\n',encoding='utf-8',newline='\n')
    print(json.dumps(dict(checked_certificates=len(checked), mutations_rejected=len(mutants), python_tests=14, collatz_proved=False)))


if __name__ == '__main__': main()
