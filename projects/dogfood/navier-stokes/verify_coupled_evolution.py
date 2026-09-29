"""Independent finite time-jet checks and RAD execution/replay. Requires SymPy."""
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


def symbolic_checks(directory):
    x, y, z = s.symbols("x y z")
    coordinates = (x, y, z)
    position = s.Matrix(coordinates)
    A = s.diag(1, 2, -3)
    B = A*A-s.Rational(14, 3)*s.eye(3)
    q = position.dot(position)
    H = position.cross(A*position)

    def curl(v):
        return s.Matrix([s.diff(v[2], y)-s.diff(v[1], z),
                         s.diff(v[0], z)-s.diff(v[2], x),
                         s.diff(v[1], x)-s.diff(v[0], y)])

    def lap(p):
        return s.expand(sum(s.diff(p, coordinate, 2) for coordinate in coordinates))

    def zero(v, message):
        require(all(s.expand(entry) == 0 for entry in v), message)

    initial = curl(-(1-q)*H/3)
    pair = initial+curl(-q*position.cross(B*position)/3)
    zero([position.dot(curl(pair.jacobian(position)*pair))-s.Rational(560, 3)*x*y*z],
         "two-mode radial leakage witness failed")
    cases = json.loads((directory/"coupled_jet_data.json").read_text())
    require([case["nu"] for case in cases] == [1, 2], "symbolic case coverage")
    witnesses = []
    for case in cases:
        require(case["monomial_base"] == 12, "jet monomial encoding")
        fields = []
        for scale, terms in zip(case["scales"], case["terms"]):
            fields.append(sum(s.Rational(value, scale)*x**(slot//144)*y**((slot//12)%12)*z**(slot%12)
                              for slot, value in terms))
        u0, u1, u2 = [s.Matrix(fields[start:start+3]) for start in [0, 3, 6]]
        p0, p1 = fields[9:11]
        u3 = s.Matrix(fields[11:14])
        p2 = fields[14]
        nu = case["nu"]
        zero(u0-initial, "independent initial-data mismatch")
        for u in [u0, u1, u2, u3]:
            zero([sum(s.diff(u[i], coordinates[i]) for i in range(3))], "independent divergence mismatch")
        zero(u1+u0.jacobian(position)*u0-nu*u0.applyfunc(lap)
             +s.Matrix([s.diff(p0, coordinate) for coordinate in coordinates]), "independent momentum order zero")
        zero(2*u2+u1.jacobian(position)*u0+u0.jacobian(position)*u1-nu*u1.applyfunc(lap)
             +s.Matrix([s.diff(p1, coordinate) for coordinate in coordinates]), "independent momentum order one")
        # The previously nonzero order-two residual is now canceled.
        r2 = u2.jacobian(position)*u0+u1.jacobian(position)*u1+u0.jacobian(position)*u2-nu*u2.applyfunc(lap)
        zero(3*u3+r2+s.Matrix([s.diff(p2, coordinate) for coordinate in coordinates]),
             "independent momentum order two")
        r3 = (u3.jacobian(position)*u0+u2.jacobian(position)*u1
              +u1.jacobian(position)*u2+u0.jacobian(position)*u3-nu*u3.applyfunc(lap))
        point = dict(zip(coordinates, [s.Rational(1, 2)]*3))
        witness = [s.simplify(entry.subs(point)) for entry in curl(r3)]
        require(any(entry != 0 for entry in witness), "expected unresolved order-three residual missing")
        witnesses.append((nu, [str(entry) for entry in witness]))
    return witnesses


def main():
    directory = Path(__file__).resolve().parent
    root = directory.parents[2]
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", type=Path, default=root/"target/debug/rad.exe")
    args = parser.parse_args()
    witnesses = symbolic_checks(directory)
    command = [str(args.rad.resolve()), "projects/dogfood/navier-stokes/coupled_evolution.rad",
               "--experimental-laws", "--strict-types", "--deny-warnings"]
    baseline = subprocess.run(command, cwd=root, check=True, capture_output=True, text=True,
                              encoding="utf-8", env={**os.environ, "RAYON_NUM_THREADS": "1"})
    rows = [json.loads(line) for line in baseline.stdout.splitlines() if line.startswith("{")]
    require(rows == [dict(nu=nu, divergence_checked=4, momentum_orders=3) for nu in [1, 2]],
            "RAD jet result mismatch")
    require("AssembleJets" in baseline.stdout and "SubmitJet" in baseline.stdout, "jet provenance missing")
    for probe, diagnostic in [("pressure-zero", "jet momentum order zero mismatch"),
                              ("pressure-one", "jet momentum order one mismatch"),
                              ("pressure-two", "jet momentum order two mismatch"),
                              ("product-overflow", "polynomial product bound")]:
        rejected = subprocess.run(command+["--", probe], cwd=root, capture_output=True,
                                  text=True, encoding="utf-8")
        require(rejected.returncode != 0 and diagnostic in rejected.stdout+rejected.stderr,
                f"{probe} not rejected at the intended momentum check")
    with tempfile.TemporaryDirectory(prefix="rad-coupled-jet-") as temporary:
        trace = Path(temporary)/"coupled.radr"
        parallel = subprocess.run(command+["--record", str(trace)], cwd=root, check=True,
                                  capture_output=True, text=True, encoding="utf-8",
                                  env={**os.environ, "RAYON_NUM_THREADS": "4"})
        require(parallel.stdout == baseline.stdout, "worker-count or recording discrepancy")
        replay = subprocess.run([str(args.rad.resolve()), "replay", str(trace)], cwd=root,
                                check=True, capture_output=True, text=True, encoding="utf-8")
        require("Replay verified: world digest matches the recorded run" in replay.stderr,
                "jet replay did not verify")
    print("RAD verified all spatial coefficients of momentum orders 0, 1, 2 and divergence orders 0, 1, 2, 3 for nu=1,2.")
    print("Independent symbolic checks, pressure corruption rejection, worker-count and replay checks passed.")
    print("Two-mode radial leakage witness: y dot curl N = (560/3)xyz.")
    for nu, witness in witnesses:
        print(f"Unresolved order-three curl at (1/2,1/2,1/2), nu={nu}: {witness}")
    print("Local polynomial jet only: no finite-energy global solution, convergence or blowup certified.")


if __name__ == "__main__":
    main()
