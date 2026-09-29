"""Audit the RAD-owned repair search, native acceleration, and proof gate."""

from copy import deepcopy
from datetime import datetime, timezone
import json
import platform
import statistics
import time

from accept import HERE, ROOT, OUT, RAD, digest, run, write
from repair_campaign import configuration, request, run as campaign
from repair_oracle import audit_report
from verify import known_natural_control, require
from context_probe import main as check_context_witnesses


def reports(name):
    return [json.loads(line) for line in (OUT / f'{name}.stdout').read_text(encoding='utf-8').splitlines()]


def semantic_report(report):
    return {key: value for key, value in report.items() if key != 'rule_evaluations'}


def benchmark(req, name, mode):
    source = write(f'{name}.json', req)
    started = time.perf_counter()
    output = run([RAD, HERE / 'repair_bench.rad', '--strict-types', '--deny-warnings',
                  '--', mode, source], name)
    result = json.loads(output.stdout)
    elapsed = time.perf_counter() - started
    audit_report(dict(label=name, omitted=-1, request=req, report=result))
    return dict(elapsed_seconds=elapsed, result=result)


def main():
    OUT.mkdir(exist_ok=True)
    # Every new launch goes through the final source; historical outputs retain
    # their original hashes and timings and are not counted as fresh searches.
    config = configuration(2, 120, [2, 3])
    config['lanes'] = config['lanes'][::7]
    control_request = request(trials=120)
    control_request['active_mask'] ^= 8
    config['lanes'].append(dict(label='positive-control', omitted=3, request=control_request))
    for workers in (1, 4):
        name = f'repair-accepted-{workers}'
        campaign(config, name, workers, record=True)
        rows = reports(name)
        for row in rows:
            if row['kind'] == 'lane':
                audit_report(row)
        require(any(row['kind'] == 'certificate' and not row['checked']['complete_proof'] for row in rows),
                'positive control did not reach the certificate checker')
        replay = run([RAD, 'replay', OUT / f'{name}.radr'], f'{name}-replay', 5 - workers)
        require(b'Replay verified: world digest matches' in replay.stdout + replay.stderr, 'repair replay differs')
    require(reports('repair-accepted-1') == reports('repair-accepted-4'), 'worker count changed repair results')

    # Different dimensions, orientations, temperatures, and grounding test the
    # entire accepted/rejected walk, including a checksum of every trial score.
    differential = []
    for d, reverse in ((2, False), (3, True), (4, False), (8, True)):
        req = request(d, reverse, 3 if reverse else 1792, trials=40)
        req.update(temperature=0 if d == 3 else 128, ground=None, stop_on_zero=False)
        pair = {mode: benchmark(req, f'repair-diff-{d}-{mode}', mode) for mode in ('native', 'reference')}
        require(semantic_report(pair['native']['result']) == pair['reference']['result'], 'native/reference walk differs')
        differential.append(dict(dimension=d, reverse=reverse, evaluated=pair['native']['result']['evaluated']))

    req = request(trials=20000)
    req.update(seed=1937, ground=None, temperature=16, stop_on_zero=False)
    write('repair-bench-input.json', req)
    pairs = []
    for repeat in range(3):
        modes = ('native', 'reference') if repeat % 2 == 0 else ('reference', 'native')
        pair = {mode: benchmark(req, f'repair-timed-{repeat}-{mode}', mode) for mode in modes}
        require(semantic_report(pair['native']['result']) == pair['reference']['result'], 'timed walks differ')
        pairs.append(pair)
    medians = {mode: statistics.median(pair[mode]['elapsed_seconds'] for pair in pairs) for mode in ('native', 'reference')}
    speed = dict(trials=req['trials'], rows=pairs, median_seconds=medians,
                 speedup=medians['reference'] / medians['native'], processor=platform.processor(),
                 scope='Process wall time includes RAD startup, extension loading, JSON, search, and independent RAD endpoint scoring. Same requests and exact reports; no experimental-laws or profiler flags.')
    write('repair-benchmark-accepted.json', speed)

    mutations = []
    for label, key, value, message in [
        ('rules', 'rules', [[[0], [1]]], 'campaign changed Collatz rewriting rules'),
        ('mask', 'active_mask', 2046, 'campaign silently omitted a rule'),
        ('strict', 'strict_mask', 4, 'strict rules must occur at the left boundary'),
    ]:
        bad = deepcopy(config)
        bad['lanes'][0]['request'][key] = value
        source = write(f'repair-bad-{label}.json', bad)
        result = run([RAD, HERE / 'repair.rad', '--strict-types', '--deny-warnings', '--experimental-laws',
                      '--', source], f'repair-bad-{label}', success=False)
        require(message.encode() in result.stderr, 'campaign mutation did not fail at its domain check')
        mutations.append(label)
    for label in ('homogeneous', 'negative', 'shape'):
        bad = request(trials=1)
        if label == 'homogeneous': bad['matrices'][0][2][0] = 1
        elif label == 'negative': bad['matrices'][0][0][0] = -1
        else: bad['matrices'][0].pop()
        source = write(f'repair-bad-{label}.json', bad)
        result = run([RAD, HERE / 'repair_bench.rad', '--strict-types', '--deny-warnings', '--',
                      'native', source], f'repair-bad-{label}', success=False)
        require(b'rewrite rank' in result.stderr, 'native mutation did not reach domain validation')
        mutations.append(label)

    control = known_natural_control()
    control_path = write('repair-control-model.json', control)
    forged = {**control, 'omitted': -1, 'complete_proof': True}
    forged_path = write('repair-full-forged.json', forged)
    for name, mode, source, code in [
        ('forward', 'forward', control_path, None), ('reverse', 'reverse', control_path, None),
        ('mismatch', 'mismatch', control_path, 'rank.native_reference_disagree'),
        ('forged', 'forged', control_path, 'rank.unproved_complete_system'),
        ('full-forged', 'forged', forged_path, 'rank.unproved_complete_system'),
    ]:
        result = run([RAD, HERE / 'test_repair_settlement.rad', '--strict-types', '--deny-warnings',
                      '--experimental-laws', '--', mode, source], f'repair-settlement-{name}', success=code is None)
        if code:
            require(code.encode() in result.stderr and b'0 evaluation failure(s)' in result.stderr,
                    'forged proposal did not produce a clean constraint violation')
            mutations.append(name)
    require((OUT / 'repair-settlement-forward.stdout').read_bytes() ==
            (OUT / 'repair-settlement-reverse.stdout').read_bytes(), 'proposal order changed resolution')

    historical = {}
    for name in ('repair-campaign', 'repair-pooled', 'repair-final', 'repair-diverse'):
        path = OUT / f'{name}.stdout'
        if not path.exists(): continue
        rows = reports(name)
        for row in rows:
            if row['kind'] == 'lane': audit_report(row)
        rounds = [row for row in rows if row['kind'] == 'round']
        require(rounds and not any(row['complete_proof'] for row in rounds), 'inspect a newly found proof before recording absence')
        historical[name] = dict(output_sha256=digest(path), evaluated=rounds[-1]['evaluated'],
                                lane_reports=sum(row['kind'] == 'lane' for row in rows))
    if 'repair-campaign' in historical and 'repair-pooled' in historical:
        for name in ('repair-pooled', 'repair-final'):
            if name in historical:
                require([r for r in reports('repair-campaign') if r['kind'] == 'lane'] ==
                        [r for r in reports(name) if r['kind'] == 'lane'], 'worker pool changed campaign search')
    if 'repair-diverse' in historical:
        check_context_witnesses()

    kernel = HERE.parent / 'native-math-kernels'
    evidence = dict(format='rad-collatz-repair-search-v1', completed_utc=datetime.now(timezone.utc).isoformat(),
                    complete_proofs=0, historical=historical, differential=differential,
                    mutations_rejected=mutations, benchmark={k: v for k, v in speed.items() if k != 'rows'},
                    sources={p.name: digest(p) for p in HERE.iterdir() if p.suffix in ('.rad', '.py', '.md')},
                    native_sources={p.relative_to(kernel).as_posix(): digest(p) for p in (kernel / 'src').rglob('*.rs')},
                    binaries={str(p.relative_to(ROOT)): digest(p) for p in [RAD, ROOT / 'target/release/rad-ffi-worker.exe',
                                                                         kernel / 'out/rad_dogfood_math_kernels']},
                    artifacts={p.name: digest(p) for p in OUT.glob('repair-*') if p.is_file()},
                    scope='RAD searches and checks candidate worlds; generic native code proposes matrix repairs. Exact endpoint checks, full walk differential tests, constraint rejection, and replay are distinct checks. Mutation counts include replacements by the same value and repeated matrices. Repeated campaigns are validation, not new trials. No full-system certificate was found; score is not proof progress.')
    (HERE / 'repair-evidence.json').write_text(json.dumps(evidence, indent=2) + '\n', encoding='utf-8', newline='\n')
    print(json.dumps(dict(speedup=speed['speedup'], mutations=len(mutations), complete_proofs=0, historical=historical), indent=2))


if __name__ == '__main__': main()
