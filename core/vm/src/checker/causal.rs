use super::*;

impl Checker {
    pub(super) fn warn_unproduced_resolvers(&mut self) {
        let unused = self
            .resolvers
            .iter()
            .filter(|(intent, _)| !self.proposed_intents.contains(*intent))
            .flat_map(|(intent, owners)| {
                owners
                    .iter()
                    .map(|owner| (intent.clone(), owner.clone()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        for (intent, owner) in unused {
            self.warning(
                &owner.span,
                format!(
                    "Resolver '{}' owns intent '{}', but no law can propose that intent",
                    owner.name, intent
                ),
                Some("An unused resolver is legal in v0".to_string()),
            );
        }
    }

    pub(super) fn causal_laws_enabled(&self) -> bool {
        self.options
            .features
            .iter()
            .any(|f| f == "causal_laws" || f == "experimental-laws")
    }

    pub(super) fn register_intent(&mut self, decl: &IntentDecl) {
        if self.intents.contains_key(&decl.name) {
            self.error(
                &decl.span,
                format!("Intent '{}' is already declared", decl.name),
                None,
            );
            return;
        }
        if self.components.contains_key(&decl.name)
            || self.resources.contains_key(&decl.name)
            || self.structs.contains_key(&decl.name)
            || self.events.contains_key(&decl.name)
        {
            self.error(
                &decl.span,
                format!("Intent '{}' conflicts with an existing type", decl.name),
                Some("Intent names share RAD's type namespace".to_string()),
            );
        }

        let mut fields = Vec::new();
        let mut keys = Vec::new();
        let mut seen = HashSet::new();
        for field in &decl.fields {
            if !seen.insert(field.name.clone()) {
                self.error(
                    &field.span,
                    format!("Duplicate field '{}' in intent '{}'", field.name, decl.name),
                    None,
                );
            }
            let ty = self.resolve_type_expr(&field.type_annotation, &field.span);
            if field.is_key {
                keys.push((field.name.clone(), ty.clone(), field.span.clone()));
            }
            fields.push((field.name.clone(), ty));
        }
        if keys.len() != 1 {
            self.error(
                &decl.span,
                format!(
                    "Intent '{}' must declare exactly one `key` field; found {}",
                    decl.name,
                    keys.len()
                ),
                Some(
                    "Mark one entity field as `key`, for example `key target: entity`".to_string(),
                ),
            );
        }
        if let Some((name, ty, span)) = keys.first() {
            if *ty != Ty::EntityId && *ty != Ty::Any {
                self.error(
                    span,
                    format!(
                        "Intent '{}.{}' is the key but has type {}; v0 keys must be entity",
                        decl.name, name, ty
                    ),
                    None,
                );
            }
        }
        self.intents.insert(
            decl.name.clone(),
            IntentType {
                name: decl.name.clone(),
                fields,
                file_id: decl.span.file,
            },
        );
    }

    pub(super) fn register_law(&mut self, decl: &LawDecl) {
        let params: Vec<Ty> = decl
            .param_types
            .iter()
            .map(|ty| self.resolve_type_expr(ty, &decl.span))
            .collect();
        if self.laws.contains_key(&decl.name) || self.functions.contains_key(&decl.name) {
            self.error(
                &decl.span,
                format!("Law '{}' conflicts with an existing callable", decl.name),
                None,
            );
            return;
        }
        self.laws.insert(
            decl.name.clone(),
            LawType {
                params: params.clone(),
            },
        );
        self.functions.insert(
            decl.name.clone(),
            FunctionSig {
                type_params: Vec::new(),
                params: params.clone(),
                ret: Ty::Nil,
                is_pure: false,
                effects: EffectSet::single(Effect::ReadECS),
            },
        );
        self.fn_param_names
            .insert(decl.name.clone(), decl.params.clone());
        self.define(
            &decl.name,
            Ty::Fn {
                params,
                ret: Box::new(Ty::Nil),
                purity: FnPurity::Readonly,
            },
            false,
            decl.span.clone(),
            decl.is_pub,
            false,
        );
    }

    pub(super) fn register_resolver(&mut self, decl: &ResolverDecl) {
        let intent_name = self.resolve_canonical_name(&decl.intent_name);
        self.resolvers
            .entry(intent_name)
            .or_default()
            .push(ResolverOwner {
                name: decl.name.clone(),
                span: decl.span.clone(),
            });
    }

    pub(super) fn register_constraint(&mut self, decl: &ConstraintDecl) {
        let name = self.resolve_canonical_name(&decl.name);
        if self.constraints.contains_key(&name) || self.functions.contains_key(&name) {
            self.error(
                &decl.span,
                format!(
                    "Constraint '{}' conflicts with an existing declaration",
                    decl.name
                ),
                None,
            );
            return;
        }
        let attached_component = self.resolve_canonical_name(&decl.component_name);
        let watches = decl
            .watches
            .iter()
            .map(|watch| self.resolve_canonical_name(watch))
            .collect();
        self.constraints.insert(
            name.clone(),
            ConstraintType {
                attached_component,
                watches,
            },
        );
    }

    pub(super) fn check_intent_decl(&mut self, decl: &IntentDecl) {
        self.require_causal_feature(&decl.span);
    }

    pub(super) fn check_law_decl(&mut self, decl: &LawDecl) {
        self.require_causal_feature(&decl.span);
        if decl.params.len() != decl.param_types.len() {
            self.error(
                &decl.span,
                format!("Law '{}' has inconsistent parameter metadata", decl.name),
                None,
            );
        }

        if let Some(reason) = self.find_block_readonly_breach(&decl.body) {
            self.error(
                &decl.span,
                format!("law `{}` cannot perform this effect: {}", decl.name, reason),
                Some(
                    "Laws read one immutable settlement snapshot and may only produce typed proposals"
                        .to_string(),
                ),
            );
        }

        let saved_fn = self.current_fn_name.replace(decl.name.clone());
        let saved_returns = std::mem::take(&mut self.current_fn_returns);
        self.push_scope();
        {
            let scope = self.scopes.last_mut().unwrap();
            scope.effect_context = EffectSet::single(Effect::ReadECS);
            scope.causal_context = CausalContext::Law(decl.name.clone());
        }
        for (index, name) in decl.params.iter().enumerate() {
            let ty = self
                .laws
                .get(&decl.name)
                .and_then(|law| law.params.get(index))
                .cloned()
                .unwrap_or(Ty::Any);
            self.define(name, ty, false, decl.span.clone(), false, false);
        }
        self.check_block(&decl.body);
        self.pop_scope();
        self.current_fn_name = saved_fn;
        self.current_fn_returns = saved_returns;
    }

    pub(super) fn check_resolver_decl(&mut self, decl: &ResolverDecl) {
        self.require_causal_feature(&decl.span);
        let intent_name = self.resolve_canonical_name(&decl.intent_name);
        let Some(intent) = self.intents.get(&intent_name).cloned() else {
            self.error(
                &decl.span,
                format!(
                    "Resolver '{}' owns unknown intent '{}'",
                    decl.name, decl.intent_name
                ),
                None,
            );
            return;
        };
        let owners = self
            .resolvers
            .get(&intent_name)
            .cloned()
            .unwrap_or_default();
        if owners.len() > 1 && owners.first().is_some_and(|o| o.name == decl.name) {
            let locations = owners
                .iter()
                .map(|o| format!("`{}` at line {}", o.name, o.span.line))
                .collect::<Vec<_>>()
                .join(", ");
            self.error(
                &decl.span,
                format!("intent `{}` has multiple resolvers", intent.name),
                Some(format!(
                    "An intent has exactly one owning resolver. Declarations: {}",
                    locations
                )),
            );
        }
        if intent.file_id != decl.span.file {
            self.error(
                &decl.span,
                format!(
                    "Resolver '{}' must be declared in the same module as intent '{}'",
                    decl.name, decl.intent_name
                ),
                Some("Imported modules cannot override another module's resolver".to_string()),
            );
        }
        if let Some(reason) = self.find_block_readonly_breach(&decl.body) {
            self.error(
                &decl.span,
                format!("resolver `{}` cannot perform this effect: {}", decl.name, reason),
                Some("Resolvers read the base snapshot and may only stage component replacements with `next`".to_string()),
            );
        }

        let saved_fn = self.current_fn_name.replace(decl.name.clone());
        let saved_returns = std::mem::take(&mut self.current_fn_returns);
        self.push_scope();
        {
            let scope = self.scopes.last_mut().unwrap();
            scope.effect_context = EffectSet::single(Effect::ReadECS);
            scope.causal_context = CausalContext::Resolver {
                name: decl.name.clone(),
                intent: intent_name,
                key_param: decl.key_param.clone(),
            };
        }
        self.define(
            &decl.key_param,
            Ty::EntityId,
            false,
            decl.span.clone(),
            false,
            false,
        );
        let intent_struct_name = Self::intent_struct_name(&intent.name);
        self.structs
            .entry(intent_struct_name.clone())
            .or_insert(StructType {
                name: intent_struct_name.clone(),
                is_pub: false,
                file_id: intent.file_id,
                fields: intent.fields.clone(),
            });
        self.define(
            &decl.proposals_param,
            Ty::List(Box::new(Ty::Struct(intent_struct_name))),
            false,
            decl.span.clone(),
            false,
            false,
        );
        self.check_block(&decl.body);
        self.pop_scope();
        self.current_fn_name = saved_fn;
        self.current_fn_returns = saved_returns;
    }

    pub(super) fn check_constraint_decl(&mut self, decl: &ConstraintDecl) {
        if !self.causal_laws_enabled() {
            self.error(
                &decl.span,
                format!(
                    "constraint `{}` is experimental RAD Causal Laws syntax",
                    decl.name
                ),
                Some("Pass `--experimental-laws` to enable RFC-0002 syntax".into()),
            );
        }
        let name = self.resolve_canonical_name(&decl.name);
        let Some(info) = self.constraints.get(&name).cloned() else {
            return;
        };
        let Some(component) = self.components.get(&info.attached_component).cloned() else {
            self.error(
                &decl.span,
                format!(
                    "Constraint '{}' is attached to unknown component '{}'",
                    decl.name, decl.component_name
                ),
                None,
            );
            return;
        };
        if component.file_id != decl.span.file {
            self.error(
                &decl.span,
                format!(
                    "Constraint '{}' must be declared in the same module as component '{}'",
                    decl.name, decl.component_name
                ),
                Some("Imported modules cannot attach invariants to foreign components".into()),
            );
        }
        let mut seen = HashSet::new();
        for watch in &decl.watches {
            let resolved = self.resolve_canonical_name(watch);
            if resolved == info.attached_component {
                self.error(
                    &decl.span,
                    format!(
                        "Constraint '{}' watches its attached component '{}' twice",
                        decl.name, watch
                    ),
                    Some("The attached component is already an implicit trigger".into()),
                );
            }
            if !seen.insert(resolved.clone()) {
                self.error(
                    &decl.span,
                    format!("Constraint '{}' has duplicate watch '{}'", decl.name, watch),
                    None,
                );
            }
            if !self.components.contains_key(&resolved) {
                self.error(
                    &decl.span,
                    format!(
                        "Constraint '{}' watches unknown component '{}'",
                        decl.name, watch
                    ),
                    None,
                );
            }
        }
        if let Some(reason) = self.find_block_readonly_breach(&decl.body) {
            self.error(
                &decl.span,
                format!(
                    "constraint `{}` cannot perform this effect: {}",
                    decl.name, reason
                ),
                Some("Constraints only read the immutable base and complete candidate".into()),
            );
        }
        let saved_fn = self.current_fn_name.replace(decl.name.clone());
        let saved_returns = std::mem::take(&mut self.current_fn_returns);
        self.push_scope();
        {
            let scope = self.scopes.last_mut().unwrap();
            scope.effect_context = EffectSet::single(Effect::ReadECS);
            scope.causal_context = CausalContext::Constraint {
                name: name.clone(),
                attached_component: info.attached_component.clone(),
                subject_param: decl.subject_param.clone(),
                proposed_param: decl.proposed_param.clone(),
                watches: info.watches.clone(),
            };
        }
        self.define(
            &decl.subject_param,
            Ty::EntityId,
            false,
            decl.span.clone(),
            false,
            false,
        );
        self.define(
            &decl.proposed_param,
            Ty::Component(info.attached_component),
            false,
            decl.span.clone(),
            false,
            false,
        );
        self.check_block(&decl.body);
        self.pop_scope();
        self.current_fn_name = saved_fn;
        self.current_fn_returns = saved_returns;
    }

    pub(super) fn check_require_stmt(&mut self, stmt: &RequireStmt) {
        const MAX_CONSTRAINT_CODE_BYTES: usize = 128;
        if !matches!(
            self.scopes.last().map(|scope| &scope.causal_context),
            Some(CausalContext::Constraint { .. })
        ) {
            self.error(
                &stmt.span,
                "`require ... else \"code\"` is only valid inside a constraint".into(),
                None,
            );
        }
        let condition = self.check_expr(&stmt.condition);
        if condition != Ty::Bool && condition != Ty::Any {
            self.error(
                stmt.condition.span(),
                format!("constraint requirement must be bool, got {condition}"),
                None,
            );
        }
        if stmt.code.is_empty()
            || stmt.code.len() > MAX_CONSTRAINT_CODE_BYTES
            || !stmt.code.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })
        {
            self.error(
                &stmt.span,
                format!("constraint violation code '{}' is not stable", stmt.code),
                Some(format!(
                    "Use at most {MAX_CONSTRAINT_CODE_BYTES} lowercase ASCII letters, digits, '.', '_' or '-'"
                )),
            );
        }
    }

    pub(super) fn check_constraint_read_call(
        &mut self,
        read_kind: &str,
        args: &[Expr],
        span: &Span,
    ) -> Option<Ty> {
        if read_kind != "base" && read_kind != "candidate" {
            return None;
        }
        let context = self.scopes.last().unwrap().causal_context.clone();
        let CausalContext::Constraint {
            name,
            attached_component,
            subject_param,
            watches,
            ..
        } = context
        else {
            self.error(
                span,
                format!("`{read_kind}` is only valid inside a constraint"),
                None,
            );
            for arg in args {
                self.check_expr(arg);
            }
            return Some(Ty::Any);
        };
        if args.len() != 2 {
            self.error(
                span,
                format!("`{read_kind}` expects (subject, Component)"),
                None,
            );
            for arg in args {
                self.check_expr(arg);
            }
            return Some(Ty::Any);
        }
        if !matches!(&args[0], Expr::Ident(subject, _) if subject == &subject_param) {
            self.error(
                args[0].span(),
                format!("constraint `{name}` may only read its current subject `{subject_param}`"),
                Some("RFC-0002 v0 permits same-entity candidate dependencies only".into()),
            );
        }
        self.check_expr(&args[0]);
        let Expr::Ident(component, component_span) = &args[1] else {
            self.error(
                args[1].span(),
                format!("`{read_kind}` expects a component type as its second argument"),
                None,
            );
            return Some(Ty::Any);
        };
        let component = self.resolve_canonical_name(component);
        if !self.components.contains_key(&component) {
            self.error(
                component_span,
                format!("`{read_kind}` references unknown component '{component}'"),
                None,
            );
            return Some(Ty::Any);
        }
        if component != attached_component && !watches.contains(&component) {
            self.error(
                component_span,
                format!(
                    "constraint `{name}` reads `{component}` without declaring `watches {component}`"
                ),
                Some("Explicit watches make candidate invalidation auditable".into()),
            );
        }
        Some(Ty::Component(component))
    }

    pub(super) fn check_settle_stmt(&mut self, stmt: &SettleStmt) {
        self.require_causal_feature(&stmt.span);
        let context = self.scopes.last().unwrap().causal_context.clone();
        if context != CausalContext::None {
            let message = if context == CausalContext::Settlement {
                "nested settlements are not allowed".to_string()
            } else {
                "`settle` cannot be invoked from a law or resolver".to_string()
            };
            self.error(&stmt.span, message, None);
        }
        if self
            .scopes
            .iter()
            .rev()
            .any(|scope| scope.in_system.is_some())
        {
            self.error(
                &stmt.span,
                "`settle` cannot run from inside a system worker".to_string(),
                Some(
                    "Invoke the settlement from synchronous host code or an event handler"
                        .to_string(),
                ),
            );
        }
        self.push_scope();
        {
            let scope = self.scopes.last_mut().unwrap();
            scope.effect_context = EffectSet::single(Effect::ReadECS);
            scope.causal_context = CausalContext::Settlement;
            scope.settlement_depth += 1;
        }
        self.check_block(&stmt.body);
        self.pop_scope();
    }

    pub(super) fn check_propose_stmt(&mut self, stmt: &ProposeStmt) {
        self.require_causal_feature(&stmt.span);
        let context = self.scopes.last().unwrap().causal_context.clone();
        if !matches!(context, CausalContext::Law(_)) {
            self.error(
                &stmt.span,
                format!("`propose {}` is only valid inside a law", stmt.intent_name),
                None,
            );
        }
        let resolved = self.resolve_canonical_name(&stmt.intent_name);
        let Some(intent) = self.intents.get(&resolved).cloned() else {
            self.error(
                &stmt.span,
                format!("Cannot propose unknown intent '{}'", stmt.intent_name),
                None,
            );
            for (_, expr) in &stmt.fields {
                self.check_expr(expr);
            }
            return;
        };
        self.proposed_intents.insert(resolved.clone());
        self.check_typed_causal_fields(
            "intent",
            &intent.name,
            &intent.fields,
            &stmt.fields,
            &stmt.span,
        );
        if self.resolvers.get(&resolved).is_none_or(Vec::is_empty) {
            self.error(
                &stmt.span,
                format!("proposed intent `{}` has no resolver", intent.name),
                Some("Declare its single owning resolver in the same module".to_string()),
            );
        }
    }

    pub(super) fn check_next_stmt(&mut self, stmt: &NextStmt) {
        self.require_causal_feature(&stmt.span);
        let context = self.scopes.last().unwrap().causal_context.clone();
        let CausalContext::Resolver {
            name, key_param, ..
        } = context
        else {
            self.error(
                &stmt.span,
                "`next` is only valid inside a resolver".to_string(),
                None,
            );
            self.check_expr(&stmt.entity);
            for (_, expr) in &stmt.fields {
                self.check_expr(expr);
            }
            return;
        };
        if !matches!(&stmt.entity, Expr::Ident(target, _) if target == &key_param) {
            self.error(
                &stmt.span,
                format!(
                    "resolver `{}` attempted to write another entity, but its current key is `{}`",
                    name, key_param
                ),
                Some("v0 resolvers may only write their current key entity".to_string()),
            );
        }
        self.check_expr(&stmt.entity);
        let component_name = self.resolve_canonical_name(&stmt.component_name);
        if !self.components.contains_key(&component_name) {
            self.error(
                &stmt.span,
                format!(
                    "`next` can only replace a component; '{}' is not a component",
                    stmt.component_name
                ),
                None,
            );
            for (_, expr) in &stmt.fields {
                self.check_expr(expr);
            }
            return;
        }
        self.check_expr(&Expr::ComponentExpr(
            component_name,
            stmt.fields.clone(),
            None,
            stmt.span.clone(),
        ));
    }

    pub(super) fn check_law_call_context(&mut self, name: &str, span: &Span) {
        let resolved = self.resolve_canonical_name(name);
        if !self.laws.contains_key(&resolved) {
            return;
        }
        match self.scopes.last().unwrap().causal_context.clone() {
            CausalContext::Settlement => {}
            CausalContext::Law(caller) => self.error(
                span,
                format!("law `{}` cannot call law `{}` in v0", caller, name),
                Some("Invoke both laws directly in the enclosing `settle` block".to_string()),
            ),
            _ => self.error(
                span,
                format!("law `{}` may only be called inside `settle`", name),
                None,
            ),
        }
    }

    pub(super) fn check_causal_statement_boundary(&mut self, stmt: &Stmt) {
        let context = self.scopes.last().unwrap().causal_context.clone();
        let inside_anonymous_function = self.current_fn_name.as_deref() == Some("<anon>");
        match (&context, stmt) {
            (CausalContext::None, _) => {}
            (CausalContext::Settlement, Stmt::Return(_)) => self.error(
                stmt.span(),
                "`return` cannot leave a settlement before its atomic commit".to_string(),
                Some("Move the control flow outside `settle`".to_string()),
            ),
            (CausalContext::Law(name), Stmt::Return(ret))
            | (CausalContext::Resolver { name, .. }, Stmt::Return(ret))
            | (CausalContext::Constraint { name, .. }, Stmt::Return(ret))
                if ret.value.is_some() && !inside_anonymous_function =>
            {
                self.error(
                    stmt.span(),
                    format!("`{}` returns no meaningful value", name),
                    Some("Use a bare `return` only for early exit".to_string()),
                )
            }
            (_, Stmt::Emit(_)) => self.error(
                stmt.span(),
                "Causal settlements cannot emit events".to_string(),
                Some("Emit after the settlement commits".to_string()),
            ),
            (_, Stmt::Schedule(_)) => self.error(
                stmt.span(),
                "Causal settlements cannot run schedules".to_string(),
                None,
            ),
            (_, Stmt::Update(_)) => self.error(
                stmt.span(),
                "Causal code cannot call `update`".to_string(),
                Some(
                    "A law must `propose`; a resolver may stage one replacement with `next`"
                        .to_string(),
                ),
            ),
            _ => {}
        }
    }

    fn check_typed_causal_fields(
        &mut self,
        kind: &str,
        type_name: &str,
        expected: &[(String, Ty)],
        actual: &[(String, Expr)],
        span: &Span,
    ) {
        let mut seen = HashSet::new();
        for (name, expr) in actual {
            if !seen.insert(name.clone()) {
                self.error(
                    expr.span(),
                    format!("Duplicate field '{}' in {} '{}'", name, kind, type_name),
                    None,
                );
            }
            let actual_ty = self.check_expr(expr);
            match expected.iter().find(|(field, _)| field == name) {
                Some((_, expected_ty))
                    if actual_ty != Ty::Any && !expected_ty.assignable_from(&actual_ty) =>
                {
                    self.error(
                        expr.span(),
                        format!(
                            "Type error in '{}.{}': expected {}, got {}",
                            type_name, name, expected_ty, actual_ty
                        ),
                        None,
                    );
                }
                Some(_) => {}
                None => self.error(
                    expr.span(),
                    format!("Unknown field '{}' on {} '{}'", name, kind, type_name),
                    None,
                ),
            }
        }
        for (name, _) in expected {
            if !actual.iter().any(|(actual_name, _)| actual_name == name) {
                self.error(
                    span,
                    format!("Missing field '{}' in {} '{}'", name, kind, type_name),
                    None,
                );
            }
        }
    }

    fn require_causal_feature(&mut self, span: &Span) {
        if !self.causal_laws_enabled() {
            self.error(
                span,
                "RAD Causal Laws is experimental".to_string(),
                Some("Pass `--experimental-laws` to enable RFC-0001 syntax".to_string()),
            );
        }
    }

    pub(crate) fn intent_struct_name(intent: &str) -> String {
        format!("__intent__{}", intent)
    }
}
