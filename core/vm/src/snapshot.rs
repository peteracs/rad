use std::fs;
use std::path::{Path, PathBuf};

use crate::checker::{Checker, CheckerOptions};
use crate::compiler::Compiler;
use crate::module_loader::load_program_with_source_map_and_options;
use crate::parser::ParserOptions;
use crate::vm::VM;

pub fn execute_snapshot(directory: Option<String>, update: bool, create: bool) {
    let dir = directory.unwrap_or_else(|| {
        if Path::new("tests").is_dir() {
            "tests".to_string()
        } else if Path::new("examples").is_dir() {
            "examples".to_string()
        } else {
            println!("No test or example directory found. Specify a directory.");
            std::process::exit(1);
        }
    });

    let path = Path::new(&dir);
    if !path.is_dir() {
        println!("Error: directory '{}' not found", dir);
        std::process::exit(1);
    }

    let mut rad_files = Vec::new();
    find_rad_files(path, &mut rad_files);
    rad_files.sort();

    if rad_files.is_empty() {
        println!("No .rad files found in '{}'", dir);
        return;
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut created = 0;
    let mut updated = 0;

    for filepath in rad_files {
        let snap_path = filepath.with_extension("snap");

        if create && !snap_path.exists() {
            let (stdout, stderr, exit_code) = run_file_in_memory(&filepath.to_string_lossy());
            write_snapshot(&snap_path, &filepath, &stdout, &stderr, exit_code);
            println!("  CREATE {}", filepath.display());
            created += 1;
            continue;
        }

        if !snap_path.exists() {
            continue;
        }

        let (stdout, stderr, exit_code) = run_file_in_memory(&filepath.to_string_lossy());

        if update {
            write_snapshot(&snap_path, &filepath, &stdout, &stderr, exit_code);
            println!("  UPDATE {}", filepath.display());
            updated += 1;
            continue;
        }

        let (expected_stdout, expected_stderr, expected_exit_code) = read_snapshot(&snap_path);

        let stdout_clean = clean_output(&stdout);
        let stderr_clean = clean_output(&stderr);
        let expected_stdout_clean = clean_output(&expected_stdout);
        let expected_stderr_clean = clean_output(&expected_stderr);

        if stdout_clean == expected_stdout_clean
            && stderr_clean == expected_stderr_clean
            && exit_code == expected_exit_code
        {
            println!("  PASS  {}", filepath.display());
            passed += 1;
        } else {
            println!("  FAIL  {}", filepath.display());
            if stdout_clean != expected_stdout_clean {
                println!("        --- expected stdout");
                println!("        +++ actual stdout");
                // In a real implementation, we'd use a diff library here.
                // For simplicity, we'll just print the difference if it's small,
                // or just indicate they differ.
                println!("        (stdout differs)");
            }
            if stderr_clean != expected_stderr_clean {
                println!("        --- expected stderr");
                println!("        +++ actual stderr");
                println!("        (stderr differs)");
            }
            if exit_code != expected_exit_code {
                println!("        --- expected exit_code: {}", expected_exit_code);
                println!("        +++ actual exit_code: {}", exit_code);
            }
            failed += 1;
        }
    }

    println!("\nSnapshot Summary:");
    if passed > 0 {
        println!("  {} passed", passed);
    }
    if failed > 0 {
        println!("  {} failed", failed);
    }
    if created > 0 {
        println!("  {} created", created);
    }
    if updated > 0 {
        println!("  {} updated", updated);
    }

    if failed > 0 {
        std::process::exit(1);
    }
}

fn find_rad_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                find_rad_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rad") {
                files.push(path);
            }
        }
    }
}

fn write_snapshot(
    snap_path: &Path,
    source_path: &Path,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
) {
    let mut content = format!(
        "---\nsource: {}\nexit_code: {}\n---\n{}",
        source_path.display(),
        exit_code,
        stdout
    );
    if !stderr.is_empty() {
        content.push_str("\n---stderr---\n");
        content.push_str(stderr);
    }
    fs::write(snap_path, content).unwrap();
}

fn read_snapshot(snap_path: &Path) -> (String, String, i32) {
    let content = fs::read_to_string(snap_path).unwrap_or_default();
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;

    let parts: Vec<&str> = content.split("\n---\n").collect();
    if parts.len() >= 2 {
        let header = parts[0];
        for line in header.lines() {
            if let Some(rest) = line.strip_prefix("exit_code: ") {
                if let Ok(code) = rest.parse::<i32>() {
                    exit_code = code;
                }
            }
        }

        let body = parts[1..].join("\n---\n");
        if let Some(idx) = body.find("\n---stderr---\n") {
            stdout = body[..idx].to_string();
            stderr = body[idx + 14..].to_string();
        } else {
            stdout = body;
        }
    }

    (stdout, stderr, exit_code)
}

fn clean_output(output: &str) -> String {
    let mut lines = Vec::new();
    for line in output.lines() {
        if line.trim().starts_with("--- ran in") {
            continue;
        }
        let normalized = normalize_repo_path(line);
        let mut stripped = normalized.trim_end();
        if stripped.trim_start().starts_with("Error:")
            || stripped.trim_start().starts_with("Warning:")
            || stripped.trim_start().starts_with("-->")
            || stripped.trim_start().starts_with("|")
            || stripped.trim_start().starts_with(">>")
            || stripped.trim_start().starts_with("hint:")
            || stripped.trim_start().starts_with("^")
        {
            stripped = stripped.trim_start();
        }
        lines.push(stripped.to_string());
    }
    lines.join("\n").trim_end().to_string()
}

fn normalize_repo_path(line: &str) -> String {
    let mut normalized = line.to_string();
    let mut roots = Vec::new();

    if let Ok(current) = std::env::current_dir() {
        roots.push(current.to_string_lossy().into_owned());
        if let Ok(canonical) = current.canonicalize() {
            roots.push(canonical.to_string_lossy().into_owned());
        }
    }

    roots.sort_by_key(|root| std::cmp::Reverse(root.len()));
    roots.dedup();
    for root in roots {
        normalized = normalized.replace(&root, "<repo>");
        normalized = normalized.replace(&root.replace('\\', "/"), "<repo>");
    }

    if normalized.contains("<repo>") {
        normalized = normalized.replace('\\', "/");
    }
    normalized
}

pub fn run_file_in_memory(filepath: &str) -> (String, String, i32) {
    let parser_options = ParserOptions {
        compat_v0_5_dx: false,
    };
    let (program, _source, _had_imports, _source_map, _module_fingerprints, aliases, parse_errors) =
        match load_program_with_source_map_and_options(filepath, parser_options) {
            Ok(r) => (
                r.program,
                r.merged_source,
                r.had_imports,
                r.source_map,
                r.module_fingerprints,
                r.aliases,
                r.errors,
            ),
            Err(errors) => {
                let mut stderr = String::new();
                for e in errors {
                    stderr.push_str(&format!("Error: {}\n", e.message));
                }
                return (String::new(), stderr, 1);
            }
        };

    let mut stderr = String::new();
    let mut has_errors = false;
    for e in &parse_errors {
        stderr.push_str(&format!("Error: {}\n", e.message));
        has_errors = true;
    }

    let mut checker = Checker::new_with_options(CheckerOptions {
        compat_v0_5_dx: false,
        warn_compat: true,
        strict_types: false,
        features: vec![],
    });
    checker.set_aliases(aliases.clone());
    let errors = checker.check(&program);
    let checker_output = checker.output();

    if !errors.is_empty() {
        for err in &errors {
            stderr.push_str(&format!("Error: {}\n", err.message));
        }
        has_errors = true;
    }

    if has_errors {
        return (String::new(), stderr, 1);
    }

    let compiler = Compiler::new()
        .with_checker_output(checker_output)
        .with_aliases(aliases);
    let compile_result = match compiler.compile(&program) {
        Ok(c) => c,
        Err(e) => {
            stderr.push_str(&format!("Error: {}\n", e.message));
            return (String::new(), stderr, 1);
        }
    };

    let mut vm = VM::new();
    vm.suppress_output = true;
    vm.load_compile_result(compile_result);

    let result = vm.run(0);

    let stdout = vm.print_buffer.join("\n");
    let vm_stderr = vm.eprint_buffer.join("\n");
    if !vm_stderr.is_empty() {
        stderr.push_str(&vm_stderr);
        stderr.push('\n');
    }

    match result {
        Ok(_) => (stdout, stderr, 0),
        Err(e) => {
            stderr.push_str(&format!("Error: {}\n", e));
            (stdout, stderr, 1)
        }
    }
}
