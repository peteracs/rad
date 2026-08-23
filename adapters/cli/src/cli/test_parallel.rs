#[derive(Debug)]
struct TestProcessResult {
    index: usize,
    stdout: String,
    stderr: String,
    passed: usize,
    failed: usize,
    files_errored: usize,
    files_without_tests: usize,
}

fn execute_test_files_parallel(test_files: &[std::path::PathBuf]) {
    let worker_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(test_files.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let (sender, receiver) = std::sync::mpsc::channel();
    let executable = std::env::current_exe().unwrap_or_else(|error| {
        eprintln!("Error: cannot locate the RAD test executable: {error}");
        process::exit(1);
    });

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let next = &next;
            let executable = &executable;
            scope.spawn(move || loop {
                let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(path) = test_files.get(index) else {
                    break;
                };
                let output = std::process::Command::new(executable)
                    .arg("test")
                    .arg(path)
                    .output();
                let result = match output {
                    Ok(output) => test_process_result(index, output),
                    Err(error) => TestProcessResult {
                        index,
                        stdout: String::new(),
                        stderr: format!(
                            "Error: cannot run test file '{}': {error}",
                            path.display()
                        ),
                        passed: 0,
                        failed: 0,
                        files_errored: 1,
                        files_without_tests: 0,
                    },
                };
                if sender.send(result).is_err() {
                    break;
                }
            });
        }
        drop(sender);
        let mut ordered = (0..test_files.len()).map(|_| None).collect::<Vec<_>>();
        for result in receiver {
            let index = result.index;
            ordered[index] = Some(result);
        }

        let mut passed = 0;
        let mut failed = 0;
        let mut files_errored = 0;
        let mut files_without_tests = 0;
        for result in ordered.into_iter().flatten() {
            if !result.stdout.is_empty() {
                println!("{}", result.stdout.trim_end());
            }
            if !result.stderr.is_empty() {
                eprintln!("{}", result.stderr.trim_end());
            }
            passed += result.passed;
            failed += result.failed;
            files_errored += result.files_errored;
            files_without_tests += result.files_without_tests;
        }

        let mut summary = format!(
            "\nResults: {passed} passed, {failed} failed, {} total",
            passed + failed
        );
        if files_without_tests > 0 {
            summary.push_str(&format!(
                " ({files_without_tests} file(s) with no tests)"
            ));
        }
        if files_errored > 0 {
            summary.push_str(&format!(" ({files_errored} file(s) failed to run)"));
        }
        println!("{summary}");
        if failed > 0 || files_errored > 0 {
            process::exit(1);
        }
    });
}

fn test_process_result(index: usize, output: std::process::Output) -> TestProcessResult {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let mut passed = 0;
    let mut failed = 0;
    let mut files_errored = 0;
    let mut files_without_tests = 0;
    let mut retained = Vec::new();
    let mut found_summary = false;
    for line in stdout.lines() {
        if let Some(summary) = line.strip_prefix("Results: ") {
            if let Some(counts) = parse_test_summary(summary) {
                found_summary = true;
                (passed, failed, files_errored, files_without_tests) = counts;
            }
        } else if !line.is_empty() {
            retained.push(line);
        }
    }
    if !found_summary || (!output.status.success() && failed == 0 && files_errored == 0) {
        files_errored = 1;
    }
    TestProcessResult {
        index,
        stdout: retained.join("\n"),
        stderr,
        passed,
        failed,
        files_errored,
        files_without_tests,
    }
}

fn parse_test_summary(summary: &str) -> Option<(usize, usize, usize, usize)> {
    let fields = summary.split_whitespace().collect::<Vec<_>>();
    let passed = fields.first()?.parse().ok()?;
    let failed = fields.get(2)?.parse().ok()?;
    Some((
        passed,
        failed,
        summary_count(summary, "file(s) failed to run"),
        summary_count(summary, "file(s) with no tests"),
    ))
}

fn summary_count(summary: &str, label: &str) -> usize {
    summary
        .match_indices(label)
        .find_map(|(index, _)| {
            summary[..index]
                .rsplit_once('(')
                .and_then(|(_, count)| count.trim().parse().ok())
        })
        .unwrap_or(0)
}
