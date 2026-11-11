//! `rad blame`, end to end through the real binary: record a session from
//! stdin, ask which inputs caused the symptom, then feed the answer back to
//! the program as a fresh session and watch the symptom come back.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn rad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rad"))
}

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

/// A scratch directory unique to one test of one test process.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rad-blame-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn run_with_stdin(mut command: Command, stdin: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rad starts");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin)
        .expect("session written");
    child.wait_with_output().expect("rad exits")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

/// Record `program` fed `session` on stdin; returns the trace path.
fn record(dir: &Path, program: &Path, session: &[u8]) -> PathBuf {
    let trace = dir.join("session.radr");
    let mut command = rad();
    command.arg("--record").arg(&trace).arg(program);
    let output = run_with_stdin(command, session);
    assert!(
        trace.is_file(),
        "no trace recorded:\n{}\n{}",
        stdout(&output),
        stderr(&output)
    );
    trace
}

fn blame_json(trace: &Path, options: &[&str]) -> serde_json::Value {
    let output = rad()
        .arg("blame")
        .arg(trace)
        .args(options)
        .arg("--json")
        .output()
        .expect("rad blame runs");
    assert!(
        output.status.success(),
        "rad blame failed:\n{}",
        stderr(&output)
    );
    serde_json::from_str(&stdout(&output)).expect("one JSON report on stdout")
}

fn cause_values(report: &serde_json::Value) -> Vec<String> {
    report["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .filter(|input| input["in_minimal_cause"] == true)
        .map(|input| input["value"].as_str().expect("value").to_string())
        .collect()
}

#[test]
fn stockroom_incident_is_traced_to_four_commands_of_four_hundred() {
    let dir = scratch("stockroom");
    let program = repo("projects/dogfood/stockroom/stockroom.rad");
    let session = std::fs::read(repo("projects/dogfood/stockroom/session.txt")).expect("session");
    let trace = record(&dir, &program, &session);

    let script = dir.join("minimal.txt");
    let report = blame_json(
        &trace,
        &[
            "--output",
            "AUDIT FAILED hinge",
            "--why",
            "hinge",
            "Stock",
            "--write-script",
            script.to_str().expect("utf-8 path"),
        ],
    );
    assert_eq!(report["format"], "rad-blame");
    assert_eq!(report["version"], 1);
    assert_eq!(report["inputs"].as_array().expect("inputs").len(), 400);

    // The cause: stock to reserve from, the order, its shipment, and the
    // cancellation that came after it. 396 other commands are not needed.
    let cause = cause_values(&report);
    assert_eq!(cause.len(), 4, "{cause:?}");
    // Several deliveries could have stocked the order; the report holds one.
    assert!(cause[0].starts_with("\"receive hinge "), "{cause:?}");
    assert_eq!(
        cause[1..],
        ["\"order o17 hinge 56\"", "\"ship o17\"", "\"cancel o17\""]
    );

    // Lineage of the shelf count is its last writer: a routine command late
    // in the day that has nothing to do with the discrepancy.
    let lineage: Vec<u64> = report["lineage"]["inputs"]
        .as_array()
        .expect("lineage inputs")
        .iter()
        .map(|ordinal| ordinal.as_u64().expect("ordinal"))
        .collect();
    let cause_ordinals: Vec<u64> = report["minimal_cause"]
        .as_array()
        .expect("minimal cause")
        .iter()
        .map(|ordinal| ordinal.as_u64().expect("ordinal"))
        .collect();
    assert_eq!(lineage.len(), 1, "one command wrote the value last");
    assert!(
        !cause_ordinals.contains(&lineage[0]),
        "the last writer (#{}) is not part of the cause {cause_ordinals:?}",
        lineage[0]
    );
    assert!(lineage[0] > *cause_ordinals.last().expect("cause") + 100);

    // The claim, checked outside the replay machinery: the four commands,
    // typed into a fresh process, fail the audit; any three do not.
    let minimal = std::fs::read_to_string(&script).expect("minimal script");
    let lines: Vec<&str> = minimal.lines().collect();
    assert_eq!(lines.len(), 4);
    let mut command = rad();
    command.arg(&program);
    let rerun = run_with_stdin(command, minimal.as_bytes());
    assert!(
        stdout(&rerun).contains("AUDIT FAILED hinge"),
        "{}",
        stdout(&rerun)
    );
    for skip in 0..lines.len() {
        let fewer: Vec<&str> = lines
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != skip)
            .map(|(_, line)| *line)
            .collect();
        let mut command = rad();
        command.arg(&program);
        let rerun = run_with_stdin(command, (fewer.join("\n") + "\n").as_bytes());
        assert!(
            stdout(&rerun).contains("audit clean"),
            "without {:?} the audit must pass:\n{}",
            lines[skip],
            stdout(&rerun)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_is_explained_without_naming_a_symptom() {
    let dir = scratch("crash");
    let program = dir.join("till.rad");
    std::fs::write(
        &program,
        r#"let mut cash = 100
let mut open = true
while open {
    let line = trim(readline())
    if line == "close" or line == "" {
        open = false
    } else if line == "refund" {
        cash = cash - 80
    } else if line == "sale" {
        cash = cash + 30
    }
    assert(cash >= 0, "till overdrawn")
}
"#,
    )
    .expect("program written");
    let trace = record(
        &dir,
        &program,
        b"sale\nrefund\nnote\nsale\nrefund\nrefund\nclose\n",
    );
    let report = blame_json(&trace, &[]);
    assert_eq!(report["symptom"]["kind"], "error");
    // The recorded day needed its third refund to go under: 100 +30 -80 +30
    // -80 -80. Without the sales, two refunds are enough (100 - 160), and
    // one is not: the sales were hiding how close the till was.
    assert_eq!(cause_values(&report), ["\"refund\"", "\"refund\""]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn blame_refuses_what_it_cannot_answer() {
    let dir = scratch("refuse");
    let program = dir.join("echo.rad");
    std::fs::write(&program, "let line = readline()\nprint(line)\n").expect("program written");
    let trace = record(&dir, &program, b"hello\n");

    let no_symptom = rad().arg("blame").arg(&trace).output().expect("runs");
    assert_eq!(no_symptom.status.code(), Some(1));
    assert!(stderr(&no_symptom).contains("say which outcome to explain"));

    let absent = rad()
        .arg("blame")
        .arg(&trace)
        .args(["--output", "goodbye"])
        .output()
        .expect("runs");
    assert_eq!(absent.status.code(), Some(1));
    assert!(stderr(&absent).contains("does not occur in the recorded session"));

    let both = rad()
        .arg("blame")
        .arg(&trace)
        .args(["--output", "a", "--error", "b"])
        .output()
        .expect("runs");
    assert_eq!(both.status.code(), Some(1));
    assert!(stderr(&both).contains("two symptoms"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_edited_program_states_the_symptom_and_a_fix_removes_it() {
    let dir = scratch("with");
    let source = std::fs::read_to_string(repo("projects/dogfood/stockroom/stockroom.rad"))
        .expect("program")
        .replace("\r\n", "\n");
    let session = std::fs::read(repo("projects/dogfood/stockroom/session.txt")).expect("session");
    let trace = record(
        &dir,
        &repo("projects/dogfood/stockroom/stockroom.rad"),
        &session,
    );

    // The recorded program only prints. An edited one turns the failed
    // audit into an error, and the error becomes the symptom.
    let asserting = dir.join("asserting.rad");
    let audit_print = "    if failures == 0 { print(\"audit clean\") }";
    assert!(source.contains(audit_print), "stockroom.rad changed shape");
    std::fs::write(
        &asserting,
        source.replace(
            audit_print,
            "    assert(failures == 0, \"books do not balance\")",
        ),
    )
    .expect("edited program");
    let report = blame_json(
        &trace,
        &[
            "--with",
            asserting.to_str().expect("utf-8 path"),
            "--error",
            "books do not balance",
            "--strategy",
            "ddmin",
            "--no-but-for",
        ],
    );
    let cause = cause_values(&report);
    assert_eq!(cause.len(), 4, "{cause:?}");
    assert_eq!(
        cause[1..],
        ["\"order o17 hinge 56\"", "\"ship o17\"", "\"cancel o17\""]
    );

    // The one-line fix, replayed against the same recorded day: the symptom
    // no longer occurs, so there is nothing to blame.
    let fixed = dir.join("fixed.rad");
    let bug = "    if o.state == \"cancelled\" { return nil }";
    assert!(source.contains(bug), "stockroom.rad changed shape");
    std::fs::write(
        &fixed,
        source.replace(bug, "    if o.state != \"open\" { return nil }"),
    )
    .expect("fixed program");
    let output = rad()
        .arg("blame")
        .arg(&trace)
        .args(["--with", fixed.to_str().expect("utf-8 path")])
        .args(["--output", "AUDIT FAILED"])
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("does not occur in the recorded session"),
        "{}",
        stderr(&output)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
