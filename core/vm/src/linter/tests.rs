

impl crate::visitor::AstVisitor for SystemAccessVisitor<'_> {
    fn visit_stmt(&mut self, stmt: &crate::ast::Stmt) {
        if let crate::ast::Stmt::Update(u) = stmt {
            // `update(entity, Comp) { … }` / `update(Resource) { … }` — both
            // are writes; classification (component vs resource) happens in
            // check_access. Field values are still walked below.
            self.check_access(&u.comp_name, &u.span, true, "update");
        }
        crate::visitor::walk_stmt(self, stmt);
    }

    fn visit_call_expr(
        &mut self,
        callee: &crate::ast::Expr,
        args: &[crate::ast::Expr],
        _span: &crate::ast::Span,
    ) {
        if let crate::ast::Expr::Ident(name, _) = callee {
            let arg_refs: Vec<&crate::ast::Expr> = args.iter().collect();
            self.check_builtin_call(name, &arg_refs);
        }
        crate::visitor::walk_call_expr(self, callee, args);
    }

    fn visit_expr(&mut self, expr: &crate::ast::Expr) {
        use crate::ast::{ComponentEntry, Expr};
        match expr {
            // `entity { Comp { … } }` literals spawn: every component entry
            // is a write to that component's storage.
            Expr::EntityLiteral(_, entries, _) => {
                for entry in entries {
                    match entry {
                        ComponentEntry::Init(init) => {
                            self.check_access(&init.comp_name, &init.span, true, "entity literal");
                        }
                        ComponentEntry::Expr(Expr::ComponentExpr(comp, _, _, span)) => {
                            self.check_access(comp, span, true, "entity literal");
                        }
                        ComponentEntry::Expr(_) => {}
                    }
                }
            }
            // `query { A, mut B }` reads A and writes B back per iteration.
            Expr::QueryExpr(query, span) => {
                for (comp, is_mut) in &query.components {
                    self.check_access(comp, span, *is_mut, "query { }");
                }
            }
            // `e |> get(Comp)` calls get(e, Comp): rebuild the effective
            // argument list, then recurse manually so the nested call node
            // is not matched a second time with shifted positions.
            Expr::Pipe(lhs, rhs, _) => {
                if let Expr::Call(callee, args, _) = rhs.as_ref() {
                    if let Expr::Ident(name, _) = callee.as_ref() {
                        let mut effective: Vec<&Expr> = vec![lhs.as_ref()];
                        effective.extend(args.iter());
                        self.check_builtin_call(name, &effective);
                    }
                    self.visit_expr(lhs);
                    for arg in args {
                        self.visit_expr(arg);
                    }
                    return;
                }
            }
            _ => {}
        }
        crate::visitor::walk_expr(self, expr);
    }
}

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
        // `schedule [...]` is THE documented way to run systems; it must
        // count as running them (A1 BUG 06 symptom 1).
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
        // forecast-only systems are run via simulate() on a fork (A4 BUG 07)
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
        // `run = 5` used to satisfy the old textual `run ` check even
        // though there is no `run` statement in the language (A1 BUG 06
        // symptom 2)
        let src =
            "system Alpha(w: mut Widget) { w.n = 1 }\nlet mut run = 0\nrun = 5\nrun [Alpha]\n";
        assert_eq!(l009_codes(src).len(), 1);
    }

    #[test]
    fn l009_message_suggests_real_syntax() {
        // the old hint recommended `run SystemName`, which does not parse;
        // mirror the checker's hint instead (A1 BUG 06 symptom 3)
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
mod system_access_tests {
    use super::{get_preset, lint_ast, LintIssue};

    /// Parse + type-check `source`, then run the AST lints with `preset_name`.
    /// The fixture must be checker-clean so the lint results are meaningful.
    fn lint_with_preset(source: &str, preset_name: &str) -> Vec<LintIssue> {
        let mut lexer = crate::lexer::Lexer::new(source);
        let (tokens, lex_errors) = lexer.tokenize();
        assert!(lex_errors.is_empty(), "lex errors: {:?}", lex_errors);
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        let mut checker = crate::checker::Checker::new();
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "type errors: {:?}", errors);
        let preset = get_preset(preset_name).expect("preset");
        lint_ast(
            &program,
            &checker,
            &preset,
            "test.rad",
            &std::collections::HashMap::new(),
        )
    }

    fn strict_access_issues(source: &str) -> Vec<LintIssue> {
        lint_with_preset(source, "strict")
            .into_iter()
            .filter(|i| i.code == "RAD-L015" || i.code == "RAD-L016")
            .collect()
    }

    #[test]
    fn out_of_signature_read_flagged() {
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Health { hp: 100 }

system Physics(pos: mut Position) {
    for e in entities(Health) {
        let h = require(e, Health)
        if h.hp > 0 { pos.x = 0.0 }
    }
}
"#,
        );
        assert_eq!(issues.len(), 2, "entities() and require() should both flag");
        assert!(issues.iter().all(|i| i.code == "RAD-L016"));
        assert!(issues.iter().any(|i| i
            .message
            .contains("reads component 'Health' via `entities()`")));
        assert!(issues.iter().any(|i| i
            .message
            .contains("reads component 'Health' via `require()`")));
    }

    #[test]
    fn out_of_signature_write_flagged_with_write_wording() {
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Health { hp: 100 }

system Damage(pos: mut Position) {
    for e in entities(Position) {
        set(e, Health { hp: 10 })
    }
}
"#,
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "RAD-L015");
        assert!(issues[0]
            .message
            .contains("System 'Damage' writes component 'Health' via `set()`"));
        assert!(issues[0].message.contains("cannot see this write conflict"));
    }

    #[test]
    fn update_sugar_flagged() {
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Health { hp: 100 }

system Drain(pos: mut Position) {
    for e in entities(Position) {
        update(e, Health) { hp = 0 }
    }
}
"#,
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "RAD-L015");
        assert!(issues[0]
            .message
            .contains("writes component 'Health' via `update`"));
    }

    #[test]
    fn resource_access_flagged() {
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
resource Clock { tick: 0 }

system Tick(pos: mut Position) {
    let c = get_resource(Clock) |> unwrap
    if c.tick > 0 { pos.x = 0.0 }
    update(Clock) { tick = 1 }
    set_resource(Clock, Clock { tick: 2 })
}
"#,
        );
        let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
        assert_eq!(codes, vec!["RAD-L016", "RAD-L015", "RAD-L015"]);
        assert!(issues[0]
            .message
            .contains("reads resource 'Clock' via `get_resource()`"));
        assert!(issues[1]
            .message
            .contains("writes resource 'Clock' via `update`"));
        assert!(issues[2]
            .message
            .contains("writes resource 'Clock' via `set_resource()`"));
    }

    #[test]
    fn spawn_and_entity_literal_flagged() {
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Loot { gold: 0 }

system DropLoot(pos: Position) {
    let a = spawn("chest", Loot { gold: 5 })
    let b = entity { Loot { gold: 7 } }
    if a == b { print("same") }
}
"#,
        );
        assert_eq!(issues.len(), 2);
        assert!(issues.iter().all(|i| i.code == "RAD-L015"));
        assert!(issues
            .iter()
            .any(|i| i.message.contains("writes component 'Loot' via `spawn()`")));
        assert!(issues.iter().any(|i| i
            .message
            .contains("writes component 'Loot' via `entity literal`")));
    }

    #[test]
    fn query_expr_flagged() {
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Unit { id: 0 }

system Scan(pos: mut Position) {
    for u in query { Unit } {
        pos.x = pos.x + 1.0
    }
}
"#,
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "RAD-L016");
        assert!(issues[0]
            .message
            .contains("reads component 'Unit' via `query { }`"));
    }

    #[test]
    fn in_signature_access_clean() {
        // Presence in the signature is what v1 checks — reads and writes of
        // declared types are fine, whichever builtin performs them.
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
resource Clock { tick: 0 }

system Move(pos: mut Position, clock: Clock) {
    for e in entities(Position) {
        set(e, Position { x: 1.0 })
    }
    let c = get_resource(Clock) |> unwrap
    if c.tick > 0 { pos.x = 0.0 }
}
"#,
        );
        assert!(issues.is_empty(), "unexpected: {:?}", issues[0].message);
    }

    #[test]
    fn event_payload_mention_clean() {
        // A component name in an emitted payload is a string mention, not an
        // ECS access.
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Health { hp: 100 }
event Telemetry { comp: str }

on Telemetry(t) { print(t.comp) }

system Report(pos: Position) {
    emit Telemetry { comp: Health }
}
"#,
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn match_pattern_mention_clean() {
        // `has Comp` patterns and variant patterns are not treated as
        // accesses in v1 (a name in a pattern is a mention, not a call).
        // The closure keeps the match lexically inside the system body.
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Shield { amount: 0 }

system Guard(pos: mut Position) {
    let check = fn(subject) {
        match subject {
            has Shield => { return 0.0 }
            _ => { return 1.0 }
        }
    }
    for e in entities(Position) {
        pos.x = check(e)
    }
}
"#,
        );
        assert!(issues.is_empty(), "unexpected: {:?}", issues[0].message);
    }

    #[test]
    fn peek_on_fork_clean() {
        // peek()/peek_resource() read a world FORK, not the live world the
        // scheduler coordinates.
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Health { hp: 100 }

system Predict(pos: mut Position) {
    let f = fork()
    for e in entities(Position) {
        let h = peek(f, e, Health) |> unwrap
        if h.hp > 0 { pos.x = 0.0 }
    }
}
"#,
        );
        assert!(issues.is_empty(), "unexpected: {:?}", issues[0].message);
    }

    #[test]
    fn event_handler_not_linted() {
        // `on Event` handlers are not systems; they have no signature for
        // the scheduler to consult and are out of scope.
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Health { hp: 100 }
event Damaged { amount: int }

system Move(pos: mut Position) {
    pos.x = pos.x + 1.0
}

on Damaged(d) {
    for e in entities(Health) {
        set(e, Health { hp: 100 - d.amount })
    }
}
"#,
        );
        assert!(issues.is_empty(), "unexpected: {:?}", issues[0].message);
    }

    #[test]
    fn user_function_shadowing_builtin_clean() {
        // User functions shadow builtins; calling one with a component name
        // as an argument is not an ECS access.
        let issues = strict_access_issues(
            r#"
component Position { x: 0.0 }
component Health { hp: 100 }

fn get(a: int, b: str) -> str { return b }

system Move(pos: mut Position) {
    let name = get(1, Health)
    print(name)
}
"#,
        );
        assert!(issues.is_empty(), "unexpected: {:?}", issues[0].message);
    }

    #[test]
    fn imported_decls_are_not_linted_under_the_entry_file() {
        // The module loader merges imported declarations into the entry
        // program, tagged with their own FileId. Their lint findings must
        // not appear under the entry file's heading (A5 BUG 04): here the
        // `basket` loop "belongs" to an import and only the entry file's
        // `acc` loop may be reported.
        let source = r#"
fn local_build(xs: list) -> list {
    let mut acc = []
    for x in xs {
        acc = push(acc, x)
    }
    return acc
}

fn imported_build(xs: list) -> list {
    let mut basket = []
    for x in xs {
        basket = push(basket, x)
    }
    return basket
}
"#;
        let mut lexer = crate::lexer::Lexer::new(source);
        let (tokens, lex_errors) = lexer.tokenize();
        assert!(lex_errors.is_empty(), "lex errors: {:?}", lex_errors);
        let mut parser = crate::parser::Parser::new(tokens);
        let mut program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        for decl in &mut program.declarations {
            if let crate::ast::Decl::Fn(f) = decl {
                let file = if f.name == "imported_build" { 1 } else { 0 };
                f.span.file = Some(crate::ast::FileId(file));
            }
        }
        let mut checker = crate::checker::Checker::new();
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "type errors: {:?}", errors);
        let preset = get_preset("strict").expect("preset");
        let issues = lint_ast(
            &program,
            &checker,
            &preset,
            "entry.rad",
            &std::collections::HashMap::new(),
        );
        assert!(
            issues.iter().any(|i| i.message.contains("'acc'")),
            "entry-file finding missing: {:?}",
            issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
        assert!(
            !issues.iter().any(|i| i.message.contains("'basket'")),
            "imported decl was attributed to the entry file: {:?}",
            issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn standard_preset_does_not_run_signature_access_lint() {
        // Opt-in: the default preset must not change behavior.
        let issues: Vec<LintIssue> = lint_with_preset(
            r#"
component Position { x: 0.0 }
component Health { hp: 100 }

system Damage(pos: mut Position) {
    for e in entities(Position) {
        set(e, Health { hp: 10 })
    }
}
"#,
            "standard",
        )
        .into_iter()
        .filter(|i| i.code == "RAD-L015" || i.code == "RAD-L016")
        .collect();
        assert!(issues.is_empty());
    }
}
