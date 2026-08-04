impl Checker {

    fn check_fn_decl(&mut self, decl: &FnDecl) {
        let require_types = !decl.is_pub && self.options.strict_types;

        if require_types {
            for (idx, param) in decl.params.iter().enumerate() {
                if decl.param_types.get(idx).and_then(|t| t.as_ref()).is_none() {
                    self.error(
                        &decl.span,
                        format!(
                            "Strict types: parameter '{}' in function '{}' needs a type annotation",
                            param, decl.name
                        ),
                        Some("Add a type, e.g. `param: int`".to_string()),
                    );
                }
            }
            if decl.return_type.is_none() {
                self.error(
                    &decl.span,
                    format!(
                        "Strict types: function '{}' needs an explicit return type",
                        decl.name
                    ),
                    Some("Declare `-> Type` on the function signature".to_string()),
                );
            }
        } else {
            for (idx, param) in decl.params.iter().enumerate() {
                if decl.param_types.get(idx).and_then(|t| t.as_ref()).is_none() {
                    self.warning(
                        &decl.span,
                        format!(
                            "Parameter '{}' in function '{}' has no type annotation; defaulting to any",
                            param, decl.name
                        ),
                        Some("Add a type, e.g. `param: int` or use a generic parameter like `T`".to_string()),
                    );
                }
            }
            if decl.return_type.is_none() {
                self.warning(
                    &decl.span,
                    format!(
                        "Function '{}' has no explicit return type; defaulting to inferred/any",
                        decl.name
                    ),
                    Some("Declare `-> Type` on the function signature".to_string()),
                );
            }
        }
        self.validate_fn_param_alignment(
            &format!("function '{}'", decl.name),
            decl.params.len(),
            decl.param_types.len(),
            &decl.span,
        );
        self.push_type_param_scope(&decl.type_params);
        let param_tys = decl
            .params
            .iter()
            .enumerate()
            .map(|(idx, _)| {
                decl.param_types
                    .get(idx)
                    .and_then(|ann| ann.as_ref())
                    .map_or(Ty::Any, |te| self.resolve_type_expr(te, &decl.span))
            })
            .collect::<Vec<_>>();
        // Keep in lockstep with register_function: bare fn-typed params of
        // an explicitly effect-annotated fn are promoted (readonly rows get
        // readonly callbacks, everything else pure). This pass overwrites
        // sig.params below, so both passes must agree.
        let param_tys = if decl.is_pure || !decl.effects.is_empty() {
            let allows_reads = self
                .functions
                .get(&decl.name)
                .is_some_and(|sig| sig.effects.allows(Effect::ReadECS));
            let target = if allows_reads {
                FnPurity::Readonly
            } else {
                FnPurity::Pure
            };
            Self::promote_callback_params(param_tys, target)
        } else {
            param_tys
        };
        let declared_ret = decl
            .return_type
            .as_ref()
            .map(|te| self.resolve_type_expr(te, &decl.span));
        let declared_ret = if decl.is_async {
            Some(Ty::Task(Box::new(declared_ret.clone().unwrap_or(Ty::Any))))
        } else {
            declared_ret
        };
        self.push_scope();
        if let Some(sig) = self.functions.get(&decl.name) {
            self.scopes.last_mut().unwrap().effect_context = sig.effects.clone();
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.in_async = decl.is_async;
        }
        {
            let mut seen_params = std::collections::HashSet::new();
            for param in &decl.params {
                if !seen_params.insert(param.clone()) {
                    self.error(
                        &decl.span,
                        format!(
                            "Duplicate parameter '{}' in function '{}'",
                            param, decl.name
                        ),
                        None,
                    );
                }
            }
        }
        for (idx, param) in decl.params.iter().enumerate() {
            let ty = param_tys.get(idx).cloned().unwrap_or(Ty::Any);
            let is_mut = decl.param_muts.get(idx).copied().unwrap_or(false);
            self.define(param, ty, is_mut, decl.span.clone(), false, true);
        }
        self.current_fn_name = Some(decl.name.clone());
        self.current_fn_returns.clear();
        self.check_block(&decl.body);
        if !self.block_diverges(&decl.body) {
            self.current_fn_returns.push((Ty::Nil, decl.span.clone()));
        }
        let inner_expected_ret = decl
            .return_type
            .as_ref()
            .map(|te| self.resolve_type_expr(te, &decl.span));
        let inferred_ret = self.merge_return_types(&decl.span, inner_expected_ret.as_ref());
        let effective_inferred_ret = if decl.is_async {
            Ty::Task(Box::new(inferred_ret.clone()))
        } else {
            inferred_ret.clone()
        };
        self.pop_type_param_scope();
        self.pop_scope();
        let final_ret = declared_ret.unwrap_or(effective_inferred_ret);
        let purity = self
            .functions
            .get(&decl.name)
            .map(|sig| {
                if sig.is_pure {
                    FnPurity::Pure
                } else if sig.effects.is_readonly() {
                    FnPurity::Readonly
                } else {
                    FnPurity::Impure
                }
            })
            .unwrap_or(FnPurity::Impure);
        if let Some(sig) = self.functions.get_mut(&decl.name) {
            sig.params = param_tys.clone();
            sig.ret = final_ret.clone();
        }
        if let Some(global_scope) = self.scopes.first_mut() {
            if let Some(binding) = global_scope.bindings.get_mut(&decl.name) {
                binding.ty = Ty::Fn {
                    params: param_tys,
                    ret: Box::new(final_ret),
                    purity,
                };
            }
        }
        self.current_fn_name = None;
        self.current_fn_returns.clear();
    }

    pub(super) fn check_stmt(&mut self, stmt: &Stmt) {
        self.check_causal_statement_boundary(stmt);
        match stmt {
            Stmt::Let(s) => self.check_let(s),
            Stmt::LetElse(le) => self.check_let_else(le),
            Stmt::Assign(s) => self.check_assign(s),
            Stmt::If(s) => self.check_if(s),
            Stmt::While(s) => self.check_while(s),
            Stmt::For(s) => self.check_for(s),
            Stmt::Return(s) => self.check_return(s),
            Stmt::Break(s) => self.check_break(s),
            Stmt::Continue(s) => self.check_continue(s),
            Stmt::Emit(s) => self.check_emit(s),
            Stmt::Schedule(s) => self.check_schedule(s),
            Stmt::Update(s) => self.check_update(s),
            Stmt::Settle(s) => self.check_settle_stmt(s),
            Stmt::Propose(s) => self.check_propose_stmt(s),
            Stmt::Next(s) => self.check_next_stmt(s),
            Stmt::Require(s) => self.check_require_stmt(s),
            Stmt::Match(s) => {
                self.check_match_stmt(s);
            }
            Stmt::Expr(s) => {
                self.check_expr(&s.expr);
                self.error_if_ignored_transform_result(&s.expr, &s.span);
            }
            Stmt::OnceGuardPass(_) | Stmt::Error(_) => {}
        }
    }

    fn error_if_ignored_transform_result(&mut self, expr: &Expr, span: &Span) {
        if let Some(name) = ignored_immutable_transform_name(expr) {
            let hint = match name {
                "push" => {
                    Some("`push` returns a new list; rebind it: `xs = push(xs, value)`".to_string())
                }
                "pop" => Some(
                    "`pop` returns the last element; use `drop_last(xs)` to get the remaining list"
                        .to_string(),
                ),
                "sort" => {
                    Some("`sort` returns a sorted copy; rebind it: `xs = sort(xs)`".to_string())
                }
                "reverse" => Some(
                    "`reverse` returns a reversed copy; rebind it: `xs = reverse(xs)`".to_string(),
                ),
                "slice" => Some("`slice` returns a new list; assign it to a variable".to_string()),
                "map" => Some("`map` returns a new list; assign/rebind the result".to_string()),
                "filter" => {
                    Some("`filter` returns a new list; assign/rebind the result".to_string())
                }
                _ => None,
            };
            self.error(
                span,
                format!(
                    "Ignored result from '{}' call; this statement has no effect unless you use the returned value",
                    name
                ),
                hint,
            );
        }
    }

    fn resolve_destructure_element_types(
        &mut self,
        base_ty: &Ty,
        num_bindings: usize,
        span: &Span,
    ) -> Option<Vec<Ty>> {
        match base_ty {
            Ty::Tuple(inner) if inner.len() == num_bindings => Some(inner.clone()),
            Ty::Tuple(inner) => {
                self.error(
                    span,
                    format!(
                        "tuple value has {} elements but {} bindings",
                        inner.len(),
                        num_bindings
                    ),
                    None,
                );
                None
            }
            Ty::List(elt) => Some(vec![*elt.clone(); num_bindings]),
            Ty::Any => Some(vec![Ty::Any; num_bindings]),
            other => {
                self.error(
                    span,
                    format!(
                        "Cannot destructure into {} bindings: expected tuple or list, got {}",
                        num_bindings, other
                    ),
                    Some(
                        "Use a tuple `(a, b)` or list `[a, b]` on the right-hand side".to_string(),
                    ),
                );
                None
            }
        }
    }

    fn check_let(&mut self, stmt: &LetStmt) {
        if stmt.is_unique && (stmt.tuple_destructure || stmt.names.len() != 1) {
            self.error(
                &stmt.span,
                "`let unique` requires a single binding name (tuple destructuring is not allowed)"
                    .to_string(),
                Some("Use `let unique name = ...`".to_string()),
            );
        }
        if stmt.recursive {
            if stmt.tuple_destructure || stmt.names.len() != 1 {
                self.error(
                    &stmt.span,
                    "`let rec` requires a single variable name (tuple destructuring is not allowed)"
                        .to_string(),
                    None,
                );
            }
            if !matches!(stmt.value, Expr::FnExpr(..)) {
                self.error(
                    &stmt.span,
                    "`let rec` requires a function expression (`fn(...) { ... }`) as its value"
                        .to_string(),
                    Some(
                        "Only closures can be recursive bindings; use a plain `let` for other values"
                            .to_string(),
                    ),
                );
            }
            self.define(
                &stmt.names[0],
                Ty::Any,
                stmt.mutable,
                stmt.span.clone(),
                false,
                true,
            );
            if stmt.is_unique {
                self.mark_binding_unique(&stmt.names[0]);
            }
        }

        // An explicit `list<any>` / `map<K, any>` annotation is the user
        // accepting mixed element types — exactly what the mixed-type
        // warnings' own hints recommend, so the annotation must actually
        // silence them (dogfood bug seq 58-6d: the map half was missing and
        // the hint was a no-op).
        let suppress_mixed_list_warning = stmt
            .type_annotation
            .as_ref()
            .map(|ann| self.type_expr_or_any(ann))
            .is_some_and(|ty| {
                matches!(&ty, Ty::List(inner) if **inner == Ty::Any)
                    || matches!(&ty, Ty::Map(_, val) if **val == Ty::Any)
            });
        let inferred = if suppress_mixed_list_warning {
            self.with_mixed_list_warning_suppressed(|checker| checker.check_expr(&stmt.value))
        } else {
            self.check_expr(&stmt.value)
        };

        if !stmt.tuple_destructure {
            if let Expr::Ident(source_name, _) = &stmt.value {
                if self.binding_is_unique(source_name) && source_name != &stmt.names[0] {
                    self.error(
                        &stmt.span,
                        format!(
                            "Cannot alias unique binding '{}' into '{}'",
                            source_name, stmt.names[0]
                        ),
                        Some(
                            "Move ownership instead (reassign the same name) or remove `unique`"
                                .to_string(),
                        ),
                    );
                }
            }
            if self.options.strict_types && stmt.type_annotation.is_none() {
                self.error(
                    &stmt.span,
                    format!(
                        "Strict types: variable '{}' requires an explicit type annotation",
                        stmt.names[0]
                    ),
                    Some("Write it as `let name: Type = value`".to_string()),
                );
            }
            let binding_ty = if let Some(ann) = &stmt.type_annotation {
                let declared = self.resolve_type_expr(ann, &stmt.span);
                if declared != Ty::Any
                    && inferred != Ty::Any
                    && !declared.assignable_from(&inferred)
                {
                    self.error(
                        &stmt.span,
                        format!(
                            "Type mismatch: '{}' declared as {}, got {}",
                            stmt.names[0], declared, inferred
                        ),
                        self.type_mismatch_hint(&declared, &inferred),
                    );
                }
                declared
            } else {
                inferred
            };
            // pub lets are module exports: importers read them, so local
            // unused-tracking would only produce false positives.
            self.define(
                &stmt.names[0],
                binding_ty,
                stmt.mutable,
                stmt.span.clone(),
                stmt.is_pub,
                !stmt.is_pub,
            );
            if stmt.is_unique {
                self.mark_binding_unique(&stmt.names[0]);
            }
            return;
        }

        // Tuple / multi-binding `let (a, b, ...) = ...`
        if self.options.strict_types && stmt.type_annotation.is_none() {
            self.error(
                &stmt.span,
                "Strict types: tuple destructuring requires an explicit type annotation (e.g. `let (a, b): (int, int) = ...`)"
                    .to_string(),
                None,
            );
        }

        let Some(elem_tys) =
            self.resolve_destructure_element_types(&inferred, stmt.names.len(), &stmt.span)
        else {
            return;
        };

        if let Some(ann) = &stmt.type_annotation {
            let declared = self.resolve_type_expr(ann, &stmt.span);
            match &declared {
                Ty::Tuple(parts) if parts.len() == stmt.names.len() => {
                    for (i, (pt, et)) in parts.iter().zip(elem_tys.iter()).enumerate() {
                        if *pt != Ty::Any && *et != Ty::Any && !pt.assignable_from(et) {
                            self.error(
                                &stmt.span,
                                format!(
                                    "Type mismatch for binding '{}': declared {}, inferred {}",
                                    stmt.names[i], pt, et
                                ),
                                self.type_mismatch_hint(pt, et),
                            );
                        }
                    }
                    for (name, pt) in stmt.names.iter().zip(parts.iter()) {
                        self.define(
                            name,
                            pt.clone(),
                            stmt.mutable,
                            stmt.span.clone(),
                            false,
                            true,
                        );
                    }
                }
                _ => {
                    if declared != Ty::Any
                        && inferred != Ty::Any
                        && !declared.assignable_from(&inferred)
                    {
                        self.error(
                            &stmt.span,
                            format!("Type mismatch: declared {}, got {}", declared, inferred),
                            self.type_mismatch_hint(&declared, &inferred),
                        );
                    }
                    for (name, et) in stmt.names.iter().zip(elem_tys.iter()) {
                        self.define(
                            name,
                            et.clone(),
                            stmt.mutable,
                            stmt.span.clone(),
                            false,
                            true,
                        );
                    }
                }
            }
        } else {
            for (name, et) in stmt.names.iter().zip(elem_tys.iter()) {
                self.define(
                    name,
                    et.clone(),
                    stmt.mutable,
                    stmt.span.clone(),
                    false,
                    true,
                );
            }
        }
    }

    fn check_let_else(&mut self, le: &LetElseStmt) {
        if le.variant_name != "Some" && le.variant_name != "Ok" {
            self.error(
                &le.span,
                "let ... else only supports `Some { ... }` (Option) or `Ok { ... }` (Result) patterns"
                    .to_string(),
                Some(
                    "Use `let Some { value: x } = expr else { ... }` or `let Ok { value: x } = expr else { ... }`"
                        .to_string(),
                ),
            );
            return;
        }

        let Some(primary) = le.primary_binding_name() else {
            self.error(
                &le.span,
                "let ... else requires exactly one pattern binding (for example `Some { value: hp }`)"
                    .to_string(),
                None,
            );
            return;
        };

        let subject_ty = self.check_expr(&le.subject);

        let type_ok = matches!(&subject_ty, Ty::Any)
            || matches!(
                &subject_ty,
                Ty::SumType(n) | Ty::App(n, _) if n == "Option" || n == "Result"
            );
        if !type_ok {
            self.error(
                &le.span,
                format!(
                    "let ... else subject must be Option or Result, got {}",
                    subject_ty
                ),
                None,
            );
            return;
        }

        let effective_subject_ty = match &subject_ty {
            Ty::Any => {
                if le.variant_name == "Some" {
                    Ty::App("Option".to_string(), vec![Ty::Any])
                } else {
                    Ty::App("Result".to_string(), vec![Ty::Any, Ty::Any])
                }
            }
            other => other.clone(),
        };

        let synthetic_case = MatchCase {
            id: le.id,
            span: le.span.clone(),
            pattern: Pattern::Variant {
                path: vec![le.variant_name.clone()],
                bindings: le.bindings.clone(),
                pattern_bindings: le.pattern_bindings.clone(),
                has_rest: le.has_rest,
                is_bare_variant: false,
            },
            guard: None,
            body: Block {
                id: le.id,
                span: le.span.clone(),
                stmts: vec![],
            },
        };

        let case_bindings = self.case_pattern_bindings(&synthetic_case);
        let type_name = match &effective_subject_ty {
            Ty::SumType(n) | Ty::App(n, _) => n.clone(),
            _ => {
                self.error(
                    &le.span,
                    "Internal error: expected sum type for let ... else".to_string(),
                    None,
                );
                return;
            }
        };
        let Some(sum_type) = self.sum_types.get(&type_name).cloned() else {
            return;
        };

        let Some(variant) = sum_type.variants.iter().find(|v| v.name == le.variant_name) else {
            self.error(
                &le.span,
                format!(
                    "Variant '{}' not found in type '{}'",
                    le.variant_name, sum_type.name
                ),
                None,
            );
            return;
        };

        let binding = &case_bindings[0];
        let field_ty = self.resolve_case_binding_ty(
            &sum_type,
            &effective_subject_ty,
            variant,
            binding,
            &le.span,
        );
        let inner_ty = field_ty.unwrap_or(Ty::Any);
        let inner_is_any = inner_ty == Ty::Any;

        let binding_ty = if let Some(ann) = &le.type_annotation {
            let declared = self.resolve_type_expr(ann, &le.span);
            if declared != Ty::Any && inner_ty != Ty::Any && !declared.assignable_from(&inner_ty) {
                self.error(
                    &le.span,
                    format!(
                        "Type mismatch: '{}' declared as {}, pattern yields {}",
                        primary, declared, inner_ty
                    ),
                    self.type_mismatch_hint(&declared, &inner_ty),
                );
            }
            declared
        } else {
            inner_ty.clone()
        };

        if self.options.strict_types && le.type_annotation.is_none() && inner_is_any {
            self.error(
                &le.span,
                format!(
                    "Strict types: variable '{}' needs a type annotation when the pattern type cannot be inferred",
                    primary
                ),
                Some("Write it as `let Some { value: x }: Type = value`".to_string()),
            );
        }

        let else_ty = self.check_block(&le.else_block);
        let diverges = self.block_diverges(&le.else_block);

        if !diverges
            && else_ty != Ty::Any
            && binding_ty != Ty::Any
            && self.subst.unify(&binding_ty, &else_ty).is_err()
            && !binding_ty.assignable_from(&else_ty)
        {
            let resolved_binding = self.resolve_ty(&binding_ty);
            self.error(
                &le.span,
                format!(
                    "let ... else block must either diverge (return/break/continue) or evaluate to a value compatible with the binding type {}. Got {}",
                    resolved_binding, else_ty
                ),
                Some("Add a return statement, or end the block with a default value".to_string()),
            );
        }

        self.define(
            &primary,
            binding_ty,
            le.mutable,
            le.span.clone(),
            false,
            true,
        );
    }

    fn check_assign(&mut self, stmt: &AssignStmt) {
        // Same accepted-mixed contract as check_let: a target already typed
        // `list<any>` / `map<K, any>` re-assigned with a mixed literal must
        // not re-warn.
        let suppress_mixed_list_warning = match &stmt.target {
            Expr::Ident(name, _) => self.lookup(name).is_some_and(|binding| {
                matches!(&binding.ty, Ty::List(inner) if **inner == Ty::Any)
                    || matches!(&binding.ty, Ty::Map(_, val) if **val == Ty::Any)
            }),
            _ => false,
        };
        let assign_target_name = match &stmt.target {
            Expr::Ident(name, _) => Some(name.clone()),
            _ => None,
        };
        let prev_assign_target = self.current_assign_target.clone();
        self.current_assign_target = assign_target_name;
        let val_ty = if suppress_mixed_list_warning {
            self.with_mixed_list_warning_suppressed(|checker| checker.check_expr(&stmt.value))
        } else {
            self.check_expr(&stmt.value)
        };
        self.current_assign_target = prev_assign_target;
        let in_pipeline = self.scopes.iter().rev().any(|s| s.in_pipeline);

        match &stmt.target {
            Expr::Ident(name, span) => {
                if in_pipeline {
                    let defined_in_pipeline = self.binding_defined_in_current_pipeline(name);
                    if !defined_in_pipeline {
                        self.error(
                            span,
                            format!(
                                "Cannot assign to outer variable '{}' inside a pipeline",
                                name
                            ),
                            Some(
                                "Pipeline functions must be pure — no outer mutations".to_string(),
                            ),
                        );
                    }
                }
                match self.lookup(name) {
                    Some(binding) => {
                        self.mark_var_read(name);
                        if !binding.mutable {
                            let (msg, hint) = if self.is_shadow_of_outer_mutable(name) {
                                (
                                    format!(
                                        "Cannot assign to '{}' — it refers to an immutable pattern binding that shadows your outer 'let mut {}'",
                                        name, name
                                    ),
                                    Some(format!(
                                        "Rename the outer variable or the pattern field to avoid the name collision with '{}'",
                                        name
                                    )),
                                )
                            } else {
                                (
                                    format!("Cannot assign to immutable variable '{}'", name),
                                    Some(format!(
                                        "Change the original binding to `let mut {}` (or avoid reassignment)",
                                        name
                                    )),
                                )
                            };
                            self.error(span, msg, hint);
                        }
                        if !binding.ty.assignable_from(&val_ty) && binding.ty != Ty::Any {
                            self.error(
                                span,
                                format!(
                                    "Cannot assign {} to variable '{}' of type {}",
                                    val_ty, name, binding.ty
                                ),
                                self.type_mismatch_hint(&binding.ty, &val_ty),
                            );
                        }
                    }
                    None => {
                        let cands = self.scope_binding_hint_names();
                        let refs: Vec<&str> = cands.iter().map(|s| s.as_str()).collect();
                        let hint = suggest_did_you_mean(name, &refs);
                        self.error(span, format!("Undefined variable '{}'", name), hint);
                    }
                }
            }
            Expr::Field(obj, field_name, span) => {
                if let Expr::Ident(name, ident_span) = obj.as_ref() {
                    if let Some(binding) = self.lookup(name) {
                        if !binding.mutable {
                            self.error(
                                ident_span,
                                format!("Cannot mutate field '{}' through immutable binding '{}'", field_name, name),
                                Some(format!("Declare with 'let mut {} = ...' or mark the system parameter as mut", name)),
                            );
                        }
                    }
                }

                let obj_ty = self.check_expr(obj);
                match obj_ty {
                    Ty::Component(comp_type_name) => {
                        if let Some(comp) = self.components.get(&comp_type_name).cloned() {
                            if let Some(expected_ty) = comp.field_type(field_name) {
                                if !expected_ty.assignable_from(&val_ty) && val_ty != Ty::Any {
                                    self.error(
                                        span,
                                        format!(
                                            "Type error in '{}.{}': expected {}, got {}",
                                            comp_type_name, field_name, expected_ty, val_ty
                                        ),
                                        self.type_mismatch_hint(expected_ty, &val_ty),
                                    );
                                }
                            } else {
                                self.error(
                                    span,
                                    format!(
                                        "No field '{}' on component '{}'",
                                        field_name, comp_type_name
                                    ),
                                    Some(format!(
                                        "Available fields: {}",
                                        comp.fields
                                            .iter()
                                            .map(|(n, _)| n.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    )),
                                );
                            }
                        }
                    }
                    Ty::Struct(struct_type_name) => {
                        if let Some(st) = self.structs.get(&struct_type_name).cloned() {
                            if let Some(expected_ty) = st.field_type(field_name) {
                                if !expected_ty.assignable_from(&val_ty) && val_ty != Ty::Any {
                                    self.error(
                                        span,
                                        format!(
                                            "Type error in '{}.{}': expected {}, got {}",
                                            struct_type_name, field_name, expected_ty, val_ty
                                        ),
                                        self.type_mismatch_hint(expected_ty, &val_ty),
                                    );
                                }
                            } else {
                                self.error(
                                    span,
                                    format!(
                                        "No field '{}' on struct '{}'",
                                        field_name, struct_type_name
                                    ),
                                    Some(format!(
                                        "Available fields: {}",
                                        st.fields
                                            .iter()
                                            .map(|(n, _)| n.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    )),
                                );
                            }
                        }
                    }
                    Ty::Event(event_name) => {
                        if let Some(evt) = self.events.get(&event_name).cloned() {
                            if let Some(expected_ty) = evt.field_type(field_name) {
                                if !expected_ty.assignable_from(&val_ty) && val_ty != Ty::Any {
                                    self.error(
                                        span,
                                        format!(
                                            "Type error in '{}.{}': expected {}, got {}",
                                            event_name, field_name, expected_ty, val_ty
                                        ),
                                        self.type_mismatch_hint(expected_ty, &val_ty),
                                    );
                                }
                            } else {
                                self.error(
                                    span,
                                    format!("No field '{}' on event '{}'", field_name, event_name),
                                    Some(format!(
                                        "Available fields: {}",
                                        evt.fields
                                            .iter()
                                            .map(|(n, _)| n.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    )),
                                );
                            }
                        }
                    }
                    Ty::Map(_, value_ty) => {
                        if !value_ty.assignable_from(&val_ty) && val_ty != Ty::Any {
                            self.error(
                                span,
                                format!(
                                    "Type error in map field assignment: expected {}, got {}",
                                    value_ty, val_ty
                                ),
                                None,
                            );
                        }
                    }
                    Ty::Any => {}
                    other => {
                        self.error(
                            span,
                            format!(
                                "Cannot assign field '{}' on value of type {}",
                                field_name, other
                            ),
                            None,
                        );
                    }
                }
            }
            Expr::Index(obj, idx, span) => {
                let obj_ty = self.check_expr(obj);
                let idx_ty = self.check_expr(idx);
                match obj_ty {
                    Ty::List(inner_ty) => {
                        if idx_ty != Ty::Int && idx_ty != Ty::Any {
                            self.error(
                                idx.span(),
                                format!("List index must be int, got {}", idx_ty),
                                None,
                            );
                        }
                        if !inner_ty.assignable_from(&val_ty) && val_ty != Ty::Any {
                            self.error(
                                span,
                                format!(
                                    "Type error in list assignment: expected {}, got {}",
                                    inner_ty, val_ty
                                ),
                                None,
                            );
                        }
                    }
                    Ty::Map(key_ty, value_ty) => {
                        if !key_ty.assignable_from(&idx_ty) && idx_ty != Ty::Any {
                            self.error(
                                idx.span(),
                                format!("Map key type is {}, got {}", key_ty, idx_ty),
                                None,
                            );
                        }
                        if !value_ty.assignable_from(&val_ty) && val_ty != Ty::Any {
                            self.error(
                                span,
                                format!(
                                    "Type error in map assignment: expected {}, got {}",
                                    value_ty, val_ty
                                ),
                                self.type_mismatch_hint(&value_ty, &val_ty),
                            );
                        }
                    }
                    Ty::Any => {}
                    Ty::Str => {
                        self.error(span, "Cannot assign through index on str".to_string(), None);
                    }
                    other => {
                        self.error(
                            span,
                            format!("Cannot assign through index on {}", other),
                            None,
                        );
                    }
                }
            }
            other => {
                self.error(other.span(), "Invalid assignment target".to_string(), None);
            }
        }
    }}