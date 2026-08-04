//! Source-layout invariant for the executable reference contract.

use std::fs;
use std::path::Path;

const MAX_SOURCE_LINES: usize = 1_000;

fn assert_within_limit(path: &Path) {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let lines = source.lines().count();
    assert!(
        lines <= MAX_SOURCE_LINES,
        "{} has {lines} lines; reference files are capped at {MAX_SOURCE_LINES}",
        path.display(),
    );
}

fn visit_rust_sources(path: &Path) {
    let mut entries = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
        .map(|entry| entry.expect("reference source entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            visit_rust_sources(&entry);
        } else if entry.extension().is_some_and(|extension| extension == "rs") {
            assert_within_limit(&entry);
        }
    }
}

#[test]
fn reference_oracle_files_stay_at_or_below_one_thousand_lines() {
    let manifest = option_env!("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .expect("current directory")
                .join("core/vm")
        });
    let tests = Path::new(&manifest).join("tests");
    assert_within_limit(&tests.join("rfc0003_reference.rs"));
    visit_rust_sources(&tests.join("rfc0003_reference"));
}
