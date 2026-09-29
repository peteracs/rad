"""Exact coefficient and three-dimensional checks for the first angular correction."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sympy as s


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    root = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", type=Path, default=root / "target/debug/rad.exe")
    args = parser.parse_args()
    command = [str(args.rad.resolve()), "projects/dogfood/navier-stokes/angular_correction.rad",
               "--experimental-laws", "--strict-types", "--deny-warnings"]
    run = subprocess.run(command, cwd=root, capture_output=True, text=True, encoding="utf-8",
                         check=True, env={**os.environ, "RAYON_NUM_THREADS": "1"})
    rows = [json.loads(line) for line in run.stdout.splitlines() if line.startswith("{")]
    require([row["power"] for row in rows] == [0, 1, 2], "angular portfolio coverage")
    q = s.symbols("q")
    for row in rows:
        require(row["psi_scale"] == 216216 and len(row["psi"]) == 5, "correction shape")
        chi = (1-q)**row["power"]
        a = chi+s.Rational(2, 3)*q*s.diff(chi, q)
        c = s.Rational(14, 3)*s.diff(chi, q)+s.Rational(4, 3)*q*s.diff(chi, q, 2)
        psi = sum(s.Rational(value, row["psi_scale"])*q**j for j, value in enumerate(row["psi"]))
        h9 = sum(value*q**j for j, value in enumerate(row["h9"]))
        require(s.expand(h9+18*a*c) == 0, "target differs from independent formula")
        require(s.expand(s.Rational(14, 3)*s.diff(psi, q)
                         + s.Rational(4, 3)*q*s.diff(psi, q, 2)+2*a*c) == 0,
                "independent correction ODE failed")
    # Last iteration retained the quadratic collar, which is the main example.
    x, y, z = s.symbols("x y z")
    variables = (x, y, z)
    position = s.Matrix(variables)
    A = s.diag(1, 2, -3)
    B = A*A-s.Rational(14, 3)*s.eye(3)
    radius = position.dot(position)
    H = position.cross(A*position)

    def curl(field):
        return s.Matrix([s.diff(field[2], y)-s.diff(field[1], z),
                         s.diff(field[0], z)-s.diff(field[2], x),
                         s.diff(field[1], x)-s.diff(field[0], y)])

    def zero(field, message):
        require(all(s.expand(entry) == 0 for entry in field), message)

    V = curl(-chi.subs(q, radius)*H/3).applyfunc(s.expand)
    Z = curl(-psi.subs(q, radius)*position.cross(B*position)/3).applyfunc(s.expand)
    zero(position.cross(B*position)+A*H, "second angular direction")
    zero([sum(s.diff(Z[i], variables[i]) for i in range(3))], "correction divergence")
    omega_z = curl(Z)
    zero(omega_z-2*(a*c).subs(q, radius)*A*H, "full vector angular cancellation")
    point = dict(zip(variables, [s.Rational(1, 2)]*3))

    def projection(field):
        return s.simplify((4*field[0]+5*field[1]).subs(point))

    constant = projection(omega_z+curl(V.jacobian(position)*V))
    cross = projection(curl(Z.jacobian(position)*V+V.jacobian(position)*Z))
    viscous = -projection(omega_z.applyfunc(lambda entry: sum(s.diff(entry, v, 2) for v in variables)))
    quadratic = projection(curl(Z.jacobian(position)*Z))
    require(constant == 0, "identified angular term remains at the reference time")
    require(quadratic != 0, "expected self-interaction witness disappeared")
    for probe in ["wrong-sign", "wrong-coefficient"]:
        rejected = subprocess.run(command+["--", probe], cwd=root, capture_output=True,
                                  text=True, encoding="utf-8")
        require(rejected.returncode != 0 and "angular ODE mismatch" in rejected.stdout+rejected.stderr,
                f"{probe} certificate was not rejected correctly")
    with tempfile.TemporaryDirectory(prefix="rad-angular-replay-") as directory:
        trace = Path(directory)/"angular.radr"
        parallel = subprocess.run(command+["--record", str(trace)], cwd=root, capture_output=True,
                                  text=True, encoding="utf-8", check=True,
                                  env={**os.environ, "RAYON_NUM_THREADS": "4"})
        require(parallel.stdout == run.stdout, "worker-count or recording discrepancy")
        replay = subprocess.run([str(args.rad.resolve()), "replay", str(trace)], cwd=root,
                                capture_output=True, text=True, encoding="utf-8", check=True)
        require("Replay verified: world digest matches the recorded run" in replay.stderr,
                "angular replay mismatch")
    print("Three coefficient certificates verified; full 3D divergence and angular cancellation verified.")
    print("Wrong-sign and wrong-coefficient evidence rejected; worker-count and replay checks passed.")
    print(f"At y=(1/2,1/2,1/2), residual projection for u=V+tZ is t*({cross} + nu*({viscous})) + t^2*({quadratic}).")
    print("Cancellation is at the reference time. Remaining interactions are nonzero; no blowup solution certified.")


if __name__ == "__main__":
    main()
