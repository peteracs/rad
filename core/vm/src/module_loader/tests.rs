#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::{Checker, CheckerOptions};
    use crate::compiler::Compiler;
    use crate::vm::VM;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn mk_temp_dir() -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rad_module_loader_{ts}_{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn compile_module_entry(entry: &Path) -> crate::compiler::CompileResult {
        let loaded = load_program_with_source_map(entry.to_str().unwrap()).unwrap();
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let analysis = crate::pipeline::analyze_program(
            &loaded.program,
            &loaded.aliases,
            CheckerOptions::default(),
        );
        assert!(analysis.errors().is_empty(), "{:?}", analysis.errors());
        crate::pipeline::compile_checked_program(
            &loaded.program,
            loaded.aliases,
            analysis.into_checked().expect("analysis is error-free"),
            crate::pipeline::CheckedCompileOptions::default(),
        )
        .expect("compile canonical module graph")
    }

    fn run_module_entry(entry: &Path) -> Vec<String> {
        let compiled = compile_module_entry(entry);
        let mut vm = VM::new();
        vm.load_compile_result(compiled);
        vm.run(0).expect("run canonical module graph");
        vm.print_buffer
    }

    #[test]
    fn aliased_module_forward_callback_is_compiled_as_a_view_kernel() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            r#"
pub component Active {}
pub component Position { x: int = 0 }
pub materialized view Movers { depends [Active, Position] }

pub fn run() -> nil {
    let target = spawn(Active {}, Position { x: 4 })
    visit_view(Movers, advance)
    assert(require(target, Position).x == 5, "aliased callback ran once")
}

fn advance(target: entity) -> nil {
    let x = read_field(target, Position, "x")
    write_field(target, Position, "x", x + 1)
}
"#,
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { owner.run() }\n",
        )
        .unwrap();

        let compiled = compile_module_entry(&dir.join("main.rad"));
        assert_eq!(
            compiled.view_kernels.len(),
            1,
            "a later callback declaration in an aliased module must emit RunViewKernel"
        );
        let mut vm = VM::new();
        vm.load_compile_result(compiled);
        vm.run(0).expect("run fused aliased callback");
    }

    #[test]
    fn lockfile_roundtrip_preserves_sha256_pins() {
        let sha = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let fingerprints = vec![ModuleFingerprint {
            path: "https://example.test/lib.rad".to_string(),
            bytes: 17,
            checksum: 12345,
            sha256_hex: Some(sha.to_string()),
        }];

        let lock = LockFile::generate(&fingerprints);
        let serialized = lock.serialize();
        assert!(serialized.starts_with("rad-lock 1\nchecksum "));
        assert!(serialized.contains(sha));

        let parsed = LockFile::parse(&serialized).expect("parse lockfile");
        assert_eq!(parsed.modules[0].sha256_hex.as_deref(), Some(sha));
        parsed
            .verify(&fingerprints)
            .expect("verify current modules");
    }

    #[test]
    fn expands_relative_use_and_marks_imports() {
        let dir = mk_temp_dir();
        let sub = dir.join("mods");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("lib.rad"), "fn libf() { return 1 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"mods/lib.rad\"\nfn main() -> nil { print(libf()) }\n",
        )
        .unwrap();

        let (program, _source, had_imports) =
            load_program_with_uses(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(had_imports);
        assert!(program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "libf")));
    }

    #[test]
    fn rejects_duplicate_top_level_symbols() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn same() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "pub fn same() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(err_vec[0]
                .message
                .contains("Duplicate top-level declaration"));
        } else {
            panic!("Expected error, got Ok");
        }
    }

    #[test]
    fn resources_and_structs_share_the_canonical_duplicate_namespace() {
        for declaration in [
            "pub resource Shared { value: int = 0 }\n",
            "pub struct Shared { value: int = 0 }\n",
        ] {
            let dir = mk_temp_dir();
            fs::write(dir.join("a.rad"), declaration).unwrap();
            fs::write(dir.join("b.rad"), declaration).unwrap();
            fs::write(
                dir.join("main.rad"),
                "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil {}\n",
            )
            .unwrap();

            let errors = load_program_with_uses(dir.join("main.rad").to_str().unwrap())
                .expect_err("duplicate public data declarations must be rejected");
            assert!(
                errors[0]
                    .message
                    .contains("Duplicate top-level declaration 'Shared'"),
                "unexpected error: {}",
                errors[0].message
            );
        }
    }

    #[test]
    fn duplicate_error_uses_local_file_span() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn same() { return 1 }\n").unwrap();
        fs::write(
            dir.join("b.rad"),
            "fn filler() { return 0 }\npub fn same() { return 2 }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(
                err_vec[0].message.contains("a.rad:1")
                    || err_vec[0].message.contains("already defined")
            );
            assert_eq!(err_vec[0].line, 2);
            assert_eq!(err_vec[0].col, 5);
            assert_eq!(
                err_vec[0].source.lines().next().unwrap_or_default(),
                "fn filler() { return 0 }"
            );
        } else {
            panic!("Expected error, got Ok");
        }
    }

    #[test]
    fn duplicate_in_same_file_reports_local_line() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("main.rad"),
            "fn same() { return 1 }\nfn same() { return 2 }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(err_vec[0]
                .message
                .contains("Duplicate top-level declaration"));
            assert!(
                err_vec[0].message.contains("main.rad:1")
                    || err_vec[0].message.contains("already defined")
            );
            assert_eq!(err_vec[0].line, 2);
            assert_eq!(err_vec[0].col, 1);
        } else {
            panic!("Expected error, got Ok");
        }
    }

    #[test]
    fn merged_source_has_structured_module_boundaries() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "fn a() { return 1 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nfn main() -> nil { print(a()) }\n",
        )
        .unwrap();
        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(result.had_imports);
        assert!(!result.merged_source.contains("// -- "));
        assert_eq!(result.source_layout.sections.len(), 2);
        assert!(result
            .source_layout
            .sections
            .iter()
            .any(|section| section.name.ends_with("a.rad")));
        result
            .source_layout
            .validate(&result.merged_source)
            .unwrap();
    }

    #[test]
    fn authenticated_source_bundle_reconstructs_module_order_and_private_scope() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("bare.rad"),
            "let hidden = 41\npub fn answer() -> int { return hidden + 1 }\n",
        )
        .unwrap();
        fs::write(
            dir.join("aliased.rad"),
            "let hidden = 8\npub fn answer() -> int { return hidden + 1 }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"bare.rad\"\nuse \"aliased.rad\" as other\nfn main() -> nil { print(answer()) print(other.answer()) }\n",
        )
        .unwrap();

        let recorded =
            load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        let mut direct_checker = Checker::new_with_options(CheckerOptions::default());
        direct_checker.set_aliases(recorded.aliases.clone());
        let direct_errors = direct_checker.check(&recorded.program);
        assert!(direct_errors.is_empty(), "{direct_errors:?}");
        let direct_compile = Compiler::new()
            .with_checker_output(direct_checker.output())
            .with_aliases(recorded.aliases.clone())
            .compile(&recorded.program)
            .expect("compile original module graph");
        let mut direct_vm = VM::new();
        direct_vm.load_compile_result(direct_compile);
        direct_vm.run(0).expect("run original module graph");
        assert_eq!(direct_vm.print_buffer, vec!["42", "9"]);

        let replayed = load_program_from_source_bundle(
            &recorded.merged_source,
            &recorded.source_layout,
            ParserOptions::default(),
        )
        .unwrap();
        assert!(replayed.errors.is_empty(), "{:?}", replayed.errors);

        let mut replay_checker = Checker::new_with_options(CheckerOptions::default());
        replay_checker.set_aliases(replayed.aliases.clone());
        let replay_errors = replay_checker.check(&replayed.program);
        assert!(replay_errors.is_empty(), "{replay_errors:?}");
        let compile_result = Compiler::new()
            .with_checker_output(replay_checker.output())
            .with_aliases(replayed.aliases)
            .compile(&replayed.program)
            .expect("compile reconstructed module graph");
        let mut vm = VM::new();
        vm.load_compile_result(compile_result);
        vm.run(0).expect("run reconstructed module graph");
        assert_eq!(vm.print_buffer, vec!["42", "9"]);
    }

    #[test]
    fn file_local_lines_resolve_via_source_map() {
        let dir = mk_temp_dir();
        fs::write(dir.join("empty.rad"), "").unwrap();
        fs::write(dir.join("lib.rad"), "fn helper() { return 1 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"empty.rad\"\nuse \"lib.rad\"\nfn main() -> nil { print(helper()) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        let helper_fn = result
            .program
            .declarations
            .iter()
            .find_map(|decl| match decl {
                Decl::Fn(f) if f.name == "helper" => Some(f),
                _ => None,
            })
            .expect("expected helper declaration");

        assert_eq!(
            helper_fn.span.line, 1,
            "helper should be at line 1 in its own file"
        );
        assert!(helper_fn.span.file.is_some(), "helper should have a FileId");

        let file = result
            .source_map
            .get_file(helper_fn.span.file.unwrap())
            .unwrap();
        let source_line = file.source.lines().next().unwrap_or_default();
        assert_eq!(source_line, "fn helper() { return 1 }");
    }

    #[test]
    fn merged_source_uses_single_blank_line_between_modules() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "fn a() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "fn b() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(a() + b()) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(!result.merged_source.contains("\n\n\n"));
        assert_eq!(result.source_layout.sections.len(), 3);
    }

    #[test]
    fn flat_namespace_rejects_cross_kind_duplicate_names() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("main.rad"),
            "fn same() { return 1 }\ntype same { One {} }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        assert!(err.is_err());
        let err_vec = err.unwrap_err();
        assert!(err_vec[0]
            .message
            .contains("Duplicate top-level declaration 'same'"));
    }

    #[test]
    fn cyclic_imports_do_not_reprocess_files() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "use \"b.rad\"\nfn fa() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "use \"a.rad\"\nfn fb() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let (program, _merged_source, had_imports) =
            load_program_with_uses(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(had_imports);

        let fn_names = program
            .declarations
            .iter()
            .filter_map(|d| match d {
                Decl::Fn(f) => Some(f.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        let fa_count = fn_names.iter().filter(|n| **n == "fa").count();
        let fb_count = fn_names.iter().filter(|n| **n == "fb").count();
        let main_count = fn_names.iter().filter(|n| **n == "main").count();
        assert_eq!(fa_count, 1);
        assert_eq!(fb_count, 1);
        assert_eq!(main_count, 1);
    }

    #[test]
    fn declaration_lines_resolve_via_source_map() {
        let dir = mk_temp_dir();
        fs::write(dir.join("empty.rad"), "").unwrap();
        fs::write(dir.join("a.rad"), "fn a() { return 1 }").unwrap();
        fs::write(dir.join("c.rad"), "\nfn c() { return 3 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"empty.rad\"\nuse \"a.rad\"\nuse \"c.rad\"\nfn main() -> nil { print(a() + c()) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        for decl in &result.program.declarations {
            if let Decl::Fn(f) = decl {
                let file_id = f.span.file.expect("fn should have FileId");
                let file = result.source_map.get_file(file_id).unwrap();
                let line_text = file
                    .source
                    .lines()
                    .nth((f.span.line as usize).saturating_sub(1))
                    .unwrap_or_default();
                assert!(
                    line_text.contains(&format!("fn {}", f.name)),
                    "expected source line {} to contain fn {}, got '{}' in file '{}'",
                    f.span.line,
                    f.name,
                    line_text,
                    file.path
                );
            }
        }
    }

    #[test]
    fn aliased_import_keeps_decls_separate() {
        let dir = mk_temp_dir();
        fs::write(dir.join("math.rad"), "pub fn square(x) { return x * x }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"math.rad\" as math\nfn main() -> nil { print(1) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        let has_square = result
            .program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "square"));
        assert!(!has_square, "'square' should NOT be in the flat namespace");

        assert!(result.aliases.contains_key("math"));
        let entries = &result.aliases["math"];
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            Decl::Fn(f) => assert_eq!(f.name, "square", "Alias decls keep original names"),
            other => panic!("Expected Decl::Fn, got {:?}", std::mem::discriminant(other)),
        }
    }

    #[test]
    fn duplicate_alias_names_rejected() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn af() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "pub fn bf() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\" as m\nuse \"b.rad\" as m\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(err_vec[0].message.contains("Duplicate module alias"));
        } else {
            panic!("Expected error, got Ok");
        }
    }

    #[test]
    fn duplicate_pub_let_across_modules_rejected() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub let LIMIT = 10\n").unwrap();
        fs::write(dir.join("b.rad"), "pub let LIMIT = 99\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(LIMIT) }\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(
                err_vec[0]
                    .message
                    .contains("Duplicate top-level declaration 'LIMIT'"),
                "expected duplicate pub let error, got: {}",
                err_vec[0].message
            );
        } else {
            panic!("Expected duplicate pub let error, got Ok");
        }
    }

    #[test]
    fn private_top_level_lets_keep_coexisting_across_modules() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("a.rad"),
            "let scratch = 1\npub fn af() -> int { return scratch }\n",
        )
        .unwrap();
        fs::write(
            dir.join("b.rad"),
            "let scratch = 2\npub fn bf() -> int { return scratch }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\"\nfn main() -> nil { print(af() + bf()) }\n",
        )
        .unwrap();

        // Historical behavior: private lets never participated in duplicate
        // detection; only exported (pub) lets do.
        let result = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        assert!(result.is_ok(), "private let coexistence must keep loading");
    }

    #[test]
    fn bare_use_still_works_alongside_aliased() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn af() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "pub fn bf() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\"\nuse \"b.rad\" as m\nfn main() -> nil { print(af()) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        let has_af = result
            .program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "af"));
        assert!(has_af, "Bare import 'af' should be in flat namespace");

        let bf_in_flat = result
            .program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "bf"));
        assert!(!bf_in_flat, "Aliased 'bf' should NOT be in flat namespace");

        assert!(result.aliases.contains_key("m"));
        let m_decls = &result.aliases["m"];
        assert_eq!(m_decls.len(), 1);
        match &m_decls[0] {
            Decl::Fn(f) => assert_eq!(f.name, "bf"),
            other => panic!("Expected Decl::Fn, got {:?}", std::mem::discriminant(other)),
        }
    }

    #[test]
    fn one_module_path_has_one_runtime_identity_across_import_bindings() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("state.rad"),
            "pub component State { value: int = 0 }\npub resource Counter { value: int = 0 }\npub fn publish() -> nil { spawn(State { value: 1 }) set_resource(Counter, Counter { value: res(Counter).value + 1 }) }\npub fn count() -> int { return len(entities(State)) }\npub fn resource_count() -> int { return res(Counter).value }\n",
        )
        .unwrap();
        fs::write(
            dir.join("bridge.rad"),
            "use \"state.rad\" as owner\npub fn count_through_alias() -> int { return owner.count() }\npub fn resource_through_alias() -> int { return owner.resource_count() }\n",
        )
        .unwrap();
        fs::write(
            dir.join("bare_and_alias.rad"),
            "use \"state.rad\"\nuse \"bridge.rad\"\nfn main() -> nil { publish() print(count_through_alias()) print(resource_through_alias()) }\n",
        )
        .unwrap();

        assert_eq!(
            run_module_entry(&dir.join("bare_and_alias.rad")),
            vec!["1", "1"]
        );

        fs::write(
            dir.join("two_aliases.rad"),
            "use \"state.rad\" as alpha\nuse \"state.rad\" as beta\nfn main() -> nil { alpha.publish() print(beta.count()) print(beta.resource_count()) }\n",
        )
        .unwrap();
        assert_eq!(
            run_module_entry(&dir.join("two_aliases.rad")),
            vec!["1", "1"]
        );
    }

    #[test]
    fn different_module_paths_with_identical_source_have_separate_runtime_identity() {
        let dir = mk_temp_dir();
        let module = "pub component State { value: int = 0 }\npub resource Counter { value: int = 0 }\npub fn publish() -> nil { spawn(State { value: 1 }) set_resource(Counter, Counter { value: res(Counter).value + 1 }) }\npub fn count() -> int { return len(entities(State)) }\npub fn resource_count() -> int { return res(Counter).value }\n";
        fs::write(dir.join("left.rad"), module).unwrap();
        fs::write(dir.join("right.rad"), module).unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"left.rad\" as left\nuse \"right.rad\" as right\nfn main() -> nil { left.publish() print(left.count()) print(left.resource_count()) print(right.count()) print(right.resource_count()) }\n",
        )
        .unwrap();

        assert_eq!(
            run_module_entry(&dir.join("main.rad")),
            vec!["1", "1", "0", "0"]
        );
    }

    #[test]
    fn normalized_relative_paths_share_one_module_identity() {
        let dir = mk_temp_dir();
        fs::create_dir(dir.join("mods")).unwrap();
        fs::write(
            dir.join("state.rad"),
            "pub component State { value: int = 0 }\npub resource Counter { value: int = 0 }\npub fn publish() -> nil { spawn(State { value: 1 }) set_resource(Counter, Counter { value: res(Counter).value + 1 }) }\npub fn count() -> int { return len(entities(State)) }\npub fn resource_count() -> int { return res(Counter).value }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"./state.rad\" as direct\nuse \"mods/../state.rad\" as normalized\nfn main() -> nil { direct.publish() print(normalized.count()) print(normalized.resource_count()) }\n",
        )
        .unwrap();

        let loaded = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert_eq!(
            loaded.aliases["direct"].module_identity(),
            loaded.aliases["normalized"].module_identity()
        );
        assert_eq!(
            loaded.aliases["direct"].canonical_namespace(),
            loaded.aliases["normalized"].canonical_namespace()
        );
        assert_eq!(run_module_entry(&dir.join("main.rad")), vec!["1", "1"]);
    }

    #[test]
    fn semantic_fingerprint_tracks_canonical_graph_not_alias_spelling() {
        let dir = mk_temp_dir();
        let module = "pub fn value() -> int { return 7 }\n";
        fs::write(dir.join("state.rad"), module).unwrap();
        fs::write(dir.join("other.rad"), module).unwrap();
        fs::write(
            dir.join("alpha.rad"),
            "use \"state.rad\" as alpha\nfn main() -> nil { print(alpha.value()) }\n",
        )
        .unwrap();
        fs::write(
            dir.join("renamed.rad"),
            "use \"state.rad\" as arbitrary_name\nfn main() -> nil { print(arbitrary_name.value()) }\n",
        )
        .unwrap();
        fs::write(
            dir.join("different_graph.rad"),
            "use \"other.rad\" as alpha\nfn main() -> nil { print(alpha.value()) }\n",
        )
        .unwrap();

        let alpha = load_program_with_source_map(dir.join("alpha.rad").to_str().unwrap()).unwrap();
        let renamed =
            load_program_with_source_map(dir.join("renamed.rad").to_str().unwrap()).unwrap();
        let different =
            load_program_with_source_map(dir.join("different_graph.rad").to_str().unwrap())
                .unwrap();
        let options = CheckerOptions::default();
        let alpha_fingerprint =
            crate::types::semantic_program_fingerprint(&alpha.program, &alpha.aliases, &options);
        let renamed_fingerprint = crate::types::semantic_program_fingerprint(
            &renamed.program,
            &renamed.aliases,
            &options,
        );
        let different_fingerprint = crate::types::semantic_program_fingerprint(
            &different.program,
            &different.aliases,
            &options,
        );

        assert_eq!(
            alpha_fingerprint, renamed_fingerprint,
            "alias spelling is not semantic identity"
        );
        assert_ne!(
            alpha_fingerprint, different_fingerprint,
            "changing the canonical module target changes the semantic product"
        );
    }

    #[test]
    fn coowner_authority_follows_canonical_module_identity() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("writer.rad"),
            concat!(
                "pub fn publish(target: entity) -> nil writes owned [Live] { ",
                "set(target, Live { value: 1 }) }\n",
                "pub fn bypass(target: entity) -> nil { ",
                "set(target, Live { value: 2 }) }\n",
            ),
        )
        .unwrap();
        fs::write(
            dir.join("owner.rad"),
            concat!(
                "use \"writer.rad\" as granted_writer\n",
                "pub component Live owned with [granted_writer] { value: int = 0 }\n",
            ),
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            concat!(
                "use \"owner.rad\"\n",
                "use \"writer.rad\" as differently_spelled\n",
                "fn main() -> nil { print(0) }\n",
            ),
        )
        .unwrap();

        let loaded = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert_eq!(
            loaded.aliases["granted_writer"].module_identity(),
            loaded.aliases["differently_spelled"].module_identity(),
        );
        let mut checker = Checker::new();
        checker.set_aliases(loaded.aliases);
        let errors = checker.check(&loaded.program);
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("publish cannot acquire")),
            "co-owner grant must follow module identity: {errors:?}"
        );
        assert!(
            errors.iter().any(|error| {
                error.message.contains(".bypass cannot set")
                    && error.message.contains("no owned-write grant")
            }),
            "the grant must not authorize a function without writes owned: {errors:?}"
        );
    }

    #[test]
    fn aliased_import_allows_same_name_in_different_modules() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn helper() { return 1 }\n").unwrap();
        fs::write(dir.join("b.rad"), "pub fn helper() { return 2 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"a.rad\" as a\nuse \"b.rad\" as b\nfn main() -> nil { print(0) }\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();

        let helper_in_flat = result
            .program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "helper"));
        assert!(!helper_in_flat, "'helper' should NOT be in flat namespace");

        assert!(result.aliases.contains_key("a"));
        assert!(result.aliases.contains_key("b"));

        let a_decls = &result.aliases["a"];
        assert_eq!(a_decls.len(), 1);
        match &a_decls[0] {
            Decl::Fn(f) => assert_eq!(f.name, "helper"),
            other => panic!(
                "Expected Decl::Fn in alias 'a', got {:?}",
                std::mem::discriminant(other)
            ),
        }

        let b_decls = &result.aliases["b"];
        assert_eq!(b_decls.len(), 1);
        match &b_decls[0] {
            Decl::Fn(f) => assert_eq!(f.name, "helper"),
            other => panic!(
                "Expected Decl::Fn in alias 'b', got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    #[test]
    fn alias_conflicts_with_existing_declaration() {
        let dir = mk_temp_dir();
        fs::write(dir.join("a.rad"), "pub fn af() { return 1 }\n").unwrap();
        fs::write(
            dir.join("main.rad"),
            "fn math() { return 0 }\nuse \"a.rad\" as math\n",
        )
        .unwrap();

        let err = load_program_with_uses(dir.join("main.rad").to_str().unwrap());
        if let Err(err_vec) = err {
            assert!(err_vec[0]
                .message
                .contains("conflicts with an existing declaration"));
        } else {
            panic!("Expected error, got Ok");
        }
    }

    /// Regression for Bug #1: `set(e, lex.Tok { ... })` and `has(e, lex.Tok)` must use the same
    /// mangled component type name (`__mod_lex__Tok`), not a mix of bare and qualified names.
    #[test]
    fn aliased_import_component_set_has_roundtrip() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("tok.rad"),
            "pub component Tok { kind: str = \"\" }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"tok.rad\" as lex\n\nfn main() -> nil {\n    let e = spawn()\n    set(e, lex.Tok { kind: \"hi\" })\n    print(has(e, lex.Tok))\n}\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(
            result.errors.is_empty(),
            "parse/load errors: {:?}",
            result.errors
        );

        let mut checker = Checker::new_with_options(CheckerOptions::default());
        checker.set_aliases(result.aliases.clone());
        let errors = checker.check(&result.program);
        assert!(
            errors.is_empty(),
            "typecheck errors: {:?}",
            errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
        );

        let compiler = Compiler::new()
            .with_checker_output(checker.output())
            .with_aliases(result.aliases);
        let compile_result = compiler.compile(&result.program).expect("compile");

        let mut vm = VM::new();
        vm.load_compile_result(compile_result);
        vm.run(0).expect("vm run");
        assert_eq!(
            vm.print_buffer,
            vec!["true"],
            "has(e, lex.Tok) must find the component set with lex.Tok literal (Bug #1 if false)"
        );
    }

    /// Bug #7: match arms must accept qualified paths like `lex.Tok.IntLit { n }`, not only bare
    /// `IntLit`. Parser + checker + codegen are exercised end-to-end.
    #[test]
    fn aliased_sum_type_match_qualified_variant_pattern() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("kinds.rad"),
            "pub type Tok {\n    IntLit { n: 0 }\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"kinds.rad\" as lex\n\nfn main() -> nil {\n    let k = lex.Tok::IntLit { n: 42 }\n    match k {\n        lex.Tok.IntLit { n } => { print(n) }\n    }\n}\n",
        )
        .unwrap();

        let result = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(
            result.errors.is_empty(),
            "parse/load errors: {:?}",
            result.errors
        );

        let mut checker = Checker::new_with_options(CheckerOptions::default());
        checker.set_aliases(result.aliases.clone());
        let errors = checker.check(&result.program);
        assert!(
            errors.is_empty(),
            "typecheck errors: {:?}",
            errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
        );

        let compiler = Compiler::new()
            .with_checker_output(checker.output())
            .with_aliases(result.aliases);
        let compile_result = compiler.compile(&result.program).expect("compile");

        let mut vm = VM::new();
        vm.load_compile_result(compile_result);
        vm.run(0).expect("vm run");
        assert_eq!(
            vm.print_buffer,
            vec!["42"],
            "qualified match pattern lex.Tok.IntLit should bind and run (Bug #7 if wrong)"
        );
    }

    #[test]
    fn imported_helper_io_is_enforced_transitively() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("helpers.rad"),
            "pub fn noisy() -> nil { print(\"from helper\") }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            concat!(
                "component Root {}\n",
                "use \"helpers.rad\" as helpers\n",
                "system Run(root: Root) { helpers.noisy() }\n",
            ),
        )
        .unwrap();

        let loaded = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        let mut checker = Checker::new_with_options(CheckerOptions::default());
        checker.set_aliases(loaded.aliases);
        let errors = checker.check(&loaded.program);
        assert!(
            errors.iter().any(|error| {
                error.message.contains("System 'Run'")
                    && error.message.contains("performs IO")
                    && error.hint.as_deref().is_some_and(|hint| {
                        hint.contains("Run ->")
                            && hint.contains(".noisy")
                            && hint.contains("io true")
                    })
            }),
            "expected imported helper IO authority violation and path, got {errors:?}"
        );
    }
}
