import csv
import os
import subprocess
import time
from datetime import UTC, datetime

REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
EXAMPLES_DIR = os.path.join(REPO_ROOT, "examples")
REPORTS_DIR = os.path.join(REPO_ROOT, "reports")
RAD_CLI_NAME = "rad.exe" if os.name == "nt" else "rad"
RAD_CLI = os.path.join(REPO_ROOT, "target", "release", RAD_CLI_NAME)

def main():
    if not os.path.exists(RAD_CLI):
        print(f"Error: {RAD_CLI} not found.")
        print("Please build the VM first: cargo build -p rad-vm --release")
        return 2

    os.makedirs(REPORTS_DIR, exist_ok=True)

    examples = [f for f in os.listdir(EXAMPLES_DIR) if f.endswith(".rad")]
    examples.sort()

    results = []
    print(f"Running {len(examples)} examples...")

    for ex in examples:
        path = os.path.join(EXAMPLES_DIR, ex)
        start = time.perf_counter()
        try:
            proc = subprocess.run(
                [RAD_CLI, path],
                capture_output=True,
                stdin=subprocess.DEVNULL,
                timeout=10,
                text=True
            )
            elapsed_ms = (time.perf_counter() - start) * 1000
            status = "PASS" if proc.returncode == 0 else "FAIL"
            if status == "FAIL":
                print(f"  [FAIL] {ex} (Exit code: {proc.returncode})")
        except subprocess.TimeoutExpired:
            elapsed_ms = 10000.0
            status = "TIMEOUT"
            print(f"  [TIMEOUT] {ex}")
        
        results.append({
            "Example": ex,
            "Status": status,
            "Time (ms)": elapsed_ms
        })

    # Write CSV
    csv_path = os.path.join(REPORTS_DIR, "examples_matrix_latest.csv")
    with open(csv_path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=["Example", "Status", "Time (ms)"])
        writer.writeheader()
        for r in results:
            writer.writerow({
                "Example": r["Example"],
                "Status": r["Status"],
                "Time (ms)": f"{r['Time (ms)']:.2f}"
            })

    # Write MD
    md_path = os.path.join(REPORTS_DIR, "examples_matrix_latest.md")
    passed = sum(1 for r in results if r["Status"] == "PASS")
    total = len(results)
    now = datetime.now(UTC).strftime("%Y-%m-%d %H:%M:%S UTC")

    with open(md_path, "w", encoding="utf-8") as f:
        f.write(f"# Example Matrix ({now})\n\n")
        f.write(f"- Total: {total}\n")
        f.write(f"- Pass: {passed}\n")
        f.write(f"- Fail/Timeout: {total - passed}\n\n")
        f.write("## Results\n\n")
        f.write("| Example | Status | Time (ms) |\n")
        f.write("|---|---|---:|\n")
        for r in results:
            f.write(f"| {r['Example']} | {r['Status']} | {r['Time (ms)']:.2f} |\n")

    print(f"\nDone! Passed {passed}/{total}.")
    print(f"Reports saved to {REPORTS_DIR}/")
    return 0 if passed == total else 1

if __name__ == "__main__":
    raise SystemExit(main())
