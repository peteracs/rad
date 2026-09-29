"""Independent full Fourier algebra, RAD search, and canonical counterexample."""
from fractions import Fraction as F
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s
from build_amplification import calculate


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    root = Path(__file__).resolve().parents[3]
    amps, nu, modes, derivative, energy, rate, dissipation = calculate()
    for k, vector in derivative.items():
        require(s.expand(s.Matrix(k).dot(vector)) == 0, 'pressure-projected derivative is not divergence free')
        opposite = tuple(-v for v in k)
        require(all(s.expand(v) == 0 for v in derivative[opposite]-s.conjugate(vector)), 'Fourier reality mismatch')
    kinetic_rate = s.expand(sum(s.re(s.conjugate(u).dot(derivative[k])) for k, u in modes.items()))
    require(s.expand(kinetic_rate+2*nu*energy) == 0, 'full kinetic energy identity failed')
    # Bounds proving the entire continuous parameter cone, not just its corners.
    stretching = F(1)-F(25, 256)
    dissipative = F(875, 32)/64
    require(stretching-dissipative == F(973, 2048) > F(3, 8), 'robust stretching margin')
    require(F(475, 64) < 8 and F(24, 64) == F(3, 8), 'enstrophy conversion bound')
    rad = str(root/'target/debug/rad.exe')
    direct = subprocess.run([rad, 'projects/dogfood/navier-stokes/spectral_rate.rad', '--strict-types', '--deny-warnings'],
                            cwd=root, check=True, capture_output=True, text=True, encoding='utf-8')
    direct_rows = [json.loads(line) for line in direct.stdout.splitlines() if line.startswith('{')]
    require([row['f'] for row in direct_rows] == [0, 16, 128], 'direct convolution case coverage')
    for row in direct_rows:
        substitutions = dict(zip(amps, [64]*5+[row['f']])) | {nu: 1}
        require(row['e8'] == 8*energy.subs(substitutions) and row['rate8'] == 8*rate.subs(substitutions),
                'RAD direct convolution differs from symbolic formula')
        generated = derivative[(1, 0, 1)].subs(substitutions)
        require(any(value != 0 for value in generated), 'expected generated-mode witness absent')
        for j in range(3):
            actual = s.Rational(row['generated_re'][j], row['generated_denominator'])+s.I*s.Rational(row['generated_im'][j], row['generated_denominator'])
            require(s.simplify(actual-generated[j]) == 0, 'RAD generated mode differs from full derivative')
    command = [rad, 'projects/dogfood/navier-stokes/amplification_search.rad', '--experimental-laws', '--strict-types', '--deny-warnings']
    baseline = subprocess.run(command, cwd=root, check=True, capture_output=True, text=True, encoding='utf-8',
                              env={**os.environ, 'RAYON_NUM_THREADS': '1'})
    rows = [json.loads(line) for line in baseline.stdout.splitlines() if line.startswith('{')]
    require(len(rows) == 225, 'amplification search coverage')
    first_failure = None
    for lane, row in enumerate(rows):
        if lane < 96:
            bits = lane//3
            values = [64+16*((bits//(2**j))%2) for j in [4, 3, 2, 1, 0]]+[-16+16*(lane%3)]
        else:
            values = [64]*5+[lane-96]
        substitutions = dict(zip(amps, values)) | {nu: 1}
        e4, r4 = int(4*energy.subs(substitutions)), int(4*rate.subs(substitutions))
        holds = r4 >= 0 and r4*r4*16384 >= e4**3
        require(row == dict(lane=lane, e4=e4, rate4=r4, lower_bound=holds), 'independent search result mismatch')
        if lane < 96:
            require(holds, 'continuous-cone corner failure')
        elif not holds and first_failure is None:
            first_failure = values[-1]
    require(first_failure == 39, 'canonical remainder counterexample changed')
    summaries = [json.loads(line[6:]) for line in baseline.stdout.splitlines() if line.startswith('audit ')]
    require(summaries == [dict(checked=225, cone_passes=96, first_remainder_failure=39)], 'causal aggregate mismatch')
    require('AssembleAmp' in baseline.stdout and 'SubmitAmp' in baseline.stdout, 'amplification ancestry missing')
    rejected = subprocess.run(command+['--', 'hide-counterexample'], cwd=root, capture_output=True, text=True, encoding='utf-8')
    require(rejected.returncode != 0 and 'forged amplification evidence' in rejected.stdout+rejected.stderr,
            'hidden counterexample was accepted')
    with tempfile.TemporaryDirectory(prefix='rad-amplification-') as temporary:
        trace = Path(temporary)/'amplification.radr'
        parallel = subprocess.run(command+['--record', str(trace)], cwd=root, check=True, capture_output=True,
                                  text=True, encoding='utf-8', env={**os.environ, 'RAYON_NUM_THREADS': '4'})
        require(parallel.stdout == baseline.stdout, 'amplification worker-count mismatch')
        replay = subprocess.run([rad, 'replay', str(trace)], cwd=root, check=True, capture_output=True, text=True, encoding='utf-8')
        require('Replay verified: world digest matches the recorded run' in replay.stderr, 'amplification replay failure')
    print('Verified full Fourier pressure projection, energy identity, and independent RAD convolution witnesses.')
    print('225 search cases agree; 96 cone corners pass; smallest scanned positive remainder failure is f=39.')
    print('Hidden counterexample rejected; worker-count output and recorded-world replay agree.')
    print('Continuous-cone lower-bound constants verified. Persistence/invariance and blowup remain unproved.')


if __name__ == '__main__':
    main()
