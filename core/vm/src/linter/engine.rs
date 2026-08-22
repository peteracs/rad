

pub fn get_preset(name: &str) -> Option<LintPreset> {
    match name {
        // The default. Real warnings (naming, size, complexity) without
        // fighting the language's inference-first personality: no demands
        // for type annotations on `let` in a language that infers them.
        "standard" => Some(LintPreset {
            description:
                "Sensible defaults: style and complexity warnings, no annotation requirements",
            vm_flags: vec![],
            require_type_annotations: false,
            require_pure_pipelines: false,
            require_ecs_system_flow: false,
            max_function_lines: 120,
            max_file_lines: 2000,
            require_event_handlers: false,
            no_unused_imports: false,
            require_match_exhaustive: false,
            require_effect_annotations: false,
            naming_convention: "PascalCase_for_types",
            suggest_type_annotations: false,
            suggest_pure_fn: false,
            warn_complex_pipelines: true,
            warn_imperative_collection_building: false,
            warn_bare_print: false,
            require_aliased_imports: false,
        }),
        "enterprise" => Some(LintPreset {
            description: "Maximum safety for production codebases",
            vm_flags: vec!["--strict-types", "--deny-warnings"],
            require_type_annotations: true,
            require_pure_pipelines: true,
            require_ecs_system_flow: true,
            max_function_lines: 50,
            max_file_lines: 500,
            require_event_handlers: true,
            no_unused_imports: true,
            require_match_exhaustive: true,
            require_effect_annotations: true,
            naming_convention: "PascalCase_for_types",
            suggest_type_annotations: false,
            suggest_pure_fn: false,
            warn_complex_pipelines: false,
            warn_imperative_collection_building: true,
            warn_bare_print: true,
            require_aliased_imports: true,
        }),
        "strict" => Some(LintPreset {
            description: "Strict type checking with warnings as errors",
            vm_flags: vec!["--strict-types", "--deny-warnings"],
            require_type_annotations: true,
            require_pure_pipelines: true,
            require_ecs_system_flow: true,
            max_function_lines: 80,
            max_file_lines: 1000,
            require_event_handlers: false,
            no_unused_imports: false,
            require_match_exhaustive: true,
            require_effect_annotations: false,
            naming_convention: "",
            suggest_type_annotations: false,
            suggest_pure_fn: false,
            warn_complex_pipelines: false,
            warn_imperative_collection_building: true,
            warn_bare_print: true,
            require_aliased_imports: true,
        }),
        "teaching" => Some(LintPreset {
            description: "Beginner-friendly with helpful suggestions",
            vm_flags: vec![],
            require_type_annotations: false,
            require_pure_pipelines: false,
            require_ecs_system_flow: false,
            max_function_lines: 100,
            max_file_lines: 2000,
            require_event_handlers: false,
            no_unused_imports: false,
            require_match_exhaustive: false,
            require_effect_annotations: false,
            naming_convention: "",
            suggest_type_annotations: true,
            suggest_pure_fn: true,
            warn_complex_pipelines: true,
            warn_imperative_collection_building: true,
            warn_bare_print: false,
            require_aliased_imports: false,
        }),
        _ => None,
    }
}

pub fn lint_source(source: &str, preset_name: &str) -> (Vec<LintIssue>, LintPreset) {
    let preset = get_preset(preset_name).unwrap_or_else(|| get_preset("standard").unwrap());
    let mut issues = Vec::new();
    let lines: Vec<&str> = source.split('\n').collect();

    if preset.max_file_lines > 0 && lines.len() > preset.max_file_lines {
        issues.push(LintIssue {
            line: lines.len() as u32,
            col: 0,
            severity: "warning",
            code: "RAD-L001",
            message: format!(
                "File has {} lines, exceeds limit of {}",
                lines.len(),
                preset.max_file_lines
            ),
        });
    }

    let mut fn_start = None;
    let mut fn_name = String::new();
    let mut depth = 0;
    let mut entity_lines = Vec::new();
    let mut system_lines = Vec::new();
    let mut system_names: Vec<String> = Vec::new();
    let mut systems_are_run = false;

    for (i, line) in lines.iter().enumerate() {
        let line_num = (i + 1) as u32;
        let stripped = line.trim();

        if (stripped.starts_with("fn ") || stripped.starts_with("pure fn "))
            && stripped.ends_with('{')
        {
            fn_start = Some(line_num);
            let parts: Vec<&str> = stripped.split_whitespace().collect();
            if let Some(name_part) = parts.iter().find(|&&p| p != "fn" && p != "pure") {
                fn_name = name_part.split('(').next().unwrap_or("").to_string();
            }
            depth = 1;
            continue;
        }

        if !stripped.starts_with("//") {
            let decl = stripped.strip_prefix("pub ").unwrap_or(stripped);
            if decl.starts_with("entity ") {
                entity_lines.push(line_num);
            }
            if let Some(rest) = decl.strip_prefix("system ") {
                system_lines.push(line_num);
                let name: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    system_names.push(name);
                }
            }
            // The ways a program actually runs systems: `schedule [A, B]`
            // (directly or through a `phase` group), `simulate(...)` /
            // `simulate_par(...)` on a fork, a `system::Name` reference, or
            // a direct `SystemName()` call (checked after the scan, once
            // every declared name is known). The old check looked for a
            // `run` statement, which does not exist in the language: it
            // missed all of the real forms and was silenced by any line
            // that happened to start with `run `.
            if decl.starts_with("schedule")
                || decl.starts_with("phase ")
                || decl.contains("simulate(")
                || decl.contains("simulate_par(")
                || decl.contains("system::")
            {
                systems_are_run = true;
            }
        }

        if let Some(start) = fn_start {
            let opens = line.chars().filter(|&c| c == '{').count() as i32;
            let closes = line.chars().filter(|&c| c == '}').count() as i32;
            depth += opens - closes;
            if depth <= 0 {
                let fn_length = line_num - start;
                if fn_length > preset.max_function_lines as u32 {
                    issues.push(LintIssue {
                        line: start,
                        col: 0,
                        severity: "warning",
                        code: "RAD-L002",
                        message: format!(
                            "Function '{}' is {} lines, exceeds limit of {}",
                            fn_name, fn_length, preset.max_function_lines
                        ),
                    });
                }
                fn_start = None;
                depth = 0;
            }
        }

        if preset.naming_convention == "PascalCase_for_types" {
            for kw in &["component", "state", "type", "event"] {
                if stripped.starts_with(&format!("{} ", kw)) {
                    let parts: Vec<&str> = stripped.split_whitespace().collect();
                    if parts.len() > 1 {
                        let name = parts[1];
                        if !name.chars().next().is_some_and(|c| c.is_uppercase()) {
                            issues.push(LintIssue {
                                line: line_num,
                                col: 0,
                                severity: "info",
                                code: "RAD-L003",
                                message: format!("{} name '{}' should use PascalCase", kw, name),
                            });
                        }
                    }
                }
            }
        }

        if preset.suggest_type_annotations && stripped.starts_with("let ") {
            let parts: Vec<&str> = stripped.split('=').collect();
            if !parts.is_empty() && !parts[0].contains(':') {
                let name = parts[0]
                    .trim()
                    .strip_prefix("let ")
                    .unwrap_or("")
                    .trim()
                    .strip_prefix("mut ")
                    .unwrap_or(parts[0].trim().strip_prefix("let ").unwrap_or(""));
                issues.push(LintIssue {
                    line: line_num,
                    col: 0,
                    severity: "info",
                    code: "RAD-L004",
                    message: format!(
                        "Consider adding a type annotation: let {}: Type = ...",
                        name
                    ),
                });
            }
        }

        if preset.suggest_pure_fn && stripped.starts_with("fn ") && source.contains("|>") {
            issues.push(LintIssue {
                line: line_num,
                col: 0,
                severity: "info",
                code: "RAD-L005",
                message: "Consider marking pipeline-safe functions as 'pure fn'".to_string(),
            });
        }

        if preset.warn_complex_pipelines {
            let pipe_count = stripped.matches("|>").count();
            if pipe_count > 5 {
                issues.push(LintIssue {
                    line: line_num,
                    col: 0,
                    severity: "warning",
                    code: "RAD-L006",
                    message: format!(
                        "Pipeline has {} stages — consider breaking into named steps",
                        pipe_count
                    ),
                });
            }
        }

        if !stripped.is_empty()
            && !stripped.starts_with("//")
            && (line.ends_with(' ') || line.ends_with('\t'))
        {
            issues.push(LintIssue {
                line: line_num,
                col: line.trim_end().len() as u32,
                severity: "info",
                code: "RAD-L007",
                message: "Trailing whitespace".to_string(),
            });
        }
    }

    // Direct calls (`SystemName()`) need the full set of declared names,
    // so they are resolved in a second pass.
    if !systems_are_run && !system_names.is_empty() {
        'scan: for line in &lines {
            let stripped = line.trim();
            let decl = stripped.strip_prefix("pub ").unwrap_or(stripped);
            if stripped.starts_with("//") || decl.starts_with("system ") {
                continue;
            }
            for name in &system_names {
                let needle = format!("{}(", name);
                let mut from = 0;
                while let Some(rel) = stripped[from..].find(&needle) {
                    let at = from + rel;
                    let boundary = at == 0 || {
                        let b = stripped.as_bytes()[at - 1];
                        !(b.is_ascii_alphanumeric() || b == b'_')
                    };
                    if boundary {
                        systems_are_run = true;
                        break 'scan;
                    }
                    from = at + needle.len();
                }
            }
        }
    }

    if preset.require_ecs_system_flow {
        if !entity_lines.is_empty() && system_lines.is_empty() {
            issues.push(LintIssue {
                line: entity_lines[0],
                col: 0,
                severity: "warning",
                code: "RAD-L008",
                message: "Entity declarations found without any systems; move logic into `system` blocks for ECS flow".to_string(),
            });
        } else if !system_lines.is_empty() && !systems_are_run {
            issues.push(LintIssue {
                line: system_lines[0],
                col: 0,
                severity: "warning",
                code: "RAD-L009",
                // mirror the checker's unused-system hint: these are the
                // spellings that actually exist (there is no `run` statement)
                message: "Systems are declared but never run; run them with `SystemName()` or `schedule [A, B]`, or list them in `simulate(fork, [system::SystemName], ticks)`"
                    .to_string(),
            });
        }
    }

    (issues, preset)
}

/// Does this declaration belong to the file being linted? The module
/// loader merges imported modules into the entry program and tags every
/// span with a FileId; the entry file is always FileId(0) (None means the
/// AST was built without a source map). Imported declarations must not be
/// linted here: their findings would be printed under this file's heading
/// with the OTHER file's line numbers, and repeated once per importer —
/// they are reported when their own file is linted.
fn is_entry_decl(decl: &crate::ast::Decl) -> bool {
    match decl.span() {
        Some(span) => matches!(span.file, None | Some(crate::ast::FileId(0))),
        None => true,
    }
}

pub fn lint_ast(
    program: &crate::ast::Program,
    checked: &crate::types::CheckerOutput,
    preset: &LintPreset,
    filepath: &str,
    boundaries: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<LintIssue> {
    let mut issues = Vec::new();

    if preset.require_effect_annotations {
        for decl in &program.declarations {
            if !is_entry_decl(decl) {
                continue;
            }
            if let crate::ast::Decl::Fn(f) = decl {
                if !f.is_pure && f.effects.is_empty() {
                    if let Some(sig) = checked.functions.get(&f.name) {
                        if !sig.is_pure {
                            issues.push(LintIssue {
                                line: f.span.line,
                                col: f.span.col,
                                severity: "warning",
                                code: "RAD-L010",
                                message: format!("Function '{}' has side effects but does not declare them. Add 'io', 'ecs', or 'event' to its signature.", f.name),
                            });
                        }
                    }
                }
            }
        }
    }

    if preset.warn_imperative_collection_building
        || preset.warn_bare_print
        || preset.require_aliased_imports
        || !boundaries.is_empty()
    {
        let mut visitor = AstLintVisitor {
            issues: Vec::new(),
            warn_imperative_collection_building: preset.warn_imperative_collection_building,
            warn_bare_print: preset.warn_bare_print,
            require_aliased_imports: preset.require_aliased_imports,
            filepath: filepath.to_string(),
            boundaries: boundaries.clone(),
        };
        crate::visitor::AstVisitor::visit_program(&mut visitor, program);
        issues.extend(visitor.issues);
    }

    issues
}

struct AstLintVisitor {
    issues: Vec<LintIssue>,
    warn_imperative_collection_building: bool,
    warn_bare_print: bool,
    require_aliased_imports: bool,
    filepath: String,
    boundaries: std::collections::HashMap<String, Vec<String>>,
}

// The lint pass rides the shared AST walker rather than carrying its own.
//
// It used to define a parallel `visit_*` set whose `Expr`/`Stmt` matches both
// ended in `_ => {}`. That is quieter than a duplicate walker deserves: adding
// a syntax form did not fail to compile, it simply stopped being linted inside,
// so rule coverage shrank without anyone being told. Overriding only the nodes
// a rule inspects and delegating the rest to `walk_*` keeps new syntax covered
// by construction.
impl crate::visitor::AstVisitor for AstLintVisitor {
    fn visit_program(&mut self, program: &crate::ast::Program) {
        for decl in &program.declarations {
            if !is_entry_decl(decl) {
                continue;
            }
            self.visit_decl(decl);
        }
    }

    fn visit_decl(&mut self, decl: &crate::ast::Decl) {
        use crate::ast::Decl;
        match decl {
            Decl::Fn(f) => self.visit_block(&f.body),
            Decl::System(s) => self.visit_block(&s.body),
            Decl::OnHandler(h) => self.visit_block(&h.body),
            Decl::Test(t) => self.visit_block(&t.body),
            Decl::Model(model) => {
                for command in &model.commands {
                    self.visit_expr(command);
                }
                for invariant in &model.invariants {
                    self.visit_block(invariant);
                }
            }
            Decl::Use(u) => {
                if self.require_aliased_imports && u.alias.is_none() {
                    self.issues.push(LintIssue {
                        line: u.span.line,
                        col: u.span.col,
                        severity: "error",
                        code: "RAD-L013",
                        message: format!("Bare import `use \"{}\"` is not allowed. Use `as` to alias the import.", u.path),
                    });
                }

                if !self.boundaries.is_empty() {
                    let current_mod = std::path::Path::new(&self.filepath)
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    let target_mod = std::path::Path::new(&u.path)
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    if !current_mod.is_empty()
                        && !target_mod.is_empty()
                        && current_mod != target_mod
                    {
                        if let Some(allowed) = self.boundaries.get(&current_mod) {
                            if !allowed.contains(&target_mod) {
                                self.issues.push(LintIssue {
                                    line: u.span.line,
                                    col: u.span.col,
                                    severity: "error",
                                    code: "RAD-L014",
                                    message: format!("Module boundary violation: '{}' is not allowed to import from '{}'", current_mod, target_mod),
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn visit_block(&mut self, block: &crate::ast::Block) {
        for stmt in &block.stmts {
            self.visit_stmt(stmt);
        }
    }

    fn visit_stmt(&mut self, stmt: &crate::ast::Stmt) {
        use crate::ast::Stmt;
        match stmt {
            Stmt::For(f) => {
                self.check_imperative_loop(&f.body, &f.span);
                self.visit_block(&f.body);
            }
            Stmt::While(w) => {
                self.check_imperative_loop(&w.body, &w.span);
                self.visit_block(&w.body);
            }
            Stmt::If(i) => {
                self.visit_block(&i.then_block);
                if let Some(else_block) = &i.else_block {
                    self.visit_block(else_block);
                }
            }
            Stmt::LetElse(l) => {
                self.visit_expr(&l.subject);
                self.visit_block(&l.else_block);
            }
            Stmt::Match(m) => {
                self.visit_expr(&m.subject);
                for case in &m.cases {
                    if let Some(guard) = &case.guard {
                        self.visit_expr(guard);
                    }
                    self.visit_block(&case.body);
                }
            }
            Stmt::Let(l) => self.visit_expr(&l.value),
            Stmt::Assign(a) => {
                self.visit_expr(&a.target);
                self.visit_expr(&a.value);
            }
            Stmt::Return(r) => {
                if let Some(e) = &r.value {
                    self.visit_expr(e);
                }
            }
            Stmt::Emit(e) => {
                for (_, expr) in &e.fields {
                    self.visit_expr(expr);
                }
                if let Some(d) = &e.delay {
                    self.visit_expr(d);
                }
            }
            Stmt::Expr(e) => self.visit_expr(&e.expr),
            other => crate::visitor::walk_stmt(self, other),
        }
    }

    fn visit_expr(&mut self, expr: &crate::ast::Expr) {
        use crate::ast::Expr;
        match expr {
            Expr::Call(callee, args, span) => {
                if self.warn_bare_print {
                    if let Expr::Ident(name, _) = &**callee {
                        if name == "print" || name == "eprint" {
                            self.issues.push(LintIssue {
                                line: span.line,
                                col: span.col,
                                severity: "warning",
                                code: "RAD-L012",
                                message: format!("Bare `{}` used. In enterprise code, prefer structured logging with `log()`.", name),
                            });
                        }
                    }
                }
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            Expr::Binary(l, _, r, _) => {
                self.visit_expr(l);
                self.visit_expr(r);
            }
            Expr::Unary(_, e, _) => self.visit_expr(e),
            Expr::Pipe(l, r, _) => {
                self.visit_expr(l);
                self.visit_expr(r);
            }
            Expr::Field(e, _, _) => self.visit_expr(e),
            Expr::Index(e, i, _) => {
                self.visit_expr(e);
                self.visit_expr(i);
            }
            Expr::ComponentExpr(_, fields, spread, _) => {
                for (_, e) in fields {
                    self.visit_expr(e);
                }
                if let Some(s) = spread {
                    self.visit_expr(s);
                }
            }
            Expr::VariantExpr(_, _, fields, _) => {
                for (_, e) in fields {
                    self.visit_expr(e);
                }
            }
            Expr::ListLit(items, _) | Expr::TupleLit(items, _) => {
                for item in items {
                    self.visit_expr(item);
                }
            }
            Expr::MapLit(items, _) => {
                for (k, v) in items {
                    self.visit_expr(k);
                    self.visit_expr(v);
                }
            }
            Expr::FStringExpr(parts, _) => {
                for part in parts {
                    if let crate::ast::FStringPart::Expr(e, _) = part {
                        self.visit_expr(e);
                    }
                }
            }
            Expr::MatchExpr(m, _) => {
                self.visit_expr(&m.subject);
                for case in &m.cases {
                    if let Some(guard) = &case.guard {
                        self.visit_expr(guard);
                    }
                    self.visit_block(&case.body);
                }
            }
            Expr::IfExpr(c, t, e, _) => {
                self.visit_expr(c);
                self.visit_expr(t);
                self.visit_expr(e);
            }
            Expr::FnExpr(_, _, _, _, _, body, _) => self.visit_block(body),
            Expr::Await(e, _) => self.visit_expr(e),
            Expr::AsyncCall(callee, args, _) => {
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            Expr::Try(e, _) => self.visit_expr(e),
            Expr::Spread(e, _) => self.visit_expr(e),
            Expr::EntityLiteral(name, components, _) => {
                if let Some(name_expr) = name {
                    self.visit_expr(name_expr);
                }
                for entry in components {
                    match entry {
                        crate::ast::ComponentEntry::Init(ci) => {
                            for (_name, fexpr) in &ci.fields {
                                self.visit_expr(fexpr);
                            }
                        }
                        crate::ast::ComponentEntry::Expr(ex) => self.visit_expr(ex),
                    }
                }
            }
            other => crate::visitor::walk_expr(self, other),
        }
    }

}

impl AstLintVisitor {
    fn check_imperative_loop(&mut self, block: &crate::ast::Block, span: &crate::ast::Span) {
        if !self.warn_imperative_collection_building {
            return;
        }
        use crate::ast::{Expr, Stmt};
        if block.stmts.len() == 1 {
            if let Stmt::Assign(a) = &block.stmts[0] {
                if let Expr::Call(callee, args, _) = &a.value {
                    if let Expr::Ident(name, _) = &**callee {
                        if name == "push" && args.len() == 2 {
                            if let (Expr::Ident(target_name, _), Expr::Ident(arg0_name, _)) =
                                (&a.target, &args[0])
                            {
                                if target_name == arg0_name {
                                    self.issues.push(LintIssue {
                                        line: span.line,
                                        col: span.col,
                                        severity: "info",
                                        code: "RAD-L011",
                                        message: format!("Imperative collection building detected for '{}'. Consider using a pipeline with `map` or `filter`.", target_name),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
