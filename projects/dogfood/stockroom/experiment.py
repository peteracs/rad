#!/usr/bin/env python3
"""Lineage against intervention on random stockroom days.

Each trial is a random working day in which a clerk now and then cancels an
order that already shipped (the bug in stockroom.rad). The day is recorded,
and `rad blame` is asked for the cause of the first failed audit line, with
the lineage of that sku's Stock for comparison. The search runs twice, guided
and as plain delta debugging.

    python projects/dogfood/stockroom/experiment.py --rad target/release/rad --trials 40

Prints one row per trial and a summary. Every number comes from the JSON
that `rad blame --json` wrote for that trial.
"""
import argparse
import json
import pathlib
import random
import statistics
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
PROGRAM = HERE / "stockroom.rad"
SKUS = ["bolt", "nut", "washer", "gear", "belt", "chain", "hinge", "spring"]


def random_day(rng: random.Random, lines: int, slip: float) -> list[str]:
    """A day of `lines` commands. `slip` is the chance that a cancellation
    names an order that already shipped."""
    out = []
    on_hand = {sku: 0 for sku in SKUS}
    open_orders: dict[str, tuple[str, int]] = {}
    shipped: list[str] = []
    next_id = 1
    for sku in SKUS:
        qty = rng.randrange(200, 400)
        out.append(f"receive {sku} {qty}")
        on_hand[sku] += qty
    while len(out) < lines - 1:
        roll = rng.random()
        if roll < 0.18:
            sku = rng.choice(SKUS)
            qty = rng.randrange(20, 120)
            out.append(f"receive {sku} {qty}")
            on_hand[sku] += qty
        elif roll < 0.58:
            sku = rng.choice(SKUS)
            qty = rng.randrange(5, 60)
            order = f"o{next_id}"
            next_id += 1
            out.append(f"order {order} {sku} {qty}")
            if on_hand[sku] >= qty:
                on_hand[sku] -= qty
                open_orders[order] = (sku, qty)
        elif roll < 0.86 and open_orders:
            order = rng.choice(sorted(open_orders))
            out.append(f"ship {order}")
            del open_orders[order]
            shipped.append(order)
        elif shipped and rng.random() < slip:
            order = shipped.pop(rng.randrange(len(shipped)))
            out.append(f"cancel {order}")
        elif open_orders:
            order = rng.choice(sorted(open_orders))
            out.append(f"cancel {order}")
            sku, qty = open_orders.pop(order)
            on_hand[sku] += qty
    out.append("done")
    return out


def run(rad: str, args: list[str], stdin: bytes | None = None) -> subprocess.CompletedProcess:
    return subprocess.run([rad, *args], input=stdin, capture_output=True, check=False)


def blame(rad: str, trace: pathlib.Path, symptom: str, sku: str, extra: list[str]) -> dict:
    result = run(
        rad,
        ["blame", str(trace), "--output", symptom, "--why", sku, "Stock", "--json", *extra],
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.decode(errors="replace"))
    return json.loads(result.stdout)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True)
    parser.add_argument("--trials", type=int, default=40)
    parser.add_argument("--lines", type=int, default=400)
    parser.add_argument("--slip", type=float, default=0.04)
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--json", help="write per-trial results to this file")
    args = parser.parse_args()
    # A relative path with forward slashes does not start a process on Windows.
    args.rad = str(pathlib.Path(args.rad).resolve())

    rng = random.Random(args.seed)
    rows = []
    with tempfile.TemporaryDirectory() as scratch:
        trial = 0
        while len(rows) < args.trials:
            trial += 1
            day = random_day(rng, args.lines, args.slip)
            session = ("\n".join(day) + "\n").encode("ascii")
            trace = pathlib.Path(scratch) / f"day{trial}.radr"
            recorded = run(args.rad, ["--record", str(trace), str(PROGRAM)], session)
            failed = [
                line for line in recorded.stdout.decode().splitlines() if line.startswith("AUDIT FAILED")
            ]
            if not failed:
                continue  # a day without a slip has nothing to explain
            sku = failed[0].split()[2].rstrip(":")
            symptom = f"AUDIT FAILED {sku}"
            guided = blame(args.rad, trace, symptom, sku, [])
            plain = blame(args.rad, trace, symptom, sku, ["--strategy", "ddmin", "--no-but-for"])
            cause = guided["minimal_cause"]
            lineage = guided["lineage"]["inputs"]
            texts = {row["input"]: row["value"].strip('"') for row in guided["inputs"]}
            rows.append(
                {
                    "trial": trial,
                    "inputs": len(guided["inputs"]),
                    "sku": sku,
                    "cause": cause,
                    "cause_text": [texts[o] for o in cause],
                    "lineage": lineage,
                    "lineage_in_cause": bool(set(lineage) & set(cause)),
                    "lineage_is_slip": any(
                        texts[o].startswith("cancel") and o in cause for o in lineage
                    ),
                    "from_end": len(guided["inputs"]) - max(cause),
                    "replays_guided": guided["search"]["replays"],
                    "replays_ddmin": plain["search"]["replays"],
                    "ms_guided": guided["search"]["elapsed_ms"],
                    "ms_ddmin": plain["search"]["elapsed_ms"],
                    "same_size": len(plain["minimal_cause"]) == len(cause),
                }
            )
            last = rows[-1]
            print(
                f"day {trial:>3}  {sku:<7} cause {len(cause)} inputs {cause}  "
                f"lineage {lineage} {'in' if last['lineage_in_cause'] else 'NOT in'} cause  "
                f"replays guided {last['replays_guided']} / ddmin {last['replays_ddmin']}",
                flush=True,
            )

    n = len(rows)
    hits = sum(row["lineage_in_cause"] for row in rows)
    print()
    print(f"trials with a failed audit: {n} ({args.lines} commands each, slip chance {args.slip})")
    print(f"cause size: median {statistics.median(len(r['cause']) for r in rows)}, "
          f"max {max(len(r['cause']) for r in rows)}")
    print(f"lineage (last writer of the sku's Stock) names a command of the cause: {hits} of {n}")
    print(f"commands between the cause and the audit: median {statistics.median(r['from_end'] for r in rows)}")
    print(f"replays, guided: median {statistics.median(r['replays_guided'] for r in rows)}; "
          f"ddmin: median {statistics.median(r['replays_ddmin'] for r in rows)}")
    print(f"wall time, guided: median {statistics.median(r['ms_guided'] for r in rows)} ms; "
          f"ddmin: median {statistics.median(r['ms_ddmin'] for r in rows)} ms")
    print(f"guided and ddmin agree on cause size: {sum(r['same_size'] for r in rows)} of {n}")
    if args.json:
        pathlib.Path(args.json).write_text(json.dumps(rows, indent=1) + "\n", encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
