impl Checker {

fn check_typed_world_builtin(
        &mut self,
        name: &str,
        arg_tys: &[Ty],
        arg_exprs: &[&Expr],
    ) -> Option<Ty> {
        match name {
            "world_digest" => {
                // 0 args: digest the live world. 1 arg: digest a fork's
                // state (the rolling-migration certification view).
                if arg_tys.len() > 1 {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!(
                            "Function 'world_digest' expects 0 or 1 argument(s), got {}",
                            arg_tys.len()
                        ),
                        None,
                    );
                } else if let Some(t) = arg_tys.first() {
                    let rt = self.resolve_ty(t);
                    if rt != Ty::WorldFork && rt != Ty::Any {
                        self.error(
                            arg_exprs[0].span(),
                            format!(
                                "Argument 1 to 'world_digest' expects world_fork, got {}",
                                rt
                            ),
                            None,
                        );
                    }
                }
                Some(Ty::Str)
            }
            "entities" => {
                for i in 0..arg_tys.len() {
                    if !Ty::Str.assignable_from(&arg_tys[i]) && arg_tys[i] != Ty::Any {
                        self.error(
                            arg_exprs[i].span(),
                            format!(
                                "Argument {} to 'entities' expects str, got {}",
                                i + 1,
                                arg_tys[i]
                            ),
                            None,
                        );
                    }
                    let resource_name = match &arg_exprs[i] {
                        Expr::Ident(name, _) => Some(name.as_str()),
                        Expr::ComponentExpr(name, _, _, _) => Some(name.as_str()),
                        _ => None,
                    };
                    if let Some(name) = resource_name {
                        let resolved = self.resolve_canonical_name(name);
                        if self.resources.contains_key(&resolved) {
                            self.error(
                                arg_exprs[i].span(),
                                format!(
                                    "entities() cannot query resource '{}'; resources are global and not attached to entities",
                                    name
                                ),
                                Some("Use get_resource(ResourceType) or inject it as a system parameter".to_string()),
                            );
                        }
                    }
                }
                Some(Ty::List(Box::new(Ty::EntityId)))
            }
            "spawn" => {
                let start_idx = if arg_tys.first().is_some_and(|t| Ty::Str.assignable_from(t)) {
                    1
                } else {
                    0
                };
                for i in start_idx..arg_tys.len() {
                    if !matches!(arg_tys[i], Ty::Component(_) | Ty::Any) {
                        self.error(
                            arg_exprs[i].span(),
                            format!(
                                "Argument {} to 'spawn' expects component, got {}",
                                i + 1,
                                arg_tys[i]
                            ),
                            None,
                        );
                    }
                    let resource_name = match &arg_exprs[i] {
                        Expr::Ident(name, _) => Some(name.as_str()),
                        Expr::ComponentExpr(name, _, _, _) => Some(name.as_str()),
                        _ => None,
                    };
                    if let Some(name) = resource_name {
                        let resolved = self.resolve_canonical_name(name);
                        if self.resources.contains_key(&resolved) {
                            self.error(
                                arg_exprs[i].span(),
                                format!(
                                    "spawn() cannot add resource '{}' as a component",
                                    name
                                ),
                                Some("Resources are global singletons. Declare entity components separately.".to_string()),
                            );
                        }
                    }
                }
                Some(Ty::EntityId)
            }
            "get_resource" => {
                if !arg_exprs.is_empty() {
                    if let Expr::Ident(name, _) = &arg_exprs[0] {
                        let resolved = self.resolve_canonical_name(name);
                        if self.components.contains_key(&resolved)
                            && !self.resources.contains_key(&resolved)
                        {
                            self.error(
                                arg_exprs[0].span(),
                                format!(
                                    "get_resource({}) is invalid — '{}' is a component, not a resource",
                                    name, name
                                ),
                                Some("Use get(entity, ComponentType) to read components from entities.".to_string()),
                            );
                        }
                    }
                }
                None
            }
            // res(R) reads a declared resource directly (no Option): declared
            // resources auto-initialize from field defaults, so absence is a
            // programming error, not a state to pattern-match.
            "res" if arg_tys.len() == 1 => {
                if let Some(Expr::Ident(name, _)) = arg_exprs.first() {
                    let resolved = self.resolve_canonical_name(name);
                    if self.resources.contains_key(&resolved) {
                        return Some(Ty::Component(resolved));
                    }
                    if self.components.contains_key(&resolved) {
                        self.error(
                            arg_exprs[0].span(),
                            format!(
                                "res({}) is invalid — '{}' is a component, not a resource",
                                name, name
                            ),
                            Some(
                                "Use get(entity, ComponentType) to read components from entities."
                                    .to_string(),
                            ),
                        );
                        return Some(Ty::Any);
                    }
                    self.error(
                        arg_exprs[0].span(),
                        format!("res({}) references unknown resource '{}'", name, name),
                        Some(format!("Declare it first: `resource {} {{ ... }}`", name)),
                    );
                    return Some(Ty::Any);
                }
                Some(Ty::Any)
            }
            "set_resource" => {
                if !arg_exprs.is_empty() {
                    let rtype_name = match &arg_exprs[0] {
                        Expr::Ident(name, _) => Some(name.as_str()),
                        _ => None,
                    };
                    if let Some(name) = rtype_name {
                        let resolved = self.resolve_canonical_name(name);
                        if self.components.contains_key(&resolved)
                            && !self.resources.contains_key(&resolved)
                        {
                            self.error(
                                arg_exprs[0].span(),
                                format!(
                                    "set_resource({}, ...) is invalid — '{}' is a component, not a resource",
                                    name, name
                                ),
                                Some("Resources must be declared with the `resource` keyword.".to_string()),
                            );
                        }
                        let in_system = self.scopes.iter().rev().any(|s| s.in_system.is_some());
                        if in_system {
                            let has_mut_param =
                                self.system_params.values().any(|(comp_type, is_mut)| {
                                    *is_mut && self.resolve_canonical_name(comp_type) == resolved
                                });
                            if has_mut_param {
                                self.error(
                                    arg_exprs[0].span(),
                                    format!(
                                        "set_resource({}, ...) conflicts with mutable system parameter of the same resource; mutate the parameter directly",
                                        name
                                    ),
                                    Some("The system writeback will overwrite this set_resource call. Use the `mut` parameter instead.".to_string()),
                                );
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn resolve_component_name(&self, expr: Option<&&Expr>, ty: Option<&Ty>) -> Option<String> {
        if let Some(Expr::Ident(name, _)) = expr {
            if self.components.contains_key(name) {
                return Some(name.clone());
            }
        }
        if let Some(Ty::Component(name)) = ty {
            return Some(name.clone());
        }
        None
    }

    fn check_closure_has_untyped_params(&self, expr: Option<&&Expr>) -> bool {
        if let Some(Expr::FnExpr(_, _, param_types, _, _, _, _)) = expr {
            param_types.iter().any(|pt| pt.is_none())
        } else {
            false
        }
    }

    fn check_sum_type_field_access(
        &mut self,
        sum: &SumTypeDef,
        subject_ty: &Ty,
        field_name: &str,
        span: &Span,
    ) -> Ty {
        let mut field_types = Vec::new();
        let mut missing_in = Vec::new();

        for variant in &sum.variants {
            if let Some((_, ty)) = variant.fields.iter().find(|(n, _)| n == field_name) {
                let resolved_ty = self.resolve_sum_type_field_ty(sum, subject_ty, ty);
                field_types.push(resolved_ty);
            } else {
                missing_in.push(variant.name.clone());
            }
        }

        if !missing_in.is_empty() {
            self.error(
                span,
                format!(
                    "Field '{}' is not present in all variants of sum type '{}'",
                    field_name, sum.name
                ),
                Some(format!("Missing in variants: {}", missing_in.join(", "))),
            );
            return Ty::Any;
        }

        if field_types.is_empty() {
            self.error(
                span,
                format!("Sum type '{}' has no variants", sum.name),
                None,
            );
            return Ty::Any;
        }

        let first_ty = field_types[0].clone();
        for ty in &field_types[1..] {
            if ty != &first_ty {
                self.error(
                    span,
                    format!(
                        "Field '{}' has conflicting types across variants of sum type '{}'",
                        field_name, sum.name
                    ),
                    None,
                );
                return Ty::Any;
            }
        }

        first_ty
    }

    fn resolve_sum_type_field_ty(
        &self,
        sum_type: &SumTypeDef,
        subject_ty: &Ty,
        field_ty: &Ty,
    ) -> Ty {
        match subject_ty {
            Ty::App(name, args)
                if name == &sum_type.name && args.len() == sum_type.type_params.len() =>
            {
                let mapping: HashMap<String, Ty> = sum_type
                    .type_params
                    .iter()
                    .cloned()
                    .zip(args.iter().cloned())
                    .collect();
                self.substitute_type_params(field_ty, &mapping)
            }
            _ => field_ty.clone(),
        }
    }

    fn case_pattern_bindings(&self, case: &MatchCase) -> Vec<MatchBinding> {
        match &case.pattern {
            Pattern::Variant {
                bindings,
                pattern_bindings,
                ..
            } => {
                if !pattern_bindings.is_empty() {
                    return pattern_bindings.clone();
                }
                bindings
                    .iter()
                    .map(|name| MatchBinding {
                        name: name.clone(),
                        path: vec![name.clone()],
                    })
                    .collect()
            }
            _ => vec![],
        }
    }

    fn resolve_case_binding_ty(
        &mut self,
        sum_type: &SumTypeDef,
        subject_ty: &Ty,
        variant: &VariantType,
        binding: &MatchBinding,
        case_span: &Span,
    ) -> Option<Ty> {
        let head = binding.path.first()?;
        let mut current_ty = match variant
            .fields
            .iter()
            .find(|(n, _)| n == head)
            .map(|(_, t)| self.resolve_sum_type_field_ty(sum_type, subject_ty, t))
        {
            Some(ty) => ty,
            None => {
                self.error(
                    case_span,
                    format!(
                        "Error[E2504]: Unknown binding '{}' for variant '{}::{}'",
                        head, sum_type.name, variant.name
                    ),
                    Some(format!(
                        "Known fields: {}",
                        variant
                            .fields
                            .iter()
                            .map(|(n, _)| n.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                );
                return None;
            }
        };

        for (i, seg) in binding.path.iter().enumerate().skip(1) {
            match self.resolve_named_field_ty(&current_ty, seg) {
                Some(ty) => current_ty = ty,
                None => {
                    let path_so_far = binding.path[..=i].join(".");
                    let known_fields = self.known_fields_for_ty(&current_ty);
                    let hint = if known_fields.is_empty() {
                        format!("Type '{}' has no known fields", current_ty)
                    } else {
                        format!("Known fields: {}", known_fields.join(", "))
                    };
                    self.error(
                        case_span,
                        format!(
                            "Error[E2504]: Unknown field '{}' in binding path '{}' for type '{}'",
                            seg, path_so_far, current_ty
                        ),
                        Some(hint),
                    );
                    return None;
                }
            }
        }
        Some(current_ty)
    }

    fn known_fields_for_ty(&self, ty: &Ty) -> Vec<String> {
        match ty {
            Ty::Component(comp_name) => self
                .components
                .get(comp_name)
                .map(|comp| comp.fields.iter().map(|(n, _)| n.clone()).collect())
                .unwrap_or_default(),
            Ty::Struct(struct_name) => self
                .structs
                .get(struct_name)
                .map(|st| st.fields.iter().map(|(n, _)| n.clone()).collect())
                .unwrap_or_default(),
            Ty::SumType(name) | Ty::App(name, _) => self
                .sum_types
                .get(name)
                .map(|sum| {
                    let mut fields = Vec::new();
                    for variant in &sum.variants {
                        for (n, _) in &variant.fields {
                            if !fields.contains(n) {
                                fields.push(n.clone());
                            }
                        }
                    }
                    fields
                })
                .unwrap_or_default(),
            _ => vec![],
        }
    }

    fn resolve_named_field_ty(&self, ty: &Ty, field: &str) -> Option<Ty> {
        match ty {
            Ty::Component(comp_name) => self
                .components
                .get(comp_name)
                .and_then(|comp| comp.field_type(field))
                .cloned(),
            Ty::Struct(struct_name) => self
                .structs
                .get(struct_name)
                .and_then(|st| st.field_type(field))
                .cloned(),
            Ty::SumType(name) => self.sum_types.get(name).and_then(|sum| {
                let mut first_ty = None;
                for variant in &sum.variants {
                    if let Some((_, ty)) = variant.fields.iter().find(|(n, _)| n == field) {
                        if first_ty.is_none() {
                            first_ty = Some(ty.clone());
                        } else if first_ty.as_ref() != Some(ty) {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                first_ty
            }),
            Ty::App(name, args) => self.sum_types.get(name).and_then(|sum| {
                let mut first_ty = None;
                for variant in &sum.variants {
                    if let Some((_, ty)) = variant.fields.iter().find(|(n, _)| n == field) {
                        let resolved = self.resolve_sum_type_field_ty(
                            sum,
                            &Ty::App(name.clone(), args.clone()),
                            ty,
                        );
                        if first_ty.is_none() {
                            first_ty = Some(resolved);
                        } else if first_ty.as_ref() != Some(&resolved) {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                first_ty
            }),
            Ty::Map(_, inner) => Some((**inner).clone()),
            _ => None,
        }
    }

    fn check_fn_expr_with_expected(
        &mut self,
        expr: &Expr,
        expected_params: &[Ty],
        expected_ret: Option<&Ty>,
    ) -> Ty {
        let Expr::FnExpr(
            params,
            param_muts,
            param_types,
            param_destructures,
            return_type,
            body,
            span,
        ) = expr
        else {
            return self.check_expr(expr);
        };
        self.validate_fn_param_alignment(
            "<anonymous function>",
            params.len(),
            param_types.len(),
            span,
        );
        let saved_fn_name = self.current_fn_name.clone();
        let saved_returns = std::mem::take(&mut self.current_fn_returns);
        self.push_scope();
        let mut resolved_param_tys = Vec::new();
        for (idx, param) in params.iter().enumerate() {
            let ty = param_types
                .get(idx)
                .and_then(|ann| ann.as_ref())
                .map(|te| self.resolve_type_expr(te, span))
                .or_else(|| expected_params.get(idx).cloned())
                .unwrap_or(Ty::Any);
            resolved_param_tys.push(ty.clone());
            let is_mut = param_muts.get(idx).copied().unwrap_or(false);
            self.define(param, ty.clone(), is_mut, span.clone(), false, true);
            if let Some(bindings) = param_destructures.get(idx).and_then(|d| d.as_ref()) {
                if let Some(elem_tys) =
                    self.resolve_destructure_element_types(&ty, bindings.len(), span)
                {
                    for (name, elem_ty) in bindings.iter().zip(elem_tys) {
                        self.define(name, elem_ty, is_mut, span.clone(), false, true);
                    }
                } else {
                    for name in bindings {
                        self.define(name, Ty::Any, is_mut, span.clone(), false, true);
                    }
                }
            }
        }
        self.current_fn_name = Some("<anon>".to_string());
        self.check_block(body);
        if !self.block_diverges(body) {
            if let Some(Stmt::Expr(expr_stmt)) = body.stmts.last() {
                let implicit_ret_ty = self.check_expr(&expr_stmt.expr);
                self.current_fn_returns
                    .push((implicit_ret_ty, expr_stmt.span.clone()));
            } else {
                self.current_fn_returns.push((Ty::Nil, span.clone()));
            }
        }
        let explicit_ret = return_type
            .as_ref()
            .map(|ret_ann| self.resolve_type_expr(ret_ann, span));
        let expected_for_merge = explicit_ret.as_ref().or(expected_ret);
        let inferred_ret = self.merge_return_types(span, expected_for_merge);
        let ret_ty = explicit_ret.unwrap_or(inferred_ret);
        self.pop_scope();
        self.current_fn_name = saved_fn_name;
        self.current_fn_returns = saved_returns;
        Ty::Fn {
            params: resolved_param_tys,
            ret: Box::new(ret_ty),
            purity: if self.closure_body_is_conservatively_pure(
                params,
                param_muts,
                param_destructures,
                body,
            ) {
                FnPurity::Pure
            } else if self.closure_body_is_conservatively_readonly(
                params,
                param_muts,
                param_destructures,
                body,
            ) {
                FnPurity::Readonly
            } else {
                FnPurity::Impure
            },
        }
    }
    fn eval_const_bool(expr: &Expr) -> Option<bool> {
        match expr {
            Expr::BoolLit(b, _) => Some(*b),
            Expr::Unary(UnaryOp::Not, inner, _) => Self::eval_const_bool(inner).map(|b| !b),
            Expr::Binary(left, BinOp::And, right, _) => {
                match (Self::eval_const_bool(left), Self::eval_const_bool(right)) {
                    (Some(false), _) | (_, Some(false)) => Some(false),
                    (Some(true), Some(true)) => Some(true),
                    _ => None,
                }
            }
            Expr::Binary(left, BinOp::Or, right, _) => {
                match (Self::eval_const_bool(left), Self::eval_const_bool(right)) {
                    (Some(true), _) | (_, Some(true)) => Some(true),
                    (Some(false), Some(false)) => Some(false),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}