"""Independent rational verification of the local-convergence constants."""
from fractions import Fraction as F
import json
import os
from pathlib import Path
import subprocess
import tempfile


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    root = Path(__file__).resolve().parents[3]
    # Recompute derivative/product bounds independently as rational numbers.
    rho = [F(1), F(2), F(36), F(1584)]
    reciprocal = [F(8), F(256), F(20992), F(2660352)]
    from math import comb
    step = [F(1)] + [sum(comb(n, k)*rho[k]*reciprocal[n-k] for k in range(n+1))*8**n
                      for n in range(1, 4)]
    g = [F(1), 1+step[1], 2*step[1]+step[2], 3*step[2]+step[3]]
    cutoff = [F(1)] + [sum(comb(n, k)*g[k]*step[n-k] for k in range(n+1)) for n in range(1, 4)]
    require(cutoff[1:] == [4353, 12333568, 21568303104], "independent cutoff derivative bound")
    l = 1+26*cutoff[1]+40*cutoff[2]+8*cutoff[3]
    norm = 7*l
    require(norm == 1211279165117 and 2**40 < norm <= 2**41, "independent Sobolev bound")
    command = [str(root/"target/debug/rad.exe"), "projects/dogfood/navier-stokes/local_convergence.rad",
               "--experimental-laws", "--strict-types", "--deny-warnings"]
    run = subprocess.run(command, cwd=root, capture_output=True, check=True, text=True, encoding="utf-8",
                         env={**os.environ, "RAYON_NUM_THREADS": "1"})
    rows = [json.loads(line) for line in run.stdout.splitlines() if line.startswith("{")]
    require([(row['norm_exponent'], row['iteration']) for row in rows] ==
            [(m, n) for m in [41, 42, 43] for n in [0, 8, 32, 64]], "certificate coverage")
    for row in rows:
        m, n, e = row['norm_exponent'], row['iteration'], row['time_den_exponent']
        require(e % 2 == 0, "nonintegral half exponent")
        contraction = F(8)*2**m*F(1, 2**(e//2))
        require(contraction <= F(1, 2), "contraction certificate failed")
        require(row['error_exponent'] == m-2-n, "Picard error exponent mismatch")
    require('AssembleConvergence' in run.stdout and 'SubmitConvergence' in run.stdout, "convergence ancestry missing")
    for probe, reason in [('undersized-norm', 'initial norm bound not covered'),
                          ('oversized-time', 'contraction bound not established')]:
        rejected = subprocess.run(command+['--', probe], cwd=root, capture_output=True, text=True, encoding='utf-8')
        require(rejected.returncode != 0 and reason in rejected.stdout+rejected.stderr, "invalid bound not rejected")
    with tempfile.TemporaryDirectory(prefix='rad-local-convergence-') as temporary:
        trace = Path(temporary)/'convergence.radr'
        parallel = subprocess.run(command+['--record', str(trace)], cwd=root, check=True,
                                  capture_output=True, text=True, encoding='utf-8',
                                  env={**os.environ, 'RAYON_NUM_THREADS': '4'})
        require(parallel.stdout == run.stdout, "convergence worker-count mismatch")
        replay = subprocess.run([command[0], 'replay', str(trace)], cwd=root, check=True,
                                capture_output=True, text=True, encoding='utf-8')
        require('Replay verified: world digest matches the recorded run' in replay.stderr, "convergence replay failed")
    print('Verified cutoff H2 upper bound <= 2^41 and 12 rational contraction/error certificates.')
    print('Rejected insufficient norm and excessive-time certificates; worker-count and replay checks passed.')
    print('Analytic bridge: short-time Picard convergence on T=nu*2^-90. No blowup conclusion.')


if __name__ == '__main__':
    main()
