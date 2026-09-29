"""Independent symbolic derivative checks, replay, and remainder mutation."""
import json
from pathlib import Path
import subprocess
import tempfile
import sympy as s

here = Path(__file__).resolve().parent
rad = str(here.parents[2] / 'target/debug/rad.exe')
flags = ['--strict-types', '--deny-warnings', '--experimental-laws']
def command(name):
    return [rad, str(here / name)] + flags

with tempfile.TemporaryDirectory(prefix='rad-covariance-') as directory:
    trace = str(Path(directory) / 'trace.radtrace')
    result = subprocess.run(command('signed_covariance.rad') + ['--record', trace], capture_output=True, text=True, check=True)
    replay = subprocess.run([rad, 'replay', trace], capture_output=True, text=True, check=True)
    assert 'Replay verified: world digest matches the recorded run' in replay.stderr
bad = subprocess.run(command('signed_covariance.rad') + ['--', 'drop-square'], capture_output=True, text=True)
assert bad.returncode != 0 and 'quadratic remainder cannot be discarded' in bad.stdout + bad.stderr
jets = subprocess.run(command('weighted_quotient.rad'), capture_output=True, text=True, check=True)
rows = [json.loads(line) for line in jets.stdout.splitlines()]
assert len(rows) == 9
x = s.Symbol('x')
R, Y = s.Function('R')(x), s.Function('Y')(x)
for m, terms in enumerate(rows):
    actual = 0
    for t in terms:
        key, factors = t['key'], 0
        term = s.Integer(t['c']) * s.diff(R, x, t['r']) / 2**(m+1)
        for j in range(10):
            n = key % 16
            key //= 16
            factors += n
            term *= s.diff(Y, x, j+1)**n
        assert key == 0
        actual += term * Y**(-s.Rational(1, 2)-factors)
    assert s.expand(actual - s.diff(R/(2*s.sqrt(Y)), x, m)) == 0, m
(here / 'signed_covariance.why.txt').write_text(result.stdout, encoding='utf-8')
report = dict(cross_identity=True, quadratic_remainder=True, record_replay=True,
              dropped_square_rejected=True, independent_symbolic_derivative_orders=list(range(9)),
              actual_stage_covariance_bounds_verified=False, navier_stokes_proof=False)
(here / 'signed_covariance.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps(report))
