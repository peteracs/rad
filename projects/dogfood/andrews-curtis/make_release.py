#!/usr/bin/env python3
"""Assemble the standalone certificate release (no Rad needed to use it).

    python export.py && python novelty.py --table && python make_release.py <out_dir>

Copies release/ (README, licenses, citation, note), the certificates, the
results table, the checker and the benchmark table into <out_dir>, then runs
the checker there.
"""
import pathlib
import shutil
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent


def main():
    out = pathlib.Path(sys.argv[1]).resolve()
    if out.exists() and any(p.name != ".git" for p in out.iterdir()):
        for p in out.iterdir():
            if p.name != ".git":
                shutil.rmtree(p) if p.is_dir() else p.unlink()
    out.mkdir(parents=True, exist_ok=True)
    shutil.copytree(HERE / "release", out, dirs_exist_ok=True)
    (out / "certificates").mkdir()
    for cert in sorted((HERE / "certificates").glob("*.json")):
        shutil.copy2(cert, out / "certificates" / cert.name)
    shutil.copy2(HERE / "certificates" / "results.tsv", out / "results.tsv")
    shutil.copy2(HERE / "check_certificate.py", out / "check_certificate.py")
    (out / "data").mkdir()
    shutil.copy2(HERE / "data" / "miller_schupp.tsv", out / "data" / "miller_schupp.tsv")
    certs = sorted(str(p) for p in (out / "certificates").glob("*.json"))
    done = subprocess.run([sys.executable, str(out / "check_certificate.py"), *certs], capture_output=True, text=True)
    print(done.stdout.splitlines()[-1])
    sys.exit(done.returncode)


if __name__ == "__main__":
    main()
