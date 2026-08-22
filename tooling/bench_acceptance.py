"""Record and enforce a dogfood benchmark's acceptance manifest.

`rad bench <file> --json` already reports the deterministic half of a run:
world digest, per-system instruction counts, and guest-allocation counts. This
wraps it with the workload identity and the tighten-only rule, so a run either
reproduces the accepted result or fails loudly.

Deterministic metrics (digest, instructions, allocations) must match or
improve. Wall clock is recorded for context but never gates: it is not stable
across machines, so treat it as a reading, not a bound.

    python tooling/bench_acceptance.py record <manifest.json> <bench.rad>
    python tooling/bench_acceptance.py check  <manifest.json> <bench.rad>
"""

import json
import subprocess
import sys
from pathlib import Path

RAD = Path("target/release/rad.exe")

# Bump when the meaning of a recorded metric changes, not when a value does.
# v1 counted executed opcodes, which charged 4 instructions for a fused
# `visit_view` over 600 entities; v2 charges each kernel for the work it
# performs, so the reported number and the `@budget` contract agree. v3
# follows `@no_allocation` splitting into guest/runtime/host categories:
# one pass/fail flag became three, and the runtime counter it used to
# hide is now reported.
SCHEMA_VERSION = 3
METRIC_SEMANTICS = "semantic-work-v1+alloc-categories-v1"


def measure(bench: Path) -> dict:
    if not RAD.exists():
        raise SystemExit(f"{RAD} is missing; build it with `cargo build --release -p rad-cli`")
    result = subprocess.run(
        [str(RAD.resolve()), "bench", bench.name, "--json"],
        cwd=bench.parent,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit(f"bench failed ({result.returncode}):\n{result.stderr.strip()}")
    start = result.stdout.find("{")
    if start < 0:
        raise SystemExit(f"bench produced no JSON:\n{result.stdout.strip()}")
    return json.loads(result.stdout[start:])


def systems_of(report: dict) -> dict:
    return {system["name"]: system for system in report.get("systems", [])}


def check(accepted: dict, observed: dict) -> list[str]:
    failures = []
    if accepted["world_digest"] != observed["world_digest"]:
        failures.append(
            f"world digest changed: accepted {accepted['world_digest']}, "
            f"observed {observed['world_digest']}"
        )

    accepted_systems = systems_of(accepted)
    observed_systems = systems_of(observed)
    for name, was in accepted_systems.items():
        now = observed_systems.get(name)
        if now is None:
            failures.append(f"system {name} disappeared from the report")
            continue
        failures.extend(check_system(name, was, now))
    for name in observed_systems.keys() - accepted_systems.keys():
        failures.append(f"system {name} is not in the manifest; re-record it deliberately")
    return failures


def check_system(name: str, was: dict, now: dict) -> list[str]:
    """Compare one system, ratcheting whatever the manifest recorded.

    Driven by the recorded keys rather than a hard-coded list: when the bench
    starts reporting a new counter, re-recording the manifest is enough to put
    it under the ratchet. A fixed list silently stops covering new metrics.
    """
    failures = []
    for key, before in sorted(was.items()):
        if key in ("name", "instruction_budget"):
            continue
        after = now.get(key)
        if after is None:
            # Absent is not the same as failing. Reporting a vanished metric as
            # a regression sends the reader hunting for a fault that does not
            # exist; the honest answer is that the bench no longer reports it.
            failures.append(
                f"{name}.{key} is no longer reported by this binary "
                "(metric renamed or removed); re-record the manifest deliberately"
            )
            continue
        if key.endswith("_pass"):
            if before and not after:
                failures.append(f"{name}.{key} regressed from pass to fail")
        elif key.endswith("_declared"):
            if before and not after:
                failures.append(
                    f"{name}.{key} was dropped: the contract is no longer declared"
                )
        elif isinstance(before, bool) or isinstance(after, bool):
            if before != after:
                failures.append(f"{name}.{key} changed from {before} to {after}")
        elif isinstance(before, (int, float)) and after > before:
            failures.append(
                f"{name}.{key} rose from {before} to {after} (this ratchet only tightens)"
            )
    for key in sorted(set(now) - set(was)):
        if key.endswith(("_pass", "_declared")) or key.startswith(("max_", "total_")):
            failures.append(
                f"{name}.{key} is reported but not in the manifest; "
                "re-record so it is covered"
            )
    return failures


def main() -> int:
    if len(sys.argv) != 4 or sys.argv[1] not in {"record", "check"}:
        print(__doc__)
        return 2
    mode, manifest_path, bench_path = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    observed = measure(bench_path)

    if mode == "record":
        manifest = {
            "schema_version": SCHEMA_VERSION,
            "metric_semantics": METRIC_SEMANTICS,
            "bench": bench_path.as_posix(),
            "build_profile": "release",
            "world_digest": observed["world_digest"],
            "systems": observed["systems"],
            "elapsed_ns_when_recorded": observed["elapsed_ns"],
        }
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        print(f"recorded {manifest_path}")
        return 0

    # utf-8-sig: PowerShell 5.1's Set-Content writes a BOM, and a hand-edited
    # manifest should report a bad field, not a JSON stack trace.
    accepted = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
    recorded_semantics = accepted.get("metric_semantics")
    if recorded_semantics != METRIC_SEMANTICS:
        # Refusing here is the point: instruction counts recorded under
        # "opcode-count-v0" omitted the work fused kernels performed, so
        # comparing them against semantic-work numbers would read a corrected
        # meter as a regression. Re-record deliberately instead.
        print(
            f"  manifest was recorded under {recorded_semantics!r}, this binary "
            f"reports {METRIC_SEMANTICS!r}"
        )
        print(f"{bench_path}: metric semantics changed; re-record the manifest")
        return 1
    failures = check(accepted, observed)
    for failure in failures:
        print(f"  {failure}")
    if failures:
        print(f"{bench_path}: {len(failures)} acceptance failure(s)")
        return 1
    print(
        f"{bench_path}: digest, instruction, and allocation budgets hold "
        f"(elapsed {observed['elapsed_ns'] / 1e9:.3f}s)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
