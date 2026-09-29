"""Global pressure formula: exact spatial algebra plus non-certified quadrature."""
import argparse
from functools import lru_cache
import json
import os
from pathlib import Path
import subprocess
import tempfile
import mpmath as mp
import sympy as s


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check_algebra():
    x, y, z, r = s.symbols("x y z q", positive=True)
    position = s.Matrix([x, y, z])
    A = s.diag(1, 2, -3)
    q = position.dot(position)
    Q = position.dot(A*position)
    P = position.dot((A*A-s.Rational(14, 3)*s.eye(3))*position)
    H4 = Q**2-s.Rational(4, 7)*q*P-s.Rational(28, 15)*q*q
    chi = s.Function("chi")(r)
    a = chi+s.Rational(2, 3)*r*s.diff(chi, r)
    b = -s.Rational(2, 3)*s.diff(chi, r)
    ap, bp = s.diff(a, r), s.diff(b, r)
    d2 = 4*a*(ap+b)+8*r*ap*b
    d4 = 11*b*b+4*ap*b+4*ap*ap+4*a*bp+12*r*b*bp+8*r*ap*bp+4*r*r*bp*bp
    f0 = -(14*a*a+s.Rational(14, 3)*r*d2+s.Rational(28, 15)*r*r*d4)
    f2 = -(d2+s.Rational(4, 7)*r*d4)
    f4 = -d4
    velocity = a.subs(r, q)*A*position+b.subs(r, q)*Q*position
    jacobian = velocity.jacobian(position)
    actual = -s.trace(jacobian*jacobian)
    expected = f0.subs(r, q)+f2.subs(r, q)*P+f4.subs(r, q)*H4
    require(s.simplify(actual-expected) == 0, "global harmonic source decomposition")
    for harmonic in [P, H4]:
        require(s.expand(sum(s.diff(harmonic, v, 2) for v in position)) == 0, "harmonic basis")
    particular = [-s.Rational(7, 3)*r+s.Rational(49, 15)*r*r-s.Rational(1127, 945)*r**3,
                  s.Rational(2, 7)*r-s.Rational(23, 189)*r*r, -s.Rational(52, 99)*r]
    source = [f0, f2, f4]
    for ell, g, f in zip([0, 2, 4], particular, source):
        local = s.simplify(f.subs(chi, 1-r).doit())
        require(s.expand(4*r*s.diff(g, r, 2)+(4*ell+6)*s.diff(g, r)-local) == 0,
                "collar particular source mismatch")


def quadrature(digits):
    mp.mp.dps = digits
    def step(q, a, b):
        if q <= a:
            return mp.mpf(0)
        if q >= b:
            return mp.mpf(1)
        t = (q-a)/(b-a)
        e0, e1 = mp.exp(-1/t), mp.exp(-1/(1-t))
        return e0/(e0+e1)

    def cutoff(q):
        return (1-q*step(q, mp.mpf(1)/4, mp.mpf(3)/8))*(1-step(q, mp.mpf(5)/8, mp.mpf(1)))

    @lru_cache(maxsize=None)
    def sources(q):
        c, cp, cpp = cutoff(q), mp.diff(cutoff, q), mp.diff(cutoff, q, 2)
        a, b = c+2*q*cp/3, -2*cp/3
        ap, bp = 5*cp/3+2*q*cpp/3, -2*cpp/3
        d2 = 4*a*(ap+b)+8*q*ap*b
        d4 = 11*b*b+4*ap*b+4*ap*ap+4*a*bp+12*q*b*bp+8*q*ap*bp+4*q*q*bp*bp
        return [-(14*a*a+mp.mpf(14)/3*q*d2+mp.mpf(28)/15*q*q*d4),
                -(d2+mp.mpf(4)/7*q*d4), -d4]

    anchor = mp.mpf(1)/2
    left = [mp.mpf(0), mp.mpf(1)/4, mp.mpf(3)/8, anchor]
    right = [anchor, mp.mpf(5)/8, mp.mpf(1)]
    modes = []
    for index, ell in enumerate([0, 2, 4]):
        exponent = mp.mpf(ell)+mp.mpf(1)/2
        integral = mp.quad(lambda q: q**exponent*sources(q)[index], left)
        exterior = mp.quad(lambda q: sources(q)[index], right)
        moment = integral+mp.quad(lambda q: q**exponent*sources(q)[index], right)
        g = -(anchor**(-exponent)*integral+exterior)/(4*ell+2)
        gp = anchor**(-exponent-1)*integral/4
        def local(q):
            if ell == 0:
                return -mp.mpf(7)/3*q+mp.mpf(49)/15*q*q-mp.mpf(1127)/945*q**3
            if ell == 2:
                return mp.mpf(2)/7*q-mp.mpf(23)/189*q*q
            return -mp.mpf(52)/99*q
        d = -(gp-mp.diff(local, anchor))*anchor**(exponent+1)/exponent
        c = g-local(anchor)-d*anchor**(-exponent)
        modes.append(dict(ell=ell, C=c, D=d, moment=moment))
    require(abs(modes[0]["moment"]) < mp.mpf(10)**(-digits//2), "monopole cancellation diagnostic failed")
    return modes


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--quadrature", action="store_true")
    args = parser.parse_args()
    directory = Path(__file__).resolve().parent
    root = directory.parents[2]
    check_algebra()
    command = [str(root/"target/debug/rad.exe"), "projects/dogfood/navier-stokes/global_pressure.rad",
               "--experimental-laws", "--strict-types", "--deny-warnings"]
    run = subprocess.run(command, cwd=root, check=True, capture_output=True, text=True, encoding="utf-8",
                         env={**os.environ, "RAYON_NUM_THREADS": "1"})
    rows = [json.loads(line) for line in run.stdout.splitlines() if line.startswith("{")]
    require(rows == [dict(ell=ell, particular_verified=True, homogeneous_verified=True) for ell in [0, 2, 4]],
            "pressure mode portfolio mismatch")
    require("AssemblePressure" in run.stdout and "SubmitPressure" in run.stdout, "pressure provenance missing")
    with tempfile.TemporaryDirectory(prefix="rad-pressure-match-") as temporary:
        trace = Path(temporary)/"pressure.radr"
        parallel = subprocess.run(command+["--record", str(trace)], cwd=root, check=True,
                                  capture_output=True, text=True, encoding="utf-8",
                                  env={**os.environ, "RAYON_NUM_THREADS": "4"})
        require(parallel.stdout == run.stdout, "pressure worker-count mismatch")
        replay = subprocess.run([command[0], "replay", str(trace)], cwd=root, check=True,
                                capture_output=True, text=True, encoding="utf-8")
        require("Replay verified: world digest matches the recorded run" in replay.stderr, "pressure replay")
    print("General pressure-source decomposition and exact RAD radial identities verified; parallel/replay checks passed.")
    if args.quadrature:
        coarse, fine = quadrature(30), quadrature(45)
        output = []
        for a, b in zip(coarse, fine):
            for key in ["C", "D", "moment"]:
                require(abs(a[key]-b[key]) < mp.mpf('1e-20'), "quadrature precision comparison failed")
            output.append({key: (value if key == 'ell' else mp.nstr(value, 22)) for key, value in b.items()})
        receipt = dict(status="numerical quadrature, not interval certified", cutoff="specified in GLOBAL_PRESSURE_MATCHING.md",
                       anchor_q="1/2", modes=output)
        (directory/"global_pressure_integrals.json").write_text(json.dumps(receipt, indent=2)+"\n", encoding="utf-8")
        print(json.dumps(receipt))
    print("Global matching and finite-energy conclusions use the written analytic argument, not a RAD PDE kernel.")


if __name__ == "__main__":
    main()
