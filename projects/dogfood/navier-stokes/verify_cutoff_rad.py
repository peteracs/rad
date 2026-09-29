"""Differential check of RAD polynomial results; requires SymPy."""
import argparse
from fractions import Fraction
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s


def require(condition, message):
    if not condition:
        raise ValueError(message)


def curl(field, variables):
    x, y, z = variables
    return s.Matrix([s.diff(field[2], y)-s.diff(field[1], z),
                     s.diff(field[0], z)-s.diff(field[2], x),
                     s.diff(field[1], x)-s.diff(field[0], y)])


def expected(power):
    variables = s.symbols("x y z")
    position = s.Matrix(variables)
    matrix = s.diag(1, 2, -3)
    q = position.dot(position)
    w = curl(-(1-q)**power * position.cross(matrix*position), variables)
    require(s.expand(sum(s.diff(w[i], variables[i]) for i in range(3))) == 0,
            "SymPy solenoidal identity failed")
    linear = curl(5*w+4*w.jacobian(position)*position, variables)
    advection = curl(w.jacobian(position)*w, variables)
    omega = curl(w, variables)
    diffusion = omega.applyfunc(lambda entry: sum(s.diff(entry, v, 2) for v in variables))
    half = s.Rational(1, 2)
    point0 = dict(zip(variables, [half, half, 0]))
    point1 = dict(zip(variables, [half, half, half]))

    def exact_integer(value):
        value = s.simplify(value)
        require(value.is_Integer is True, "nonintegral scaled coefficient")
        return int(value)

    def angular(field):
        return exact_integer(512*(4*field[0]+5*field[1]).subs(point1))

    return dict(power=power,
                curl_b27_at_z0=[exact_integer(512*v.subs(point0)) for v in linear+3*advection],
                diffusion_w_at_z0=[exact_integer(512*v.subs(point0)) for v in diffusion],
                angular_advection_w=angular(advection),
                angular_linear_w=angular(linear),
                angular_diffusion_w=angular(diffusion))


def main():
    root = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", type=Path, default=root / "target/debug/rad.exe")
    args = parser.parse_args()
    command = [str(args.rad.resolve()), "projects/dogfood/navier-stokes/cutoff_polynomial.rad",
               "--experimental-laws", "--strict-types", "--deny-warnings"]
    process = subprocess.run(command, cwd=root, capture_output=True, text=True,
                             encoding="utf-8", check=True,
                             env={**os.environ, "RAYON_NUM_THREADS": "1"})
    rows = [json.loads(line) for line in process.stdout.splitlines() if line.startswith("{")]
    require(rows == [expected(power) for power in range(3)], "RAD/SymPy polynomial mismatch")
    require("AssembleCutoffs" in process.stdout and "SubmitCutoff" in process.stdout,
            "causal ancestry missing")
    require(Fraction(rows[2]["curl_b27_at_z0"][2], 27*512) == Fraction(191, 216),
            "earlier inviscid coefficient not reproduced")
    require(Fraction(rows[2]["diffusion_w_at_z0"][2], 3*512) == 42,
            "earlier viscous coefficient not reproduced")
    require(Fraction(10, 27)*Fraction(14, 27) == Fraction(140, 729),
            "lower-bound arithmetic")
    for probe, diagnostic in [("coefficient", "polynomial coefficient bound"),
                              ("degree", "polynomial degree overflow")]:
        rejected = subprocess.run(command + ["--", probe], cwd=root,
                                  capture_output=True, text=True, encoding="utf-8")
        require(rejected.returncode != 0 and diagnostic in rejected.stderr + rejected.stdout,
                f"{probe} corruption not rejected with the intended diagnostic")
    with tempfile.TemporaryDirectory(prefix="rad-polynomial-replay-") as directory:
        trace = Path(directory) / "cutoff.radr"
        parallel = subprocess.run(command + ["--record", str(trace)], cwd=root,
                                  check=True, capture_output=True, text=True, encoding="utf-8",
                                  env={**os.environ, "RAYON_NUM_THREADS": "4"})
        require(parallel.stdout == process.stdout, "worker-count or recording mismatch")
        replay = subprocess.run([str(args.rad.resolve()), "replay", str(trace)], cwd=root,
                                check=True, capture_output=True, text=True, encoding="utf-8")
        require("Replay verified: world digest matches the recorded run" in replay.stderr,
                "polynomial replay did not verify")
    print("Three RAD polynomial cases match independent symbolic differentiation.")
    print("Reproduced 191/216 and 42; two nonzero angular obstructions, one zero control.")
    print("One/four-worker outputs match; recorded final world verifies on replay.")
    print("Coefficient and degree overflow probes rejected before arithmetic can wrap or alias.")
    print("General moving-cutoff lemma remains a written analytic argument, not a kernel proof.")


if __name__ == "__main__":
    main()
