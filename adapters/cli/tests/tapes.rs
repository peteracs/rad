//! Every committed `.radr` trace must replay, or be listed as stale.
//!
//! A trace is a claim about the build that cut it: this program, fed these
//! inputs, ends in this world. The docs quote several as receipts. When the
//! VM, the checker or the digest changes, the claim silently stops holding
//! unless something replays it (GH 0003: all ten committed traces failed
//! `rad replay` and nothing noticed).
//!
//! `tooling/stale-tapes.tsv` is the reviewed debt. It may only shrink: an
//! entry that replays again, or names no committed trace, fails this test.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn committed_traces() -> Vec<String> {
    let output = Command::new("git")
        .current_dir(repo())
        .args(["ls-files", "*.radr"])
        .output()
        .expect("git lists the committed traces");
    assert!(output.status.success(), "git ls-files failed");
    String::from_utf8(output.stdout)
        .expect("utf-8 paths")
        .lines()
        .map(str::to_string)
        .collect()
}

fn stale_list() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(repo().join("tooling/stale-tapes.tsv")).expect("stale list");
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let (path, reason) = line
                .split_once('\t')
                .unwrap_or_else(|| panic!("stale-tapes.tsv: no tab in {line:?}"));
            assert!(
                !reason.trim().is_empty(),
                "stale-tapes.tsv: {path} has no reason"
            );
            (path.to_string(), reason.to_string())
        })
        .collect()
}

#[test]
fn committed_traces_replay_or_are_listed_as_stale() {
    let traces = committed_traces();
    let stale = stale_list();
    let mut problems = Vec::new();

    for path in stale.keys() {
        if !traces.contains(path) {
            problems.push(format!("{path}: listed as stale but not a committed trace"));
        }
    }
    for path in &traces {
        let output = Command::new(env!("CARGO_BIN_EXE_rad"))
            .current_dir(repo())
            .args(["replay", path])
            .output()
            .expect("rad replay runs");
        let replays = output.status.success();
        let last_line = String::from_utf8_lossy(&output.stderr)
            .lines()
            .last()
            .unwrap_or_default()
            .to_string();
        match (replays, stale.contains_key(path)) {
            (true, false) | (false, true) => {}
            (true, true) => problems.push(format!(
                "{path}: replays again; remove it from tooling/stale-tapes.tsv"
            )),
            (false, false) => problems.push(format!(
                "{path}: does not replay ({last_line}); re-record it, or list it in tooling/stale-tapes.tsv with the reason"
            )),
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
