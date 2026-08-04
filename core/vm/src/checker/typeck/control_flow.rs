impl Checker {

    fn check_if(&mut self, stmt: &IfStmt) {
        self.check_expr(&stmt.condition);

        if let Some(val) = Self::eval_const_bool(&stmt.condition) {
            self.warning(
                stmt.condition.span(),
                format!("Condition is always `{}`", val),
                Some("Remove this condition or use a dynamic expression".to_string()),
            );
        }

        let (then_narrows, else_narrows) = self.resolve_condition_narrowings(&stmt.condition);

        self.push_scope();
        for (name, ty) in &then_narrows {
            if let Some(binding) = self.lookup(name) {
                self.define(
                    name,
                    ty.clone(),
                    binding.mutable,
                    binding.defined_at.clone(),
                    false,
                    false,
                );
            }
        }
        self.check_block(&stmt.then_block);
        self.pop_scope();

        if let Some(else_block) = &stmt.else_block {
            self.push_scope();
            for (name, ty) in &else_narrows {
                if let Some(binding) = self.lookup(name) {
                    self.define(
                        name,
                        ty.clone(),
                        binding.mutable,
                        binding.defined_at.clone(),
                        false,
                        false,
                    );
                }
            }
            self.check_block(else_block);
            self.pop_scope();
        }

        // Guard-clause narrowing: `if x == nil { return }` means everything
        // AFTER the if sees x as non-nil — the early exit IS the else branch.
        if stmt.else_block.is_none() && Self::block_always_exits(&stmt.then_block) {
            for (name, ty) in &else_narrows {
                if let Some(binding) = self.lookup(name) {
                    self.define(
                        name,
                        ty.clone(),
                        binding.mutable,
                        binding.defined_at.clone(),
                        false,
                        false,
                    );
                }
            }
        }
    }

    /// True when every path through the block leaves the enclosing scope:
    /// return / break / continue, directly or through an exhaustive
    /// if/else whose arms all exit. Conservative — false means "unknown".
    fn block_always_exits(block: &Block) -> bool {
        block.stmts.iter().any(|stmt| match stmt {
            Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::If(i) => {
                i.else_block.as_ref().is_some_and(Self::block_always_exits)
                    && Self::block_always_exits(&i.then_block)
            }
            _ => false,
        })
    }

    #[allow(clippy::type_complexity)]
    fn resolve_condition_narrowings(
        &self,
        condition: &Expr,
    ) -> (Vec<(String, Ty)>, Vec<(String, Ty)>) {
        let mut then_narrows = Vec::new();
        let mut else_narrows = Vec::new();

        match condition {
            // `x != nil` -> then: x is non-nil (keep original type), else: x is nil
            Expr::Binary(left, BinOp::Ne, right, _) => {
                if let (Expr::Ident(name, _), Expr::NilLit(_)) = (left.as_ref(), right.as_ref()) {
                    if let Some(binding) = self.lookup(name) {
                        then_narrows.push((name.clone(), Self::ty_without_nil(&binding.ty)));
                        else_narrows.push((name.clone(), Ty::Nil));
                    }
                }
                if let (Expr::NilLit(_), Expr::Ident(name, _)) = (left.as_ref(), right.as_ref()) {
                    if let Some(binding) = self.lookup(name) {
                        then_narrows.push((name.clone(), Self::ty_without_nil(&binding.ty)));
                        else_narrows.push((name.clone(), Ty::Nil));
                    }
                }
            }
            // `x == nil` -> then: x is nil, else: x is non-nil
            Expr::Binary(left, BinOp::Eq, right, _) => {
                if let (Expr::Ident(name, _), Expr::NilLit(_)) = (left.as_ref(), right.as_ref()) {
                    if let Some(binding) = self.lookup(name) {
                        then_narrows.push((name.clone(), Ty::Nil));
                        else_narrows.push((name.clone(), Self::ty_without_nil(&binding.ty)));
                    }
                }
                if let (Expr::NilLit(_), Expr::Ident(name, _)) = (left.as_ref(), right.as_ref()) {
                    if let Some(binding) = self.lookup(name) {
                        then_narrows.push((name.clone(), Ty::Nil));
                        else_narrows.push((name.clone(), Self::ty_without_nil(&binding.ty)));
                    }
                }
            }
            // `!x` negation: swap then/else narrowings of inner
            Expr::Unary(UnaryOp::Not, inner, _) => {
                let (inner_then, inner_else) = self.resolve_condition_narrowings(inner);
                then_narrows.extend(inner_else);
                else_narrows.extend(inner_then);
            }
            _ => {}
        }

        (then_narrows, else_narrows)
    }

    /// Strip `nil` from **union** types so `x != nil` / `x == nil` branches refine
    /// optional representations like `entity | nil`.
    ///
    /// This does **not** turn `any` into a concrete type, and does not interpret
    /// sentinel patterns (e.g. `0` meaning "absent"). Those need separate rules or
    /// explicit annotations.
    fn ty_without_nil(ty: &Ty) -> Ty {
        match ty {
            Ty::Union(variants) => {
                let filtered: Vec<Ty> = variants
                    .iter()
                    .filter(|v| **v != Ty::Nil)
                    .cloned()
                    .collect();
                match filtered.len() {
                    0 => Ty::Nil,
                    1 => filtered[0].clone(),
                    _ => Ty::Union(filtered),
                }
            }
            _ => ty.clone(),
        }
    }

    fn check_while(&mut self, stmt: &WhileStmt) {
        self.check_expr(&stmt.condition);

        if let Some(val) = Self::eval_const_bool(&stmt.condition) {
            self.warning(
                stmt.condition.span(),
                format!("Condition is always `{}`", val),
                Some("Remove this condition or use a dynamic expression".to_string()),
            );
        }

        self.push_scope();
        if let Some(scope) = self.scopes.last_mut() {
            scope.in_loop = true;
            scope.loop_target_settlement_depth = Some(scope.settlement_depth);
        }
        self.check_block(&stmt.body);
        self.pop_scope();
    }

    fn check_for(&mut self, stmt: &ForStmt) {
        if let Expr::QueryExpr(q, span) = &stmt.iterable {
            let has_mut = q.components.iter().any(|(_, is_mut)| *is_mut);
            let is_special_loop = has_mut || stmt.bindings.len() > 1;
            if is_special_loop {
                if !q.select.is_empty() {
                    self.error(
                        span,
                        "Component unpack queries cannot use `select` clauses".to_string(),
                        None,
                    );
                }

                for (comp, _) in &q.components {
                    if let Some(ct) = self.components.get(comp) {
                        if !ct.is_pub && is_cross_file(ct.file_id, span.file) {
                            self.error(
                                span,
                                format!("Component '{}' is private", comp),
                                Some(format!("Add `pub` to the declaration of '{}'", comp)),
                            );
                        }
                    } else {
                        self.error(span, format!("Unknown component '{}' in query", comp), None);
                    }
                }

                self.push_scope();
                if let Some(scope) = self.scopes.last_mut() {
                    scope.in_loop = true;
                    scope.loop_target_settlement_depth = Some(scope.settlement_depth);
                }

                if let Some(filter) = &q.filter {
                    self.push_scope();
                    for (comp, _is_mut) in &q.components {
                        self.define(
                            comp,
                            Ty::Component(comp.clone()),
                            false,
                            span.clone(),
                            false,
                            false,
                        );
                    }
                    self.define("__entity", Ty::EntityId, false, span.clone(), false, false);
                    self.check_expr(filter);
                    self.pop_scope();
                }

                let num_comps = q.components.len();
                if stmt.bindings.len() == num_comps {
                    for (i, (comp, is_mut)) in q.components.iter().enumerate() {
                        if !self.components.contains_key(comp) {
                            self.error(
                                &stmt.span,
                                format!("Unknown component '{}' in query unpack", comp),
                                None,
                            );
                        }
                        self.define(
                            &stmt.bindings[i],
                            Ty::Component(comp.clone()),
                            *is_mut,
                            stmt.span.clone(),
                            false,
                            true,
                        );
                    }
                } else if stmt.bindings.len() == num_comps + 1 {
                    self.define(
                        &stmt.bindings[0],
                        Ty::EntityId,
                        false,
                        stmt.span.clone(),
                        false,
                        true,
                    );
                    for (i, (comp, is_mut)) in q.components.iter().enumerate() {
                        if !self.components.contains_key(comp) {
                            self.error(
                                &stmt.span,
                                format!("Unknown component '{}' in query unpack", comp),
                                None,
                            );
                        }
                        self.define(
                            &stmt.bindings[i + 1],
                            Ty::Component(comp.clone()),
                            *is_mut,
                            stmt.span.clone(),
                            false,
                            true,
                        );
                    }
                } else {
                    self.error(
                        &stmt.span,
                        format!(
                            "Query unpack with {} components requires either {} or {} bindings, got {}",
                            num_comps, num_comps, num_comps + 1, stmt.bindings.len()
                        ),
                        None,
                    );
                }

                self.check_block(&stmt.body);
                self.pop_scope();
                return;
            }
        }

        let iter_ty = self.check_expr(&stmt.iterable);
        let (key_ty, val_ty, iter_kind) = match iter_ty {
            Ty::List(inner) => (Ty::Int, *inner, ForIterKind::List),
            Ty::Str => (Ty::Int, Ty::Int, ForIterKind::Str),
            Ty::Map(k_ty, v_ty) => (*k_ty, *v_ty, ForIterKind::Map),
            Ty::Any => (Ty::Any, Ty::Any, ForIterKind::Unknown),
            other => {
                self.error(
                    &stmt.span,
                    format!("Cannot iterate over type {}", other),
                    None,
                );
                (Ty::Any, Ty::Any, ForIterKind::Unknown)
            }
        };
        self.for_iter_kinds.insert(stmt.id, iter_kind);
        self.push_scope();
        if let Some(scope) = self.scopes.last_mut() {
            scope.in_loop = true;
            scope.loop_target_settlement_depth = Some(scope.settlement_depth);
        }

        if stmt.bindings.len() == 1 {
            // For list/str, single binding is the value. For map, it's the key.
            let ty = match iter_kind {
                ForIterKind::Map => key_ty.clone(),
                _ => val_ty.clone(),
            };
            self.define(
                &stmt.bindings[0],
                ty.clone(),
                false,
                stmt.span.clone(),
                false,
                true,
            );
            if let Some(names) = &stmt.destructure_bindings {
                let source_ty = ty;
                if let Some(elem_tys) =
                    self.resolve_destructure_element_types(&source_ty, names.len(), &stmt.span)
                {
                    for (name, elem_ty) in names.iter().zip(elem_tys) {
                        self.define(name, elem_ty, false, stmt.span.clone(), false, true);
                    }
                } else {
                    for name in names {
                        self.define(name, Ty::Any, false, stmt.span.clone(), false, true);
                    }
                }
            }
        } else if stmt.bindings.len() >= 2 && matches!(iter_kind, ForIterKind::List) {
            // tuple destructure over a list: `for (due, who, x, z) in rows`
            // — same semantics as the bracket form, parens for symmetry
            // with `let (a, b) = t`
            if let Some(elem_tys) =
                self.resolve_destructure_element_types(&val_ty, stmt.bindings.len(), &stmt.span)
            {
                for (name, elem_ty) in stmt.bindings.iter().zip(elem_tys) {
                    self.define(name, elem_ty, false, stmt.span.clone(), false, true);
                }
            } else {
                for name in &stmt.bindings {
                    self.define(name, Ty::Any, false, stmt.span.clone(), false, true);
                }
            }
        } else if stmt.bindings.len() == 2 {
            if stmt.destructure_bindings.is_some() {
                self.error(
                    &stmt.span,
                    "For-loop destructuring cannot be combined with two-variable map iteration"
                        .to_string(),
                    None,
                );
            }
            if !matches!(iter_kind, ForIterKind::Map | ForIterKind::Unknown) {
                self.error(
                    &stmt.span,
                    "Two-variable for loop is only supported for maps (or tuple lists)".to_string(),
                    None,
                );
            }
            self.define(
                &stmt.bindings[0],
                key_ty,
                false,
                stmt.span.clone(),
                false,
                true,
            );
            self.define(
                &stmt.bindings[1],
                val_ty,
                false,
                stmt.span.clone(),
                false,
                true,
            );
        } else {
            self.error(
                &stmt.span,
                format!(
                    "Expected 1 or 2 loop variables, got {}",
                    stmt.bindings.len()
                ),
                None,
            );
        }

        self.check_block(&stmt.body);
        self.pop_scope();
    }

    fn check_return(&mut self, stmt: &ReturnStmt) {
        let ret_ty = if let Some(val) = &stmt.value {
            self.check_expr(val)
        } else {
            Ty::Nil
        };
        if self.current_fn_name.is_some() {
            self.current_fn_returns.push((ret_ty, stmt.span.clone()));
        }
    }

    fn check_break(&mut self, stmt: &BreakStmt) {
        let scope_floor = self.anon_fn_scope_bases.last().copied().unwrap_or(0);
        let target_depth = self.scopes[scope_floor..]
            .iter()
            .rev()
            .find_map(|scope| scope.loop_target_settlement_depth);
        let Some(target_depth) = target_depth else {
            self.error(
                &stmt.span,
                "'break' used outside of a loop".to_string(),
                None,
            );
            return;
        };
        let current_depth = self.scopes.last().unwrap().settlement_depth;
        if target_depth != current_depth {
            self.error(
                &stmt.span,
                "`break` cannot cross a settlement boundary".to_string(),
                Some(
                    "Move the loop inside `settle`, or move the control flow outside it"
                        .to_string(),
                ),
            );
        }
    }

    fn check_continue(&mut self, stmt: &ContinueStmt) {
        let scope_floor = self.anon_fn_scope_bases.last().copied().unwrap_or(0);
        let target_depth = self.scopes[scope_floor..]
            .iter()
            .rev()
            .find_map(|scope| scope.loop_target_settlement_depth);
        let Some(target_depth) = target_depth else {
            self.error(
                &stmt.span,
                "'continue' used outside of a loop".to_string(),
                None,
            );
            return;
        };
        let current_depth = self.scopes.last().unwrap().settlement_depth;
        if target_depth != current_depth {
            self.error(
                &stmt.span,
                "`continue` cannot cross a settlement boundary".to_string(),
                Some(
                    "Move the loop inside `settle`, or move the control flow outside it"
                        .to_string(),
                ),
            );
        }
    }

    fn check_emit(&mut self, stmt: &EmitStmt) {
        let in_pipeline = self.scopes.iter().rev().any(|s| s.in_pipeline);
        if in_pipeline {
            self.error(
                &stmt.span,
                "Cannot emit events inside a pipeline function".to_string(),
                Some(
                    "Pipeline functions must be pure — use an event handler for side effects"
                        .to_string(),
                ),
            );
        }

        let resolved_event = self.resolve_canonical_name(&stmt.event_name);

        if let Some(event_type) = self.events.get(&resolved_event).cloned() {
            if !event_type.is_pub && is_cross_file(event_type.file_id, stmt.span.file) {
                self.error(
                    &stmt.span,
                    format!("Event '{}' is private", stmt.event_name),
                    Some(format!(
                        "Add `pub` to the declaration of '{}'",
                        stmt.event_name
                    )),
                );
            }
            for (field_name, expr) in &stmt.fields {
                let expr_ty = self.check_expr(expr);
                if let Some(expected_ty) = event_type.field_type(field_name) {
                    if !expected_ty.assignable_from(&expr_ty) && expr_ty != Ty::Any {
                        self.error(
                            expr.span(),
                            format!(
                                "Type error in emit field '{}': expected {}, got {}",
                                field_name, expected_ty, expr_ty
                            ),
                            self.type_mismatch_hint(expected_ty, &expr_ty),
                        );
                    }
                } else {
                    self.error(
                        expr.span(),
                        format!(
                            "Unknown field '{}' for event '{}'",
                            field_name, stmt.event_name
                        ),
                        Some(format!(
                            "Expected fields: {}",
                            event_type
                                .fields
                                .iter()
                                .map(|(n, _)| n.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    );
                }
            }
            if let Some(delay) = &stmt.delay {
                let d_ty = self.check_expr(delay);
                if d_ty != Ty::Int && d_ty != Ty::Any {
                    self.error(
                        delay.span(),
                        format!("emit ... after expects an int tick count, got {}", d_ty),
                        None,
                    );
                }
            }
        } else {
            self.error(
                &stmt.span,
                format!("Emitting unknown event '{}'", stmt.event_name),
                None,
            );
        }
    }

    fn check_phase_decl(&mut self, decl: &PhaseDecl) {
        for name in &decl.systems {
            let resolved_name = self.resolve_canonical_name(name);
            if !self.systems.contains_key(&resolved_name) {
                self.error(
                    &decl.span,
                    format!("Phase '{}' references unknown system '{}'", decl.name, name),
                    None,
                );
            }
        }
    }

    fn check_update(&mut self, stmt: &UpdateStmt) {
        let resolved_name = self.resolve_canonical_name(&stmt.comp_name);
        if let Some(entity_expr) = &stmt.entity_expr {
            let entity_ty = self.check_expr(entity_expr);
            if entity_ty != Ty::EntityId && entity_ty != Ty::Any {
                self.error(
                    &stmt.span,
                    format!(
                        "update() first argument must be an entity, got {}",
                        entity_ty
                    ),
                    None,
                );
            }
            if self.resources.contains_key(&resolved_name) {
                self.error(
                    &stmt.span,
                    format!(
                        "update(entity, {}) is invalid because '{}' is a resource; use update({})",
                        stmt.comp_name, stmt.comp_name, stmt.comp_name
                    ),
                    None,
                );
            }
        } else if !self.resources.contains_key(&resolved_name) {
            self.error(
                &stmt.span,
                format!(
                    "update({}) is only valid for resources; use update(entity, Component) for components",
                    stmt.comp_name
                ),
                None,
            );
        }

        if stmt.entity_expr.is_none() {
            let in_system = self.scopes.iter().rev().any(|s| s.in_system.is_some());
            if in_system {
                let has_mut_param = self.system_params.values().any(|(comp_type, is_mut)| {
                    *is_mut && self.resolve_canonical_name(comp_type) == resolved_name
                });
                if has_mut_param {
                    self.error(
                        &stmt.span,
                        format!(
                            "update({}) conflicts with mutable system parameter of the same resource; mutate the parameter directly",
                            stmt.comp_name
                        ),
                        Some("The system writeback will overwrite this update. Use the `mut` parameter instead.".to_string()),
                    );
                }
            }
        }

        let fields = if let Some(comp) = self.components.get(&resolved_name).cloned() {
            Some(comp.fields)
        } else if let Some(res) = self.resources.get(&resolved_name).cloned() {
            Some(res.fields)
        } else {
            None
        };

        if let Some(fields) = fields {
            self.check_update_fields(&stmt.span, &stmt.comp_name, &stmt.field_updates, &fields);
        } else {
            self.error(
                &stmt.span,
                format!("update() references unknown component '{}'", stmt.comp_name),
                None,
            );
            for fu in &stmt.field_updates {
                if let Some(idx) = &fu.index {
                    self.check_expr(idx);
                }
                self.check_expr(&fu.value);
            }
        }
    }

    fn check_update_fields(
        &mut self,
        span: &Span,
        type_name: &str,
        field_updates: &[FieldUpdate],
        fields: &[(String, Ty)],
    ) {
        for fu in field_updates {
            let fname = &fu.name;
            let fexpr = &fu.value;
            let expr_ty = self.check_expr(fexpr);
            if let Some((_, expected_ty)) = fields.iter().find(|(n, _)| n == fname) {
                if let Some(idx) = &fu.index {
                    // `vals[i] = x` / `items[k] = v` writes one element, so
                    // the field must be a list (int index) or a map (index
                    // unifies with the key type, value with the value type).
                    let idx_ty = self.check_expr(idx);
                    match expected_ty {
                        Ty::List(_) => {
                            if idx_ty != Ty::Int && idx_ty != Ty::Any {
                                self.error(
                                    idx.span(),
                                    format!(
                                        "update() index for list field '{}' must be int, got {}",
                                        fname, idx_ty
                                    ),
                                    None,
                                );
                            }
                        }
                        Ty::Map(key_ty, val_ty) => {
                            if **key_ty != Ty::Any
                                && idx_ty != Ty::Any
                                && self.subst.unify(key_ty, &idx_ty).is_err()
                            {
                                self.error(
                                    idx.span(),
                                    format!(
                                        "update() key for map field '{}' expects {}, got {}",
                                        fname, key_ty, idx_ty
                                    ),
                                    None,
                                );
                            }
                            if !val_ty.assignable_from(&expr_ty)
                                && expr_ty != Ty::Any
                                && **val_ty != Ty::Any
                            {
                                self.error(
                                    fexpr.span(),
                                    format!(
                                        "update() map field '{}' holds {} values, got {}",
                                        fname, val_ty, expr_ty
                                    ),
                                    self.type_mismatch_hint(val_ty, &expr_ty),
                                );
                            }
                        }
                        Ty::Any => {}
                        other => {
                            self.error(
                                span,
                                format!(
                                    "update() field '{}' is {}, not a list or map — indexed assignment needs an indexable field",
                                    fname, other
                                ),
                                None,
                            );
                        }
                    }
                } else if !expected_ty.assignable_from(&expr_ty)
                    && expr_ty != Ty::Any
                    && *expected_ty != Ty::Any
                {
                    self.error(
                        fexpr.span(),
                        format!(
                            "update() field '{}' expects {}, got {}",
                            fname, expected_ty, expr_ty
                        ),
                        self.type_mismatch_hint(expected_ty, &expr_ty),
                    );
                }
            } else {
                self.error(
                    span,
                    format!("'{}' has no field '{}'", type_name, fname),
                    None,
                );
            }
        }
    }

    fn check_schedule(&mut self, stmt: &ScheduleStmt) {
        let in_pipeline = self.scopes.iter().rev().any(|s| s.in_pipeline);
        if in_pipeline {
            self.error(
                &stmt.span,
                "Cannot run systems inside a pipeline".to_string(),
                Some("Pipeline functions must be pure".to_string()),
            );
        }
        let mut expanded = Vec::new();
        for name in &stmt.systems {
            if let Some(phase_systems) = self.phases.get(name).cloned() {
                expanded.extend(phase_systems);
            } else {
                expanded.push(name.clone());
            }
        }
        for name in &expanded {
            let resolved_name = self.resolve_canonical_name(name);
            if let Some(sys) = self.systems.get(&resolved_name) {
                if !sys.is_pub && is_cross_file(sys.file_id, stmt.span.file) {
                    self.error(
                        &stmt.span,
                        format!("System '{}' is private", name),
                        Some(format!("Add `pub` to the declaration of '{}'", name)),
                    );
                }
            } else {
                self.error(
                    &stmt.span,
                    format!("Running unknown system '{}'", name),
                    None,
                );
            }
        }
    }

    fn is_type_exported_by_prefix(&self, prefix: &str, type_name: &str) -> bool {
        if self.resolve_canonical_name(prefix) == type_name {
            return true;
        }
        if let Some(alias_map) = self.module_aliases.get(prefix) {
            if alias_map.values().any(|v| v == type_name) {
                return true;
            }
        }
        if prefix == type_name {
            return true;
        }
        false
    }

    fn check_match_stmt(&mut self, stmt: &MatchStmt) -> Ty {
        self.check_match_with_mode(stmt, false)
    }

    fn check_match(&mut self, stmt: &MatchStmt) -> Ty {
        self.check_match_with_mode(stmt, true)
    }}