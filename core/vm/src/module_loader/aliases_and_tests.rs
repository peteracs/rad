

fn alias_pass(ctx: &mut LoadContext) -> Result<(), ()> {
    let mut alias_targets = HashMap::new();
    let mut paths: Vec<_> = ctx.parsed_files.keys().cloned().collect();
    paths.sort();
    let mut has_error = false;

    for path in paths {
        let parsed = match ctx.parsed_files.get(&path) {
            Some(p) => p.clone(),
            None => continue,
        };

        for decl in &parsed.decls {
            if let Decl::Use(u) = decl {
                if let Some(alias) = &u.alias {
                    let child = match resolve_module_path(&parsed.path, &u.path, ctx) {
                        Ok(p) => p,
                        Err(msg) => {
                            ctx.errors.push(ModuleLoadError {
                                filepath: path.to_string_lossy().to_string(),
                                source: parsed.source.clone(),
                                message: msg,
                                line: u.span.line,
                                col: u.span.col,
                            });
                            has_error = true;
                            continue;
                        }
                    };

                    if let Some(existing_target) = alias_targets.get(alias) {
                        if existing_target != &child {
                            ctx.errors.push(ModuleLoadError {
                                filepath: path.to_string_lossy().to_string(),
                                source: parsed.source.clone(),
                                message: format!(
                                    "Duplicate module alias '{}' points to different files",
                                    alias
                                ),
                                line: u.span.line,
                                col: u.span.col,
                            });
                            has_error = true;
                        }
                    } else {
                        if ctx.symbols.contains_key(alias) {
                            ctx.errors.push(ModuleLoadError {
                                filepath: path.to_string_lossy().to_string(),
                                source: parsed.source.clone(),
                                message: format!(
                                    "Module alias '{}' conflicts with an existing declaration",
                                    alias
                                ),
                                line: u.span.line,
                                col: u.span.col,
                            });
                            has_error = true;
                        }
                        alias_targets.insert(alias.clone(), child.clone());

                        let child_parsed = match ctx.parsed_files.get(&child) {
                            Some(p) => p,
                            None => continue,
                        };
                        let mut alias_decls = Vec::new();
                        for d in &child_parsed.decls {
                            if !matches!(d, Decl::Use(_)) {
                                alias_decls.push(d.clone());
                            }
                        }
                        ctx.aliases.insert(alias.clone(), alias_decls);
                    }
                }
            }
        }
    }
    if has_error {
        Err(())
    } else {
        Ok(())
    }
}

fn decl_is_pub(decl: &Decl) -> bool {
    match decl {
        Decl::Component(c) => c.is_pub,
        Decl::Struct(s) => s.is_pub,
        Decl::Intent(i) => i.is_pub,
        Decl::Law(l) => l.is_pub,
        Decl::Resolver(r) => r.is_pub,
        Decl::Constraint(c) => c.is_pub,
        Decl::Entity(e) => e.is_pub,
        Decl::State(s) => s.is_pub,
        Decl::System(s) => s.is_pub,
        Decl::Event(e) => e.is_pub,
        Decl::Phase(p) => p.is_pub,
        Decl::Fn(f) => f.is_pub,
        Decl::Type(t) => t.is_pub,
        Decl::TypeAlias(a) => a.is_pub,
        Decl::Stmt(Stmt::Let(l)) => l.is_pub,
        _ => false,
    }
}

fn decl_symbol(decl: &Decl) -> Option<(String, u32, u32)> {
    match decl {
        Decl::Component(c) => Some((c.name.clone(), c.span.line, c.span.col)),
        Decl::Intent(i) => Some((i.name.clone(), i.span.line, i.span.col)),
        Decl::Law(l) => Some((l.name.clone(), l.span.line, l.span.col)),
        Decl::Resolver(r) => Some((r.name.clone(), r.span.line, r.span.col)),
        Decl::Constraint(c) => Some((c.name.clone(), c.span.line, c.span.col)),
        Decl::Entity(e) => Some((e.name.clone(), e.span.line, e.span.col)),
        Decl::State(s) => Some((s.name.clone(), s.span.line, s.span.col)),
        Decl::System(s) => Some((s.name.clone(), s.span.line, s.span.col)),
        Decl::Event(e) => Some((e.name.clone(), e.span.line, e.span.col)),
        Decl::Phase(p) => Some((p.name.clone(), p.span.line, p.span.col)),
        Decl::Fn(f) => Some((f.name.clone(), f.span.line, f.span.col)),
        Decl::Type(t) => Some((t.name.clone(), t.span.line, t.span.col)),
        Decl::TypeAlias(a) => Some((a.name.clone(), a.span.line, a.span.col)),
        // Exported constants join duplicate detection; private top-level
        // lets keep their historical file-local coexistence.
        Decl::Stmt(Stmt::Let(l)) if l.is_pub => l
            .names
            .first()
            .map(|n| (n.clone(), l.span.line, l.span.col)),
        _ => None,
    }
}

fn normalize_file_source_for_merge(source: &str) -> String {
    if source.ends_with('\n') {
        source.to_string()
    } else {
        format!("{source}\n")
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(p) = fs::canonicalize(path) {
        return p;
    }
    path.to_path_buf()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

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
}
