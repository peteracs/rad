impl Checker {

    fn check_match_with_mode(&mut self, stmt: &MatchStmt, merge_arm_values: bool) -> Ty {
        let subject_ty = self.check_expr(&stmt.subject);

        let subject_ty = match &subject_ty {
            Ty::Any => self
                .try_infer_match_subject_type(stmt)
                .unwrap_or(subject_ty),
            _ => subject_ty,
        };

        // A guarded arm may not run, so it cannot make a match exhaustive.
        // This mirrors `has_unconditional_wildcard` below, which the
        // primitive-subject check already uses.
        let has_wildcard = stmt
            .cases
            .iter()
            .any(|c| matches!(c.pattern, Pattern::Wildcard) && c.guard.is_none());

        match &subject_ty {
            Ty::State(machine_name) => {
                if let Some(machine) = self.state_machines.get(machine_name).cloned() {
                    let covered = unguarded_variant_names(stmt);
                    let guarded_only = guarded_variant_names(stmt);

                    if !has_wildcard {
                        for state in &machine.states {
                            if !covered.contains(&state) {
                                if guarded_only.contains(&state) {
                                    self.error(
                                        &stmt.span,
                                        format!(
                                            "Non-exhaustive match: state '{}' of machine '{}' is only covered by guarded arms",
                                            state, machine_name
                                        ),
                                        Some(format!(
                                            "A guarded arm is skipped when its guard is false, so the match can fall through. Add an unguarded arm: {} => {{ ... }}",
                                            state
                                        )),
                                    );
                                } else {
                                    self.error(
                                        &stmt.span,
                                        format!(
                                            "Non-exhaustive match: state '{}' of machine '{}' is not covered",
                                            state, machine_name
                                        ),
                                        Some(format!("Add a case: {} => {{ ... }}", state)),
                                    );
                                }
                            }
                        }
                    }

                    for case in &stmt.cases {
                        let path = match &case.pattern {
                            Pattern::Variant { path, .. } => path,
                            _ => continue,
                        };
                        let state_name = path.last().unwrap();
                        if path.len() > 1 {
                            let prefix = path[..path.len() - 1].join(".");
                            if !self.is_type_exported_by_prefix(&prefix, machine_name) {
                                self.error(
                                    &case.span,
                                    format!(
                                        "Prefix '{}' does not match the subject machine '{}'",
                                        prefix, machine_name
                                    ),
                                    None,
                                );
                            }
                        }
                        if !machine.has_state(state_name) {
                            self.error(
                                &stmt.span,
                                format!(
                                    "Unknown state '{}' in match for machine '{}'",
                                    state_name, machine_name
                                ),
                                Some(format!("Known states: {}", machine.states.join(", "))),
                            );
                        }
                    }
                }
            }
            Ty::SumType(type_name) | Ty::App(type_name, _) => {
                if let Some(sum_type) = self.sum_types.get(type_name).cloned() {
                    let covered = unguarded_variant_names(stmt);
                    let guarded_only = guarded_variant_names(stmt);

                    if !has_wildcard {
                        for variant in &sum_type.variants {
                            if !covered.contains(&&variant.name) {
                                let arm = if variant.fields.is_empty() {
                                    variant.name.clone()
                                } else {
                                    let fields = variant
                                        .fields
                                        .iter()
                                        .map(|(n, _)| n.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    format!("{} {{ {} }}", variant.name, fields)
                                };
                                if guarded_only.contains(&&variant.name) {
                                    self.error(
                                        &stmt.span,
                                        format!(
                                            "Non-exhaustive match: variant '{}' of type '{}' is only covered by guarded arms",
                                            variant.name, type_name
                                        ),
                                        Some(format!(
                                            "A guarded arm is skipped when its guard is false, so the match can fall through. Add an unguarded arm: {} => {{ ... }}",
                                            arm
                                        )),
                                    );
                                } else {
                                    self.error(
                                        &stmt.span,
                                        format!(
                                            "Non-exhaustive match: variant '{}' of type '{}' is not covered",
                                            variant.name, type_name
                                        ),
                                        Some(format!("Add a case: {} => {{ ... }}", arm)),
                                    );
                                }
                            }
                        }
                    }

                    for case in &stmt.cases {
                        let path = match &case.pattern {
                            Pattern::Variant { path, .. } => path,
                            _ => continue,
                        };
                        let state_name = path.last().unwrap();
                        if path.len() > 1 {
                            let prefix = path[..path.len() - 1].join(".");
                            if !self.is_type_exported_by_prefix(&prefix, type_name) {
                                self.error(
                                    &case.span,
                                    format!(
                                        "Prefix '{}' does not match the subject type '{}'",
                                        prefix, type_name
                                    ),
                                    None,
                                );
                            }
                        }
                        if !sum_type.variants.iter().any(|v| v.name == *state_name) {
                            self.error(
                                &case.span,
                                format!(
                                    "Unknown variant '{}' in match for type '{}'",
                                    state_name, type_name
                                ),
                                Some(format!(
                                    "Known variants: {}",
                                    sum_type
                                        .variants
                                        .iter()
                                        .map(|v| v.name.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )),
                            );
                        }
                    }
                }
            }
            Ty::Any | Ty::Str | Ty::Int | Ty::Float | Ty::Bool => {}
            other => {
                self.error(
                    &stmt.span,
                    format!("Cannot match on value of type {}", other),
                    Some(
                        "Match can only be used on state machine, sum type, or primitive instances"
                            .to_string(),
                    ),
                );
            }
        }

        let mut seen_states = std::collections::HashSet::new();
        let mut merged: Option<Ty> = None;
        let mut has_unconditional_wildcard = false;

        for case in &stmt.cases {
            if has_unconditional_wildcard {
                self.error(
                    &case.span,
                    "Unreachable match case".to_string(),
                    Some("This case is unreachable because a previous wildcard case without a guard catches all values".to_string()),
                );
            }

            if matches!(case.pattern, Pattern::Wildcard) && case.guard.is_none() {
                has_unconditional_wildcard = true;
            }

            let is_wildcard = matches!(case.pattern, Pattern::Wildcard);
            let literal_pattern = match &case.pattern {
                Pattern::Literal(lit) => Some(lit.clone()),
                _ => None,
            };
            let state_name = match &case.pattern {
                Pattern::Variant { path, .. } => Some(path.last().unwrap().clone()),
                _ => None,
            };

            if !is_wildcard && literal_pattern.is_none() && case.guard.is_none() {
                if let Some(sn) = &state_name {
                    if !seen_states.insert(sn.clone()) {
                        self.error(
                            &case.span,
                            format!("Duplicate match case for '{}'", sn),
                            None,
                        );
                    }
                }
            }

            let mut seen_bindings = std::collections::HashSet::new();
            for binding in self.case_pattern_bindings(case) {
                if !seen_bindings.insert(binding.name.clone()) {
                    self.error(
                        &case.span,
                        format!("Duplicate binding '{}' in match case", binding.name),
                        None,
                    );
                }
            }

            self.push_scope();
            let sum_type_name = match &subject_ty {
                Ty::SumType(n) | Ty::App(n, _) => Some(n.clone()),
                _ => None,
            };
            if let Some(type_name) = sum_type_name {
                if !is_wildcard && literal_pattern.is_none() {
                    if let Some(sum_type) = self.sum_types.get(&type_name).cloned() {
                        if let Some(variant) = sum_type
                            .variants
                            .iter()
                            .find(|v| Some(&v.name) == state_name.as_ref())
                        {
                            let case_bindings = self.case_pattern_bindings(case);
                            let bound_heads: std::collections::HashSet<_> = case_bindings
                                .iter()
                                .filter_map(|b| b.path.first())
                                .collect();
                            let is_bare_variant = match &case.pattern {
                                Pattern::Variant {
                                    is_bare_variant, ..
                                } => *is_bare_variant,
                                _ => false,
                            };
                            let has_rest = match &case.pattern {
                                Pattern::Variant { has_rest, .. } => *has_rest,
                                _ => false,
                            };
                            if !is_bare_variant
                                && !has_rest
                                && bound_heads.len() < variant.fields.len()
                            {
                                // `..` needs --compat-v0.5-dx, so only suggest
                                // it when that mode is actually on. Otherwise
                                // spell out the form that works today: bind
                                // every field, giving the unwanted ones `_`
                                // names so they read as discards and do not
                                // trip the unused-variable warning.
                                let hint = if self.options.compat_v0_5_dx {
                                    "Use `..` to ignore remaining fields".to_string()
                                } else {
                                    let spelled = variant
                                        .fields
                                        .iter()
                                        .map(|(n, _)| {
                                            if bound_heads.iter().any(|b| *b == n) {
                                                n.clone()
                                            } else {
                                                format!("{}: _{}", n, n)
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    format!(
                                        "Bind every field, using `field: _name` for the ones you do not need: {} {{ {} }}. (`..` requires --compat-v0.5-dx.)",
                                        variant.name, spelled
                                    )
                                };
                                self.error(
                                    &case.span,
                                    format!(
                                        "Pattern does not bind all fields of variant '{}'",
                                        variant.name
                                    ),
                                    Some(hint),
                                );
                            }
                            for binding in &case_bindings {
                                if let Some(field_ty) = self.resolve_case_binding_ty(
                                    &sum_type,
                                    &subject_ty,
                                    variant,
                                    binding,
                                    &case.span,
                                ) {
                                    self.define(
                                        &binding.name,
                                        field_ty,
                                        false,
                                        case.span.clone(),
                                        false,
                                        true,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if let Some(lit) = &literal_pattern {
                let lit_ty = self.check_expr(lit);
                if lit_ty != subject_ty && lit_ty != Ty::Any && subject_ty != Ty::Any {
                    self.error(
                        &case.span,
                        format!(
                            "Mismatched types: expected {}, found {}",
                            subject_ty, lit_ty
                        ),
                        None,
                    );
                }
            }
            if let Pattern::HasComponent {
                binding: Some(bind_name),
                ..
            } = &case.pattern
            {
                self.define(bind_name, Ty::Any, false, case.span.clone(), false, true);
            }
            if let Some(guard) = &case.guard {
                let guard_ty = self.check_expr(guard);
                if guard_ty != Ty::Bool && guard_ty != Ty::Any {
                    self.error(
                        guard.span(),
                        format!("Match guard must be bool, got {}", guard_ty),
                        Some("Use a boolean condition after `when`/`if` in match arms".to_string()),
                    );
                }
            }
            let arm_ty = self.check_block(&case.body);
            if merge_arm_values {
                merged = Some(match merged {
                    None => arm_ty,
                    Some(prev) if prev.assignable_from(&arm_ty) => prev,
                    Some(prev) if arm_ty.assignable_from(&prev) => arm_ty,
                    Some(prev) => {
                        if prev != Ty::Any && arm_ty != Ty::Any {
                            self.error(
                                &case.span,
                                format!(
                                    "Match arms have incompatible types: expected {}, found {}",
                                    prev, arm_ty
                                ),
                                Some("All match arms must return the same type".to_string()),
                            );
                        }
                        Ty::Any
                    }
                });
            }
            self.pop_scope();
        }

        if !has_unconditional_wildcard
            && matches!(subject_ty, Ty::Str | Ty::Int | Ty::Float | Ty::Bool)
        {
            self.error(
                &stmt.span,
                format!(
                    "Non-exhaustive match: matching on {} requires a wildcard `_` arm",
                    subject_ty
                ),
                Some("Add a case: _ => { ... }".to_string()),
            );
        }

        if merge_arm_values {
            merged.unwrap_or(Ty::Nil)
        } else {
            Ty::Nil
        }
    }

    fn try_infer_match_subject_type(&mut self, stmt: &MatchStmt) -> Option<Ty> {
        let variant_names: Vec<&String> = stmt
            .cases
            .iter()
            .filter_map(|c| match &c.pattern {
                Pattern::Variant { path, .. } => path.last(),
                _ => None,
            })
            .collect();

        if variant_names.is_empty() {
            return None;
        }

        let mut candidates: Vec<&String> = Vec::new();
        for (type_name, sum_type) in &self.sum_types {
            let all_match = variant_names
                .iter()
                .all(|vn| sum_type.variants.iter().any(|v| &v.name == *vn));
            if all_match {
                candidates.push(type_name);
            }
        }

        if candidates.len() == 1 {
            let name = candidates[0].clone();
            let sum_type = &self.sum_types[&name];
            if sum_type.type_params.is_empty() {
                return Some(Ty::SumType(name));
            }
            let args = sum_type.type_params.iter().map(|_| Ty::Any).collect();
            return Some(Ty::App(name, args));
        }

        let has_destructuring = stmt.cases.iter().any(|c| match &c.pattern {
            Pattern::Variant {
                bindings,
                pattern_bindings,
                ..
            } => !bindings.is_empty() || !pattern_bindings.is_empty(),
            _ => false,
        });
        let subject_hint = if let Expr::Ident(name, _) = &stmt.subject {
            format!(
                "Add a type annotation to this binding/parameter, e.g. `fn ...({}: MyType) ...`",
                name
            )
        } else {
            "Add a type annotation to the parameter, e.g. `fn foo(x: MyType)`".to_string()
        };

        if has_destructuring {
            if candidates.is_empty() {
                self.error(
                    &stmt.span,
                    "Match arms use destructuring patterns but the subject type is unknown"
                        .to_string(),
                    Some(subject_hint.clone()),
                );
            } else {
                let names: Vec<&str> = candidates.iter().map(|s| s.as_str()).collect();
                self.error(
                    &stmt.span,
                    format!(
                        "Match arms use destructuring patterns but the subject type is \
                         ambiguous (could be {})",
                        names.join(" or ")
                    ),
                    Some(subject_hint),
                );
            }
        }

        None
    }

    fn block_diverges(&self, block: &Block) -> bool {
        block.stmts.iter().any(|s| self.stmt_diverges(s))
    }

    fn stmt_diverges(&self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::If(s) => {
                self.block_diverges(&s.then_block)
                    && s.else_block
                        .as_ref()
                        .is_some_and(|b| self.block_diverges(b))
            }
            Stmt::Match(m) => {
                !m.cases.is_empty() && m.cases.iter().all(|c| self.block_diverges(&c.body))
            }
            Stmt::LetElse(_) => false,
            _ => false,
        }
    }

    pub(super) fn check_block(&mut self, block: &Block) -> Ty {
        let mut diverged = false;
        let mut block_ty = Ty::Nil;
        for (i, stmt) in block.stmts.iter().enumerate() {
            if diverged {
                self.error(
                    stmt.span(),
                    "Unreachable code".to_string(),
                    Some("This code will never be executed because a previous statement diverges (e.g., returns, breaks, or continues)".to_string()),
                );
                break;
            }
            if i == block.stmts.len() - 1 {
                if let Stmt::Expr(s) = stmt {
                    block_ty = self.check_expr(&s.expr);
                    self.error_if_ignored_transform_result(&s.expr, &s.span);
                    if self.stmt_diverges(stmt) {
                        diverged = true;
                    }
                    continue;
                }
            }
            self.check_stmt(stmt);
            if self.stmt_diverges(stmt) {
                diverged = true;
            }
        }
        block_ty
    }

    pub(super) fn check_expr(&mut self, expr: &Expr) -> Ty {
        match expr {
            Expr::IntLit(..) | Expr::FloatLit(..) | Expr::StrLit(..) | Expr::BoolLit(..) | Expr::NilLit(..) | Expr::TupleLit(..) | Expr::Spread(..) | Expr::ListLit(..) | Expr::MapLit(..) | Expr::FStringExpr(..) => self.check_expr_values(expr),
            Expr::Ident(..) | Expr::Binary(..) | Expr::Unary(..) | Expr::Pipe(..) | Expr::Call(..) | Expr::Try(..) | Expr::Await(..) | Expr::AsyncCall(..) => self.check_expr_operations(expr),
            Expr::Field(..) | Expr::Index(..) | Expr::ComponentExpr(..) | Expr::VariantExpr(..) | Expr::StateRef(..) => self.check_expr_access_and_construction(expr),
            Expr::SystemRef(..) | Expr::MatchExpr(..) | Expr::IfExpr(..) | Expr::QueryExpr(..) | Expr::FnExpr(..) | Expr::EntityLiteral(..) | Expr::Error(..) => self.check_expr_control_and_functions(expr),
        }
    }

fn check_expr_values(&mut self, expr: &Expr) -> Ty {
        match expr {
            Expr::IntLit(_, _) => Ty::Int,
            Expr::FloatLit(_, _) => Ty::Float,
            Expr::StrLit(_, _) => Ty::Str,
            Expr::BoolLit(_, _) => Ty::Bool,
            Expr::NilLit(_) => Ty::Nil,
            Expr::TupleLit(elems, _span) => {
                let elem_tys: Vec<Ty> = elems.iter().map(|e| self.check_expr(e)).collect();
                Ty::Tuple(elem_tys)
            }
            Expr::Spread(_expr, span) => {
                self.error(
                    span,
                    "Spread operator is not allowed here".to_string(),
                    None,
                );
                Ty::Any
            }
            Expr::ListLit(elems, span) => {
                if elems.is_empty() {
                    Ty::List(Box::new(Ty::Any))
                } else {
                    let first_ty = self.check_expr(&elems[0]);
                    let mut elem_ty = first_ty.clone();
                    for elem in &elems[1..] {
                        let et = self.check_expr(elem);
                        if elem_ty != Ty::Any && et != Ty::Any && !elem_ty.assignable_from(&et) {
                            if et.assignable_from(&elem_ty) {
                                elem_ty = et;
                            } else {
                                if self.suppress_mixed_list_warnings == 0 {
                                    self.warning(
                                        span,
                                        format!(
                                            "List contains mixed types ({} and {}); inferred as list<any>",
                                            elem_ty, et
                                        ),
                                        Some(
                                            "Use a component or separate variables for structured heterogeneous data, \
                                             or annotate the binding as `list<any>` to silence this warning"
                                                .to_string(),
                                        ),
                                    );
                                }
                                elem_ty = Ty::Any;
                            }
                        }
                    }
                    Ty::List(Box::new(elem_ty))
                }
            }
            Expr::MapLit(entries, _) => {
                if entries.is_empty() {
                    return Ty::Map(Box::new(Ty::Any), Box::new(Ty::Any));
                }
                let key_ty = self.check_expr(&entries[0].0);
                let val_ty = self.check_expr(&entries[0].1);
                if !key_ty.is_valid_map_key() {
                    self.error(
                        entries[0].0.span(),
                        format!(
                            "Type {} cannot be used as a map key (allowed: int, str, bool, entity, and tuples of those)",
                            key_ty
                        ),
                        None,
                    );
                }
                let mut key_ty = key_ty;
                let mut val_ty = val_ty;
                for (k, v) in entries.iter().skip(1) {
                    let entry_key_ty = self.check_expr(k);
                    let entry_val_ty = self.check_expr(v);
                    if !entry_key_ty.is_valid_map_key() {
                        self.error(
                            k.span(),
                            format!(
                                "Type {} cannot be used as a map key (allowed: int, str, bool, entity, and tuples of those)",
                                entry_key_ty
                            ),
                            None,
                        );
                    }
                    if self.subst.unify(&entry_key_ty, &key_ty).is_err() {
                        self.error(
                            k.span(),
                            "Heterogeneous map literals are not allowed — all keys must share one type (use a struct for mixed-key records)"
                                .to_string(),
                            Some(
                                "Map keys in a single literal must unify with the first entry's key type (aliases and inference apply)."
                                    .to_string(),
                            ),
                        );
                    }
                    // Mixed value types widen to map<K, any> with a warning,
                    // mirroring list literals — record-shaped data still gets
                    // pointed at structs, but scripting maps stay usable.
                    if val_ty != Ty::Any && self.subst.unify(&entry_val_ty, &val_ty).is_err() {
                        if self.suppress_mixed_list_warnings == 0 {
                            self.warning(
                                v.span(),
                                format!(
                                    "Map contains mixed value types ({} and {}); inferred as map with any values",
                                    self.subst.resolve(&val_ty),
                                    self.subst.resolve(&entry_val_ty)
                                ),
                                Some(
                                    "Use a struct for fixed-shape records, or annotate the binding as `map<K, any>` to silence this warning"
                                        .to_string(),
                                ),
                            );
                        }
                        val_ty = Ty::Any;
                        key_ty = self.subst.resolve(&key_ty);
                        continue;
                    }
                    key_ty = self.subst.resolve(&key_ty);
                    val_ty = self.subst.resolve(&val_ty);
                }
                Ty::Map(
                    Box::new(self.subst.resolve(&key_ty)),
                    Box::new(self.subst.resolve(&val_ty)),
                )
            }
            Expr::FStringExpr(parts, _) => {
                for part in parts {
                    if let crate::ast::FStringPart::Expr(expr, _) = part {
                        self.check_expr(expr);
                    }
                }
                Ty::Str
            }
            _ => unreachable!("dispatcher selected the wrong match partition"),
        }
    }
}

/// Variant/state names matched by arms with NO guard. Only these make a
/// match exhaustive: a guarded arm is skipped when its guard is false, so
/// the match can fall through and evaluate to nil.
fn unguarded_variant_names(stmt: &MatchStmt) -> Vec<&String> {
    variant_names_where(stmt, |case| case.guard.is_none())
}

/// Variant/state names that appear ONLY on guarded arms, used to tell the
/// author "you wrote this arm, but it is conditional" instead of the
/// misleading "variant is not covered".
fn guarded_variant_names(stmt: &MatchStmt) -> Vec<&String> {
    variant_names_where(stmt, |case| case.guard.is_some())
}

fn variant_names_where(stmt: &MatchStmt, keep: fn(&MatchCase) -> bool) -> Vec<&String> {
    stmt.cases
        .iter()
        .filter(|case| keep(case))
        .filter_map(|case| match &case.pattern {
            Pattern::Variant { path, .. } => path.last(),
            _ => None,
        })
        .collect()
}
