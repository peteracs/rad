#[cfg(test)]
mod preset_tests {
    use super::{get_preset, lint_source};

    #[test]
    fn enterprise_preset_exists() {
        let p = get_preset("enterprise").expect("enterprise preset");
        assert!(p.vm_flags.contains(&"--strict-types"));
        assert!(p.vm_flags.contains(&"--deny-warnings"));
    }

    #[test]
    fn strict_preset_exists() {
        let p = get_preset("strict").expect("strict preset");
        assert!(p.require_type_annotations);
    }

    #[test]
    fn teaching_preset_exists() {
        let p = get_preset("teaching").expect("teaching preset");
        assert!(p.suggest_type_annotations);
        assert!(!p.require_type_annotations);
    }

    #[test]
    fn lint_detects_long_file() {
        let source = (0..600)
            .map(|i| format!("let x{i} = {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (issues, _) = lint_source(&source, "enterprise");
        let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&"RAD-L001"));
    }

    #[test]
    fn lint_teaching_suggests_types() {
        let (issues, _) = lint_source("let x = 42\n", "teaching");
        let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&"RAD-L004"));
    }

    #[test]
    fn lint_detects_trailing_whitespace() {
        let (issues, _) = lint_source("let x = 1   \n", "strict");
        let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&"RAD-L007"));
    }

    fn l009_codes(source: &str) -> Vec<&'static str> {
        let (issues, _) = lint_source(source, "strict");
        issues
            .iter()
            .filter(|i| i.code == "RAD-L009")
            .map(|i| i.code)
            .collect()
    }

    #[test]
    fn l009_recognizes_schedule() {
        let src = "component Widget { n: 0 }\nsystem Alpha(w: Widget, t: mut Tally) { t.a = t.a + w.n }\nspawn(Widget { n: 5 })\nschedule [Alpha]\n";
        assert!(l009_codes(src).is_empty());
    }

    #[test]
    fn l009_recognizes_phase_and_schedule_of_phase() {
        let src = "system Feed(w: mut Widget) { w.n = 1 }\nphase Line { Feed }\nschedule [Line]\n";
        assert!(l009_codes(src).is_empty());
    }

    #[test]
    fn l009_recognizes_simulate() {
        let src = "system Age(h: mut Health) { h.hp = h.hp - 1 }\nlet before = fork()\nlet after = simulate(before, [system::Age], 5)\n";
        assert!(l009_codes(src).is_empty());
    }

    #[test]
    fn l009_recognizes_direct_call() {
        let src = "system Alpha(w: mut Widget) { w.n = 1 }\nAlpha()\n";
        assert!(l009_codes(src).is_empty());
    }

    #[test]
    fn l009_fires_on_genuinely_unused_system() {
        let src = "system Alpha(w: mut Widget) { w.n = 1 }\nprint(\"no scheduling here\")\n";
        assert_eq!(l009_codes(src).len(), 1);
    }

    #[test]
    fn l009_not_silenced_by_run_assignment() {
        let src =
            "system Alpha(w: mut Widget) { w.n = 1 }\nlet mut run = 0\nrun = 5\nrun [Alpha]\n";
        assert_eq!(l009_codes(src).len(), 1);
    }

    #[test]
    fn l009_message_suggests_real_syntax() {
        let (issues, _) = lint_source("system Alpha(w: mut Widget) { w.n = 1 }\n", "strict");
        let msg = &issues
            .iter()
            .find(|i| i.code == "RAD-L009")
            .expect("RAD-L009 fires")
            .message;
        assert!(msg.contains("schedule [A, B]"), "got: {}", msg);
        assert!(msg.contains("simulate("), "got: {}", msg);
        assert!(!msg.contains("run SystemName"), "got: {}", msg);
    }

    #[test]
    fn l009_ignores_commented_out_schedule() {
        let src = "system Alpha(w: mut Widget) { w.n = 1 }\n// schedule [Alpha]\n";
        assert_eq!(l009_codes(src).len(), 1);
    }
}

#[cfg(test)]
mod ast_lint_tests {
    use super::{get_preset, lint_ast};

    #[test]
    fn imported_decls_are_not_linted_under_the_entry_file() {
        let source = r#"
fn local_build(xs: list) -> list {
    let mut acc = []
    for x in xs { acc = push(acc, x) }
    return acc
}

fn imported_build(xs: list) -> list {
    let mut basket = []
    for x in xs { basket = push(basket, x) }
    return basket
}
"#;
        let mut lexer = crate::lexer::Lexer::new(source);
        let (tokens, lex_errors) = lexer.tokenize();
        assert!(lex_errors.is_empty(), "lex errors: {:?}", lex_errors);
        let mut parser = crate::parser::Parser::new(tokens);
        let mut program = parser.parse();
        assert!(parser.errors().is_empty(), "parse errors: {:?}", parser.errors());
        for decl in &mut program.declarations {
            if let crate::ast::Decl::Fn(function) = decl {
                let file = if function.name == "imported_build" { 1 } else { 0 };
                function.span.file = Some(crate::ast::FileId(file));
            }
        }
        let mut checker = crate::checker::Checker::new();
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "type errors: {:?}", errors);
        let checked = checker.output();
        let preset = get_preset("strict").expect("preset");
        let issues = lint_ast(
            &program,
            &checked,
            &preset,
            "entry.rad",
            &std::collections::HashMap::new(),
        );
        assert!(issues.iter().any(|issue| issue.message.contains("'acc'")));
        assert!(!issues.iter().any(|issue| issue.message.contains("'basket'")));
    }
}
