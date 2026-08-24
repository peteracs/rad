#!/usr/bin/env python3
"""Run Sovereign Grid's source-bound, machine-readable acceptance contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import re
import statistics
import subprocess
import sys
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "projects/dogfood/sovereign-grid"
DEFAULT_OUTPUT = ROOT / "target/sovereign-grid-acceptance"


def digest_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def cargo_profile(path: Path) -> str:
    target = (ROOT / "target").resolve()
    if path.parent.parent == target:
        return path.parent.name
    return "custom"


def source_digest() -> str:
    digest = hashlib.sha256()
    roots = [PROJECT, ROOT / "tooling", ROOT / "docs/src/reference"]
    for path in sorted(
        (path for root in roots for path in root.rglob("*") if path.is_file()),
        key=lambda item: item.as_posix(),
    ):
        if any(part in {"target", "book", "__pycache__"} for part in path.parts):
            continue
        relative = path.relative_to(ROOT).as_posix().encode()
        digest.update(len(relative).to_bytes(4, "little"))
        digest.update(relative)
        payload = path.read_bytes()
        digest.update(len(payload).to_bytes(8, "little"))
        digest.update(payload)
    return digest.hexdigest()


def percentile(values: list[int], quantile: float) -> int:
    ordered = sorted(values)
    index = max(0, math.ceil(len(ordered) * quantile) - 1)
    return ordered[index]


def process_metrics(process: subprocess.Popen[Any]) -> tuple[int, int]:
    """Return current resident bytes and CPU nanoseconds for one child."""
    if sys.platform == "win32":
        import ctypes
        from ctypes import wintypes

        class Counters(ctypes.Structure):
            _fields_ = [
                ("cb", wintypes.DWORD),
                ("PageFaultCount", wintypes.DWORD),
                ("PeakWorkingSetSize", ctypes.c_size_t),
                ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t),
                ("PeakPagefileUsage", ctypes.c_size_t),
            ]

        counters = Counters()
        counters.cb = ctypes.sizeof(counters)
        handle = wintypes.HANDLE(int(process._handle))  # type: ignore[attr-defined]
        ok = ctypes.windll.psapi.GetProcessMemoryInfo(
            handle, ctypes.byref(counters), counters.cb
        )
        creation = wintypes.FILETIME()
        exit_time = wintypes.FILETIME()
        kernel = wintypes.FILETIME()
        user = wintypes.FILETIME()
        cpu = 0
        if ctypes.windll.kernel32.GetProcessTimes(
            handle,
            ctypes.byref(creation),
            ctypes.byref(exit_time),
            ctypes.byref(kernel),
            ctypes.byref(user),
        ):
            ticks = (
                (kernel.dwHighDateTime << 32)
                + kernel.dwLowDateTime
                + (user.dwHighDateTime << 32)
                + user.dwLowDateTime
            )
            cpu = ticks * 100
        return (int(counters.WorkingSetSize) if ok else 0, cpu)
    status = Path(f"/proc/{process.pid}/status")
    stat = Path(f"/proc/{process.pid}/stat")
    rss = 0
    cpu = 0
    if status.is_file():
        match = re.search(r"^VmRSS:\s+(\d+)\s+kB$", status.read_text(), re.M)
        if match:
            rss = int(match.group(1)) * 1024
    if stat.is_file():
        fields = stat.read_text().split()
        ticks = int(fields[13]) + int(fields[14])
        cpu = ticks * 1_000_000_000 // os.sysconf("SC_CLK_TCK")
    return rss, cpu


@dataclass
class Outcome:
    name: str
    command: list[str]
    exit_code: int
    expected_exit: list[int]
    pattern: str | None
    pattern_matched: bool
    elapsed_ns: int
    limit_ns: int
    elapsed_pass: bool
    cpu_ns: int
    peak_rss_bytes: int
    stdout: str
    stderr: str
    passed: bool


class Runner:
    def __init__(self, output: Path) -> None:
        self.output = output
        self.logs = output / "logs"
        self.logs.mkdir(parents=True, exist_ok=True)
        self.outcomes: list[Outcome] = []

    def run(
        self,
        name: str,
        command: list[str | Path],
        *,
        expected_exit: tuple[int, ...] = (0,),
        pattern: str | None = None,
        env: dict[str, str] | None = None,
        timeout: int = 7200,
        limit_ns: int = 1_000_000_000,
    ) -> Outcome:
        argv = [str(item) for item in command]
        safe = re.sub(r"[^A-Za-z0-9_.-]+", "-", name).strip("-")
        stdout_path = self.logs / f"{len(self.outcomes) + 1:03d}-{safe}.stdout.txt"
        stderr_path = self.logs / f"{len(self.outcomes) + 1:03d}-{safe}.stderr.txt"
        started = time.perf_counter_ns()
        peak_rss = 0
        cpu_ns = 0
        timed_out = False
        with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
            process = subprocess.Popen(
                argv,
                cwd=ROOT,
                stdout=stdout,
                stderr=stderr,
                env={**os.environ, **(env or {})},
            )
            deadline = time.monotonic() + min(
                float(timeout),
                limit_ns / 1_000_000_000 if limit_ns else float(timeout),
            )
            while process.poll() is None:
                rss, cpu = process_metrics(process)
                peak_rss = max(peak_rss, rss)
                cpu_ns = max(cpu_ns, cpu)
                if time.monotonic() >= deadline:
                    process.kill()
                    timed_out = True
                    break
                time.sleep(0.01)
            process.wait()
            rss, cpu = process_metrics(process)
            peak_rss = max(peak_rss, rss)
            cpu_ns = max(cpu_ns, cpu)
        elapsed = time.perf_counter_ns() - started
        elapsed_pass = limit_ns == 0 or elapsed < limit_ns
        stdout_text = stdout_path.read_text(encoding="utf-8", errors="replace")
        stderr_text = stderr_path.read_text(encoding="utf-8", errors="replace")
        combined = stdout_text + "\n" + stderr_text
        pattern_matched = pattern is None or re.search(pattern, combined, re.I | re.S) is not None
        passed = (
            not timed_out
            and process.returncode in expected_exit
            and pattern_matched
            and elapsed_pass
        )
        outcome = Outcome(
            name=name,
            command=argv,
            exit_code=-1 if timed_out else process.returncode,
            expected_exit=list(expected_exit),
            pattern=pattern,
            pattern_matched=pattern_matched,
            elapsed_ns=elapsed,
            limit_ns=limit_ns,
            elapsed_pass=elapsed_pass,
            cpu_ns=cpu_ns,
            peak_rss_bytes=peak_rss,
            stdout=stdout_path.relative_to(ROOT).as_posix(),
            stderr=stderr_path.relative_to(ROOT).as_posix(),
            passed=passed,
        )
        self.outcomes.append(outcome)
        print(
            f"{'PASS' if passed else 'FAIL'} {name} "
            f"elapsedNs={elapsed} limitNs={limit_ns}",
            flush=True,
        )
        if not passed:
            print(combined[-2000:], flush=True)
        return outcome


def run_protocol_harness(runner: Runner, rad: Path) -> None:
    ready = runner.output / "protocol.ready"
    stop = runner.output / "protocol.stop"
    ready.unlink(missing_ok=True)
    stop.unlink(missing_ok=True)
    harness_out = (runner.logs / "protocol-harness.stdout.txt").open("wb")
    harness_err = (runner.logs / "protocol-harness.stderr.txt").open("wb")
    harness = subprocess.Popen(
        [
            sys.executable,
            str(ROOT / "tooling/sovereign_grid_protocol_harness.py"),
            "--ready",
            str(ready),
            "--stop",
            str(stop),
        ],
        cwd=ROOT,
        stdout=harness_out,
        stderr=harness_err,
    )
    try:
        deadline = time.monotonic() + 10
        while not ready.exists() and harness.poll() is None and time.monotonic() < deadline:
            time.sleep(0.02)
        if not ready.exists():
            raise RuntimeError("protocol harness did not become ready")
        runner.run(
            "host-protocol-builtins",
            [rad, PROJECT / "builtins/host_network.rad", "--strict-types", "--deny-warnings"],
            pattern="sovereign-grid: host network builtins complete",
        )
    finally:
        stop.write_text("stop", encoding="utf-8")
        try:
            harness.wait(timeout=10)
        except subprocess.TimeoutExpired:
            harness.kill()
            harness.wait()
        harness_out.close()
        harness_err.close()
        ready.unlink(missing_ok=True)
        stop.unlink(missing_ok=True)


def run_negatives(runner: Runner, rad: Path) -> None:
    manifest = tomllib.loads((PROJECT / "negative/manifest.toml").read_text(encoding="utf-8"))
    counts: dict[str, int] = {}
    for case in manifest["case"]:
        counts[case["feature"]] = counts.get(case["feature"], 0) + 1
        source = PROJECT / "negative" / case["file"]
        if case.get("setup") == "plugin":
            runner.run(
                "build-grid-oracle-for-negative",
                [sys.executable, ROOT / "tooling/build_sovereign_grid_plugin.py"],
                limit_ns=0,
            )
        if case["mode"] == "model":
            command = [
                rad,
                "model-check",
                source,
                "--runs",
                "1",
                "--max-commands",
                "2",
                "--seed",
                "1",
                "--artifact-dir",
                runner.output / "model-failures",
                "--json",
            ]
        else:
            command = [rad, source, "--strict-types", "--deny-warnings"]
        runner.run(
            f"negative-{case['feature']}-{source.stem}",
            command,
            expected_exit=(1,),
            pattern=case["pattern"],
        )
    missing = sorted(feature for feature, count in counts.items() if count < 3)
    if missing:
        raise RuntimeError(f"major features need three negatives: {', '.join(missing)}")


def run_replay_matrix(runner: Runner, rad: Path) -> dict[str, Any]:
    traces: list[dict[str, Any]] = []
    workers = sorted({1, 2, 4, os.cpu_count() or 1})
    for worker_count in workers:
        trace = runner.output / f"workers-{worker_count}.radr"
        recorded = runner.run(
            f"record-workers-{worker_count}",
            [rad, PROJECT / "main.rad", "--strict-types", "--deny-warnings", "--record", trace],
            pattern="sovereign-grid: stable workflow complete",
            env={"RAYON_NUM_THREADS": str(worker_count)},
        )
        runner.run(
            f"replay-workers-{worker_count}",
            [rad, "replay", trace],
            pattern="Replay verified: world digest matches",
            env={"RAYON_NUM_THREADS": str(worker_count)},
        )
        raw = trace.read_bytes()
        stdout = (ROOT / recorded.stdout).read_text(encoding="utf-8", errors="replace")
        digest_match = re.search(r"sovereign-grid: world-digest ([0-9a-f]{64})", stdout)
        if digest_match is None:
            raise RuntimeError(f"worker {worker_count} did not publish its world digest")
        normalized_output = re.sub(r"(?m)^Recorded trace:.*(?:\r?\n)?", "", stdout)
        traces.append(
            {
                "workers": worker_count,
                "worldDigest": digest_match.group(1),
                "observableOrderAndProvenanceSha256": hashlib.sha256(
                    normalized_output.encode()
                ).hexdigest(),
                "traceSha256": hashlib.sha256(raw).hexdigest(),
                "bytes": len(raw),
            }
        )
    if len({trace["worldDigest"] for trace in traces}) != 1:
        raise RuntimeError("worker-count replay world digests differ")
    if len({trace["observableOrderAndProvenanceSha256"] for trace in traces}) != 1:
        raise RuntimeError("worker-count event order or provenance output differs")
    return {"workers": traces}


def run_ffi_recorded_replay(runner: Runner, rad: Path) -> None:
    plugin = ROOT / "target/sovereign-grid-plugin/grid-oracle.radext"
    held = plugin.with_suffix(".radext.held-for-replay-proof")
    trace = runner.output / "ffi-recorded-response.radr"
    runner.run(
        "ffi-record-native",
        [
            rad,
            PROJECT / "builtins/ffi_native.rad",
            "--strict-types",
            "--deny-warnings",
            "--record",
            trace,
        ],
        pattern="FFI native builtins complete",
    )
    held.unlink(missing_ok=True)
    plugin.replace(held)
    try:
        runner.run(
            "ffi-replay-without-live-plugin",
            [rad, "replay", trace],
            pattern="Replay verified: world digest matches",
        )
    finally:
        if held.exists():
            held.replace(plugin)


def benchmark(runner: Runner, rad: Path, samples: int) -> dict[str, Any]:
    results: dict[str, list[dict[str, Any]]] = {"baseline": [], "indexed": []}
    for mode in ("baseline", "indexed"):
        warmup = runner.run(
            f"benchmark-{mode}-warmup",
            [rad, "bench", PROJECT / "bench.rad", "--json", "--", mode, "1"],
        )
        if not warmup.passed:
            raise RuntimeError(
                f"{warmup.name} failed; inspect {warmup.stdout} and {warmup.stderr}"
            )
        for sample in range(samples):
            outcome = runner.run(
                f"benchmark-{mode}-{sample + 1:02d}",
                [rad, "bench", PROJECT / "bench.rad", "--json", "--", mode, "1"],
            )
            if not outcome.passed:
                raise RuntimeError(
                    f"{outcome.name} failed; inspect {outcome.stdout} and {outcome.stderr}"
                )
            payload = json.loads((ROOT / outcome.stdout).read_text(encoding="utf-8"))
            system = payload["systems"][0]
            results[mode].append(
                {
                    "elapsedNs": payload["elapsed_ns"],
                    "processCpuNs": outcome.cpu_ns,
                    "peakRssBytes": outcome.peak_rss_bytes,
                    "worldDigest": payload["world_digest"],
                    "instructions": system["total_instructions"],
                    "guestAllocations": system["total_guest_allocations"],
                    "guestAllocatedBytes": system["total_guest_allocated_bytes"],
                    "runtimeAllocations": system["total_runtime_allocations"],
                    "runtimeAllocatedBytes": system["total_runtime_allocated_bytes"],
                    "hostAllocations": system["total_host_boundary_allocations"],
                    "hostAllocatedBytes": system["total_host_boundary_allocated_bytes"],
                    "managedAllocations": system["total_managed_backing_allocations"],
                    "managedAllocatedBytes": system["total_managed_backing_bytes"],
                }
            )
    digests = {sample["worldDigest"] for mode in results.values() for sample in mode}
    if len(digests) != 1:
        raise RuntimeError("baseline and indexed benchmark business digests differ")
    summary: dict[str, Any] = {"samplesPerMode": samples, "worldDigest": digests.pop(), "modes": {}}
    for mode, samples_for_mode in results.items():
        elapsed = [sample["elapsedNs"] for sample in samples_for_mode]
        summary["modes"][mode] = {
            "medianElapsedNs": int(statistics.median(elapsed)),
            "p95ElapsedNs": percentile(elapsed, 0.95),
            "medianProcessCpuNs": int(
                statistics.median(sample["processCpuNs"] for sample in samples_for_mode)
            ),
            "peakRssBytes": max(sample["peakRssBytes"] for sample in samples_for_mode),
            "instructions": samples_for_mode[0]["instructions"],
            "allocations": {
                key: max(sample[key] for sample in samples_for_mode)
                for key in (
                    "guestAllocations",
                    "guestAllocatedBytes",
                    "runtimeAllocations",
                    "runtimeAllocatedBytes",
                    "hostAllocations",
                    "hostAllocatedBytes",
                    "managedAllocations",
                    "managedAllocatedBytes",
                )
            },
        }
    baseline = summary["modes"]["baseline"]
    indexed = summary["modes"]["indexed"]
    summary["instructionSpeedup"] = baseline["instructions"] / indexed["instructions"]
    summary["medianWallSpeedup"] = baseline["medianElapsedNs"] / indexed["medianElapsedNs"]
    if summary["instructionSpeedup"] < 100:
        raise RuntimeError("deterministic semantic-work speedup is below 100x")
    if summary["medianWallSpeedup"] < 100:
        raise RuntimeError("median measured-entry wall speedup is below 100x")
    if indexed["allocations"]["guestAllocations"] or indexed["allocations"]["hostAllocations"]:
        raise RuntimeError("indexed hot root allocated in a forbidden category")
    return summary


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", type=Path, default=Path("target/release/rad.exe" if sys.platform == "win32" else "target/release/rad"))
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--benchmark-samples", type=int, default=30)
    parser.add_argument("--model-runs", type=int, default=10_000)
    args = parser.parse_args()
    rad = (ROOT / args.rad).resolve() if not args.rad.is_absolute() else args.rad.resolve()
    output = (ROOT / args.output).resolve() if not args.output.is_absolute() else args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    runner = Runner(output)

    # These are repository orchestrators, not RAD invocations. The language
    # and documentation gates enforce and record the one-second deadline on
    # every child RAD process they own; timing the aggregate Python process as
    # one guest run would punish complete coverage for launching more checks.
    runner.run("language-surface", [sys.executable, ROOT / "tooling/check_language_surface.py", "--rad", rad], limit_ns=0)
    runner.run("normative-doc-examples", [sys.executable, ROOT / "tooling/check_rad_doc_examples.py"], limit_ns=0)
    runner.run("host-api-surface", [sys.executable, ROOT / "tooling/host_api_surface.py", "--check"], limit_ns=0)
    runner.run("stable-main", [rad, PROJECT / "main.rad", "--strict-types", "--deny-warnings"], pattern="sovereign-grid: stable workflow complete")
    runner.run("workflow-tests", [rad, "test", PROJECT / "tests"], pattern="Results: 10 passed, 0 failed")
    runner.run("causal-settlement", [rad, PROJECT / "experimental/causal_dispatch.rad", "--experimental-laws", "--strict-types", "--deny-warnings"], pattern="causal settlement complete")
    runner.run("relation-schema", [rad, "relations", "check", PROJECT / "experimental/relations.rad", "--experimental-relations", "--module", "sovereign::grid"])
    runner.run("relation-builtins", [rad, PROJECT / "experimental/relation_builtins.rad", "--experimental-laws", "--relation-schema", PROJECT / "experimental/relation_runtime.rad", "--relation-module", "sovereign::surface", "--experimental-relations", "--strict-types", "--deny-warnings"], pattern="relation builtins complete")
    runner.run(
        "build-grid-oracle",
        [sys.executable, ROOT / "tooling/build_sovereign_grid_plugin.py"],
        limit_ns=0,
    )
    runner.run("ffi-verify", [rad, "ffi", "verify", ROOT / "target/sovereign-grid-plugin/grid-oracle.radext", "--json"], pattern='"layout_agreement"\\s*:\\s*true')
    runner.run("ffi-native-reference", [rad, PROJECT / "builtins/ffi_native.rad", "--strict-types", "--deny-warnings"], pattern="FFI native builtins complete")
    run_ffi_recorded_replay(runner, rad)
    run_protocol_harness(runner, rad)
    run_negatives(runner, rad)
    replay = run_replay_matrix(runner, rad)
    runner.run("model-campaign", [rad, "model-check", PROJECT / "tests/grid_model.rad", "--model", "GridLifecycleModel", "--runs", str(args.model_runs), "--max-commands", "200", "--seed", "1234", "--artifact-dir", output / "model-campaign", "--json"], pattern='"failures"\\s*:\\s*\\[\\s*\\]', timeout=14_400)
    for name, command in (
        ("effects", ["effects", "AccountActiveMissions", "--file", PROJECT / "main.rad", "--json"]),
        ("path", ["path", "execute_cycle", "->", "CompleteMission", "--file", PROJECT / "main.rad", "--json"]),
        ("query-plan", ["query-plan", "ActiveMissions", "--file", PROJECT / "main.rad", "--json"]),
        ("cost-path", ["cost-path", "AccountActiveMissions", "--file", PROJECT / "main.rad", "--json"]),
        ("why", ["why", "1002", "AuditTrail", "--file", PROJECT / "main.rad", "--json"]),
        ("why-field", ["why-field", "1002", "AuditTrail", "revision", "--file", PROJECT / "main.rad", "--json"]),
        ("why-removed", ["why-removed", "1002", "ActiveMission", "--file", PROJECT / "main.rad", "--json"]),
        ("why-not-in-view", ["why-not-in-view", "ActiveMissions", "1002", "--file", PROJECT / "main.rad", "--json"]),
    ):
        runner.run(f"inspection-{name}", [rad, *command])
    benchmark_report = benchmark(runner, rad, args.benchmark_samples)
    worker = rad.with_name("rad-ffi-worker.exe" if os.name == "nt" else "rad-ffi-worker")
    if not worker.is_file():
        raise RuntimeError(f"missing release FFI worker: {worker}")

    report = {
        "kind": "rad_sovereign_grid_acceptance_v1",
        "passed": all(outcome.passed for outcome in runner.outcomes),
        "generatedUtc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "sourceDigest": source_digest(),
        "binary": {
            "path": rad.relative_to(ROOT).as_posix(),
            "sha256": digest_file(rad),
            "ffiWorkerPath": worker.relative_to(ROOT).as_posix(),
            "ffiWorkerSha256": digest_file(worker),
            "profile": cargo_profile(rad),
        },
        "toolchain": subprocess.check_output(["rustc", "-vV"], cwd=ROOT, text=True),
        "machine": {"platform": platform.platform(), "processors": os.cpu_count()},
        "replay": replay,
        "benchmark": benchmark_report,
        "outcomes": [outcome.__dict__ for outcome in runner.outcomes],
    }
    report_path = output / "report.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Sovereign Grid report: {report_path.relative_to(ROOT)}")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
