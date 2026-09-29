"""Independent exact checks for the limited calculations in VISCOSITY_AUDIT.md."""

from fractions import Fraction
import argparse
import json
from pathlib import Path
import subprocess


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def main() -> None:
    # Direct rational eigenvalue comparison: sqrt(bc)=6 in these examples.
    frequencies = [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 2449, 2450, 4096]
    for frequency in frequencies:
        damping = Fraction(frequency**2, 1_000_000)
        require((6 > damping) == (frequency <= 2449), "mode threshold mismatch")
    require(Fraction(2449**2, 1_000_000) < 6, "lower boundary")
    require(Fraction(2450**2, 1_000_000) > 6, "upper boundary")

    # Check the radial operator identities on y2^k = r^(2k)/2^k.
    # Coefficients below multiply r^(2k-2); axial derivatives are unchanged.
    for k in range(25):
        for radial_drift, y_drift in [(-1, 0), (3, 4)]:
            cylindrical = Fraction(2*k*(2*k-1) + radial_drift*2*k, 2**k)
            transformed = Fraction(4*k*(k-1) + 2*y_drift*k, 2**k)
            require(cylindrical == transformed, "coordinate identity mismatch")

    # Explicitly preserve equality as neutral rather than positive growth.
    require(6 - Fraction(6) == 0, "neutral mode misclassified")
    root = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", type=Path, default=root / "target/debug/rad.exe")
    binary = parser.parse_args().rad.resolve()
    process = subprocess.run(
        [str(binary), "projects/dogfood/navier-stokes/frozen_mode.rad",
         "--strict-types", "--deny-warnings"],
        cwd=root, text=True, capture_output=True, check=True,
    )
    rows = [json.loads(line) for line in process.stdout.splitlines() if line.startswith("{")]
    require([row["frequency"] for row in rows] == frequencies, "missing, duplicated or reordered cases")
    for row in rows:
        n = row["frequency"]
        damping = Fraction(n*n, 1_000_000)
        require(Fraction(row["damping_numerator"], row["damping_denominator"]) == damping,
                "RAD damping does not match independent rational calculation")
        require(row["positive_eigenvalue"] == (6 > damping), "RAD growth flag mismatch")
        require(row["neutral"] == (6 == damping), "RAD neutral flag mismatch")
        require(Fraction(row["growth_margin"], 10**12) == 36-damping*damping,
                "RAD exact margin mismatch")
    print("verified 15 RAD mode outputs against rational arithmetic and 50 radial monomial identities")
    print("scope: finite algebra checks; analytic derivation and PDE existence remain separate")


if __name__ == "__main__":
    main()
