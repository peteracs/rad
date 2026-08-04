impl Checker {

    pub(super) fn check_call_with_types_and_exprs(
        &mut self,
        callee: &Expr,
        callee_ty: Ty,
        arg_tys: &[Ty],
        arg_exprs: &[&Expr],
        span: &Span,
    ) -> Ty {
        let in_pipeline = self.scopes.iter().rev().any(|s| s.in_pipeline);
        if in_pipeline {
            let mut is_callee_allowed = true;
            let mut callee_name = None;

            if let Expr::Ident(name, _) = callee {
                callee_name = Some(name.clone());
                if is_impure_builtin(name) {
                    is_callee_allowed = false;
                } else if is_readonly_builtin(name) {
                    is_callee_allowed = true;
                } else if let Some(sig) = self.functions.get(name) {
                    is_callee_allowed = sig.is_pure || sig.effects.is_readonly();
                }
            } else if let Ty::Fn { purity, .. } = &callee_ty {
                is_callee_allowed = matches!(purity, FnPurity::Pure | FnPurity::Readonly);
            } else if let Expr::FnExpr(params, param_muts, _, param_destructures, _, body, _) =
                callee
            {
                is_callee_allowed = self.closure_body_is_conservatively_pure(
                    params,
                    param_muts,
                    param_destructures,
                    body,
                ) || self.closure_body_is_conservatively_readonly(
                    params,
                    param_muts,
                    param_destructures,
                    body,
                );
            }

            if !is_callee_allowed {
                let msg = if let Some(ref name) = callee_name {
                    if is_impure_builtin(name) {
                        format!("Cannot call impure builtin '{}' inside a pipeline", name)
                    } else {
                        format!(
                            "Cannot call side-effecting function '{}' inside a pipeline",
                            name
                        )
                    }
                } else {
                    "Cannot call a side-effecting function inside a pipeline".to_string()
                };

                let hint = if let Some(ref name) = callee_name {
                    if is_impure_builtin(name) {
                        format!(
                            "'{}' is an impure builtin (it performs side effects). Move this call outside the pipeline",
                            name
                        )
                    } else if is_readonly_builtin(name) {
                        format!(
                            "'{}' is readonly and allowed in pipelines; this error likely comes from another stage",
                            name
                        )
                    } else if let Some(reason) = self.purity_breach_reasons.get(name) {
                        self.build_purity_fix_hint(name, reason)
                    } else {
                        format!(
                            "If '{}' only reads ECS state, declare it `readonly fn {}`. Otherwise call it outside the pipeline",
                            name, name
                        )
                    }
                } else if let Expr::FnExpr(_, _, _, _, _, body, _) = callee {
                    if let Some(reason) = self.find_block_purity_breach(body) {
                        format!("Anonymous function {}", reason)
                    } else {
                        "Pipelines only allow pure/readonly stages; move this call outside the pipeline"
                            .to_string()
                    }
                } else {
                    "Pipelines only allow pure/readonly stages; move this call outside the pipeline"
                        .to_string()
                };
                self.error(span, msg, Some(hint));
            }

            for (i, arg) in arg_exprs.iter().enumerate() {
                let arg_ty = arg_tys.get(i).unwrap_or(&Ty::Any);
                let mut is_arg_allowed = true;
                let mut arg_name = None;

                if let Expr::Ident(name, _) = arg {
                    arg_name = Some(name.clone());
                    if is_impure_builtin(name) {
                        is_arg_allowed = false;
                    } else if is_readonly_builtin(name) {
                        is_arg_allowed = true;
                    } else if let Some(sig) = self.functions.get(name) {
                        is_arg_allowed = sig.is_pure || sig.effects.is_readonly();
                    }
                } else if let Expr::FnExpr(params, param_muts, _, param_destructures, _, body, _) =
                    arg
                {
                    // Literal closures get the same readonly allowance as
                    // named fns (branch above) — a callback that only READS
                    // the world (`map(fn(t) { return name_of(t) })`) is
                    // pipeline-safe. The Ty::Fn arm below can't see
                    // readonly-ness, so the body check must come first.
                    is_arg_allowed = self.closure_body_is_conservatively_pure(
                        params,
                        param_muts,
                        param_destructures,
                        body,
                    ) || self.closure_body_is_conservatively_readonly(
                        params,
                        param_muts,
                        param_destructures,
                        body,
                    );
                } else if let Ty::Fn { purity, .. } = arg_ty {
                    is_arg_allowed = matches!(purity, FnPurity::Pure | FnPurity::Readonly);
                }

                if !is_arg_allowed {
                    let msg = if let Some(ref name) = arg_name {
                        format!(
                            "Cannot pass side-effecting function '{}' as an argument in a pipeline",
                            name
                        )
                    } else {
                        "Cannot pass a side-effecting function as an argument in a pipeline"
                            .to_string()
                    };

                    let hint = if let Some(ref name) = arg_name {
                        if let Some(reason) = self.purity_breach_reasons.get(name) {
                            self.build_purity_fix_hint(name, reason)
                        } else {
                            format!(
                                "Pipeline callbacks must be pure/readonly. If '{}' only reads ECS, declare it `readonly fn {}`",
                                name, name
                            )
                        }
                    } else if let Expr::FnExpr(_, _, _, _, _, body, _) = arg {
                        if let Some(reason) = self.find_block_purity_breach(body) {
                            format!("Pipeline callback {}", reason)
                        } else {
                            "Pipeline callbacks must be pure/readonly (no external side effects)"
                                .to_string()
                        }
                    } else {
                        "Pipeline callbacks must be pure/readonly (no external side effects)"
                            .to_string()
                    };
                    self.error(arg.span(), msg, Some(hint));
                }
            }
        }

        if let Expr::Ident(name, ident_span) = callee {
            if let Some(ret_ty) = self.validate_fn_binding_call_params(name, arg_tys, span) {
                return ret_ty;
            }
            if let Some(sig) = self.functions.get(name).cloned() {
                self.check_effect_boundary(name, &sig.effects, span);
                if !sig.type_params.is_empty() {
                    let (inst_params, inst_ret) = self.instantiate_sig(&sig);
                    if inst_params.len() != arg_tys.len() {
                        let hint = self.signature_hint(name);
                        self.error(
                            span,
                            format!(
                                "Function '{}' expects {} argument(s), got {}",
                                name,
                                inst_params.len(),
                                arg_tys.len()
                            ),
                            hint,
                        );
                        return self.resolve_ty(&inst_ret);
                    }
                    for (i, (expected, actual)) in
                        inst_params.iter().zip(arg_tys.iter()).enumerate()
                    {
                        if self.subst.unify(expected, actual).is_err() && *actual != Ty::Any {
                            let resolved_expected = self.resolve_ty(expected);
                            self.error(
                                span,
                                format!(
                                    "Argument {} to '{}' expects {}, got {}",
                                    i + 1,
                                    name,
                                    resolved_expected,
                                    actual
                                ),
                                self.type_mismatch_hint(&resolved_expected, actual),
                            );
                        }
                    }
                    self.resolve_unbound_vars_in_params(&inst_params);
                    return self.resolve_ty(&inst_ret);
                }
                self.validate_call_args(name, &sig.params, arg_tys, span);
                return sig.ret.clone();
            }
            // Bindings shadow builtins at runtime, so they must shadow them
            // here too — otherwise a parameter named `range` type-checks as
            // the builtin and then dies at runtime with "Not callable: int".
            if let Some(binding) = self.lookup(name) {
                if !matches!(binding.ty, Ty::Fn { .. } | Ty::Any) {
                    let hint = if is_builtin(name) {
                        Some(format!(
                            "A binding named '{}' shadows the builtin '{}()' here — rename the variable to call the builtin",
                            name, name
                        ))
                    } else {
                        let cands = self.callable_name_hint_candidates();
                        let refs: Vec<&str> = cands.iter().map(|s| s.as_str()).collect();
                        suggest_did_you_mean(name, &refs)
                    };
                    self.error(
                        ident_span,
                        format!(
                            "Cannot call non-function '{}' of type {} (not callable)",
                            name, binding.ty
                        ),
                        hint,
                    );
                }
                // An `any`-typed callee is the unverifiable case — the same
                // conservative rule as an impure fn value applies.
                if matches!(binding.ty, Ty::Any) {
                    self.check_fn_value_call_effect_boundary(
                        &format!("'{}' (typed any, so its effects cannot be verified)", name),
                        FnPurity::Impure,
                        span,
                    );
                }
                return Ty::Any;
            }
            if is_builtin(name) {
                self.check_builtin_effect_boundary(name, span);
                return self.check_builtin_call_with_exprs(name, arg_tys, arg_exprs, span);
            }
            return Ty::Any;
        }

        // Module-qualified calls (`alias.helper(...)`) arrive here with the
        // callee's VALUE type instead of through the named-fn branch above.
        // Route their effect check through the FunctionSig like any other
        // named call — a cross-module readonly fn stays callable in a
        // readonly context, and a cross-module impure fn is rejected in
        // restricted ones instead of sailing through unchecked.
        let mut effects_checked_via_sig = false;
        if let Expr::Field(obj, member, _) = callee {
            if let Expr::Ident(alias, _) = obj.as_ref() {
                if let Some(canonical) = self.resolve_alias_member(alias, member) {
                    if let Some(sig) = self.functions.get(&canonical) {
                        let effects = sig.effects.clone();
                        self.check_effect_boundary(&canonical, &effects, span);
                        effects_checked_via_sig = true;
                    }
                }
            }
        }
        let callee_ty_str = format!("{}", callee_ty);
        if let Ty::Fn {
            params,
            ret,
            purity,
        } = callee_ty
        {
            if !effects_checked_via_sig {
                self.check_fn_value_call_effect_boundary(
                    &format!("this function value (typed {})", callee_ty_str),
                    purity,
                    span,
                );
            }
            if params.len() != arg_tys.len() {
                // anonymous callee: derive the signature from the fn type
                let ps: Vec<String> = params.iter().map(|t| format!("{}", t)).collect();
                let hint = Some(format!("signature: fn({}) -> {}", ps.join(", "), ret));
                self.error(
                    span,
                    format!(
                        "Function expects {} argument(s), got {}",
                        params.len(),
                        arg_tys.len()
                    ),
                    hint,
                );
                return *ret;
            }
            self.validate_call_args("<call>", &params, arg_tys, span);
            return *ret;
        }
        if callee_ty != Ty::Any {
            self.error(
                callee.span(),
                format!("Cannot call value of type {}", callee_ty),
                None,
            );
        } else if !effects_checked_via_sig {
            self.check_fn_value_call_effect_boundary(
                "this callee (typed any, so its effects cannot be verified)",
                FnPurity::Impure,
                span,
            );
        }
        Ty::Any
    }

    pub(super) fn validate_fn_binding_call_params(
        &mut self,
        name: &str,
        arg_tys: &[Ty],
        span: &Span,
    ) -> Option<Ty> {
        let mut binding_scope_idx = None;
        let mut binding_ty = None;
        for idx in (0..self.scopes.len()).rev() {
            if let Some(binding) = self.scopes[idx].bindings.get(name) {
                binding_scope_idx = Some(idx);
                binding_ty = Some(binding.ty.clone());
                break;
            }
        }
        let (scope_idx, ty) = match (binding_scope_idx, binding_ty) {
            (Some(i), Some(t)) => (i, t),
            _ => return None,
        };
        if scope_idx == 0 && self.functions.contains_key(name) {
            return None;
        }
        let ty_str = format!("{}", ty);
        let Ty::Fn {
            params,
            ret,
            purity,
        } = ty
        else {
            return None;
        };
        self.check_fn_value_call_effect_boundary(
            &format!("function value '{}' (typed {})", name, ty_str),
            purity,
            span,
        );

        if params.len() != arg_tys.len() {
            let ps: Vec<String> = params.iter().map(|t| format!("{}", t)).collect();
            let hint = Some(format!(
                "signature: fn {}({}) -> {}",
                name,
                ps.join(", "),
                ret
            ));
            self.error(
                span,
                format!(
                    "Function '{}' expects {} argument(s), got {}",
                    name,
                    params.len(),
                    arg_tys.len()
                ),
                hint,
            );
            return Some(*ret);
        }

        for (i, (expected, actual)) in params.iter().zip(arg_tys.iter()).enumerate() {
            if *expected == Ty::Any {
                continue;
            }
            if expected.is_numeric() && actual.is_numeric() {
                continue;
            }
            if !expected.assignable_from(actual) && *actual != Ty::Any {
                let hint = self.type_mismatch_hint(expected, actual);
                self.error(
                    span,
                    format!(
                        "Argument {} to '{}' expects {}, got {}",
                        i + 1,
                        name,
                        expected,
                        actual
                    ),
                    hint,
                );
            }
        }

        let ret_ty = *ret.clone();
        let _ = scope_idx;
        Some(ret_ty)
    }

    pub(super) fn check_builtin_call_with_exprs(
        &mut self,
        name: &str,
        arg_tys: &[Ty],
        arg_exprs: &[&Expr],
        _span: &Span,
    ) -> Ty {
        if let Some(ty) = self.check_typed_ecs_builtin(name, arg_tys, arg_exprs) {
            return ty;
        }

        if simulate_syntax::is_named_call(name, arg_exprs.len()) {
            let systems_arg = arg_exprs[simulate_syntax::SYSTEMS_ARG_INDEX];
            // Const-fold a reference to a top-level immutable system-list
            // binding to its items, so `let SET = [system::A, …]` used as the
            // schedule argument gets the same static treatment (purity
            // analysis, unused-system tracking) as an inline literal.
            let folded = match systems_arg {
                Expr::Ident(nm, sp) => self
                    .system_list_consts
                    .get(nm)
                    .map(|items| Expr::ListLit(items.clone(), sp.clone())),
                _ => None,
            };
            let effective_arg = folded.as_ref().unwrap_or(systems_arg);
            match simulate_syntax::classify_systems_argument(effective_arg) {
                SystemsListForm::StaticSchedule(items) => {
                    for item in items {
                        let Expr::SystemRef(path, item_span) = item else {
                            unreachable!(
                                "StaticSchedule must only contain Expr::SystemRef (simulate_syntax::classify_systems_argument is out of sync)"
                            );
                        };
                        let q = simulate_syntax::system_ref_qualified_string(path);
                        let resolved = self.resolve_canonical_name(&q);
                        // Unknown `system::…` names are reported once per reference via
                        // `check_system_ref_path` when each `SystemRef` is type-checked (before
                        // this builtin pass). Only resolved systems reach the block below.
                        // simulate() is strict; simulate_par()/simulate_many()/
                        // simulate_seeded() tolerate rand_* (explicit seeds
                        // keep them deterministic), so they consult the
                        // lenient breach.
                        let is_par = name == crate::value::Builtin::SimulatePar.name()
                            || name == crate::value::Builtin::SimulateMany.name()
                            || name == crate::value::Builtin::SimulateSeeded.name();
                        let breach = self.systems.get(&resolved).and_then(|sys_type| {
                            if is_par {
                                sys_type.simulation_breach_par.clone()
                            } else {
                                sys_type.simulation_breach.clone()
                            }
                        });
                        if let Some(breach) = breach {
                            let hint = if is_par {
                                "Systems used in simulate_par() must not perform IO — directly or in any handler reachable through their emits (seeded rand_* is allowed)"
                            } else {
                                "Systems used in simulate() must not perform IO — directly or in any handler reachable through their emits; if the system needs randomness, use simulate_par() with an explicit seed"
                            };
                            self.error(
                                item_span,
                                format!("System '{}' cannot be used in {}(): {}", q, name, breach),
                                Some(hint.to_string()),
                            );
                        }
                    }
                }
                SystemsListForm::StringLiteralSchedule(_) => {
                    self.error(
                        _span,
                        format!(
                            "{}() second argument must be a list of `system::…` references, not string literals",
                            name
                        ),
                        Some(
                            "Replace each `\"Name\"` with `system::Name`, e.g. `[system::MySystem]` instead of `[\"MySystem\"]`; empty `[]` is allowed"
                                .to_string(),
                        ),
                    );
                }
                SystemsListForm::MixedLiteralSchedule(_) => {
                    self.error(
                        _span,
                        format!(
                            "{}() cannot mix string literals with `system::…` references in the systems list",
                            name
                        ),
                        Some(
                            "Use only `system::Name` entries, for example `[system::A, system::B]`".to_string(),
                        ),
                    );
                }
                SystemsListForm::NonStaticListLiteral => {
                    self.error(
                        _span,
                        format!(
                            "{}() second argument must be a list literal of `system::…` references only; computed or non-reference elements are not allowed",
                            name
                        ),
                        Some(
                            format!(
                                "Write e.g. {}(fork, [system::A, system::B], ticks) with fixed references at compile time",
                                name
                            ),
                        ),
                    );
                }
                SystemsListForm::NotListLiteral => {
                    self.error(
                        _span,
                        format!(
                            "{}() second argument must be a list literal `[…]` of `system::…` references, or a top-level immutable `let` bound to one — not an arbitrary variable or call result",
                            name
                        ),
                        Some(
                            format!(
                                "Use {n}(f, [system::MySystem], n), or bind the schedule once at top level — `let ROLLOUT = [system::A, system::B]` — and pass `{n}(f, ROLLOUT, n)`",
                                n = name
                            ),
                        ),
                    );
                }
            }
        }

        if let Some(bsig) = crate::builtins::builtin_type_scheme(name) {
            let sig = FunctionSig {
                type_params: bsig.type_params,
                params: bsig.params,
                ret: bsig.ret,
                is_pure: bsig.is_pure,
                effects: if bsig.is_pure {
                    crate::types::EffectSet::pure()
                } else {
                    crate::types::EffectSet::unrestricted()
                },
            };
            let (inst_params, inst_ret) = self.instantiate_sig(&sig);

            let check_len = inst_params.len().min(arg_tys.len());
            for (i, inst_param) in inst_params.iter().enumerate().take(check_len) {
                if !matches!(arg_tys[i], Ty::Fn { .. })
                    || !self.check_closure_has_untyped_params(arg_exprs.get(i))
                {
                    let _ = self.subst.unify(inst_param, &arg_tys[i]);
                }
            }

            self.resolve_unbound_vars_in_params(&inst_params);

            for (i, inst_param) in inst_params.iter().enumerate().take(check_len) {
                if self.check_closure_has_untyped_params(arg_exprs.get(i)) {
                    let resolved_expected = self.resolve_ty(inst_param);
                    if let (
                        Ty::Fn {
                            params: expected_params,
                            ret: expected_ret,
                            ..
                        },
                        Some(closure_expr),
                    ) = (&resolved_expected, arg_exprs.get(i))
                    {
                        let re_ty = self.check_fn_expr_with_expected(
                            closure_expr,
                            expected_params,
                            Some(expected_ret),
                        );
                        let _ = self.subst.unify(inst_param, &re_ty);
                    }
                }
            }

            let resolved_params: Vec<Ty> = inst_params.iter().map(|p| self.resolve_ty(p)).collect();
            let is_variadic = name == "print"
                || name == "eprint"
                || name == "format"
                || name == "entities"
                || name == "spawn"
                || name == "query_where"
                || name == "query_map"
                || name == "query_count";
            let is_optional_args =
                name == "range" || name == "sandbox_run" || name == "simulate_par";

            if !is_variadic && !is_optional_args && resolved_params.len() != arg_tys.len() {
                let hint = self.signature_hint(name);
                self.error(
                    _span,
                    format!(
                        "Function '{}' expects {} argument(s), got {}",
                        name,
                        resolved_params.len(),
                        arg_tys.len()
                    ),
                    hint,
                );
            } else if is_variadic && arg_tys.len() < resolved_params.len() {
                self.error(
                    _span,
                    format!(
                        "Function '{}' expects at least {} argument(s), got {}",
                        name,
                        resolved_params.len(),
                        arg_tys.len()
                    ),
                    None,
                );
            } else if is_optional_args {
                // simulate_par's optional 6th argument is the resource-override list.
                let min_args = if name == "sandbox_run" {
                    3
                } else if name == "simulate_par" {
                    5
                } else {
                    1
                };
                if arg_tys.len() < min_args || arg_tys.len() > resolved_params.len() {
                    self.error(
                        _span,
                        format!(
                            "Function '{}' expects {} to {} argument(s), got {}",
                            name,
                            min_args,
                            resolved_params.len(),
                            arg_tys.len()
                        ),
                        None,
                    );
                } else {
                    let check_len = resolved_params.len().min(arg_tys.len());
                    self.validate_call_args(
                        name,
                        &resolved_params[..check_len],
                        &arg_tys[..check_len],
                        _span,
                    );
                }
            } else {
                let check_len = if is_variadic {
                    // For variadic functions, check up to the number of defined parameters
                    // (e.g., format requires the first argument to be Str)
                    resolved_params.len().min(arg_tys.len())
                } else {
                    resolved_params.len().min(arg_tys.len())
                };
                self.validate_call_args(
                    name,
                    &resolved_params[..check_len],
                    &arg_tys[..check_len],
                    _span,
                );
            }

            self.resolve_ty(&inst_ret)
        } else {
            Ty::Any
        }
    }

    fn resolve_unbound_vars_in_params(&mut self, params: &[Ty]) {
        for p in params {
            self.resolve_unbound_vars(p);
        }
    }

    fn resolve_unbound_vars(&mut self, ty: &Ty) {
        match ty {
            Ty::Var(id) => {
                if self.subst.lookup(*id).is_none() {
                    self.subst.bind(*id, Ty::Any);
                }
            }
            Ty::List(inner) => self.resolve_unbound_vars(inner),
            Ty::Map(key, val) => {
                self.resolve_unbound_vars(key);
                self.resolve_unbound_vars(val);
            }
            Ty::Fn { params, ret, .. } => {
                for p in params {
                    self.resolve_unbound_vars(p);
                }
                self.resolve_unbound_vars(ret);
            }
            Ty::App(_, args) => {
                for a in args {
                    self.resolve_unbound_vars(a);
                }
            }
            _ => {}
        }
    }

    fn check_builtin_arg_count(
        &mut self,
        name: &str,
        expected: usize,
        arg_tys: &[Ty],
        arg_exprs: &[&Expr],
    ) -> bool {
        if arg_tys.len() != expected {
            let hint = self.signature_hint(name);
            self.error(
                arg_exprs
                    .first()
                    .map(|e| e.span())
                    .unwrap_or(&Span::default()),
                format!(
                    "Function '{}' expects {} argument(s), got {}",
                    name,
                    expected,
                    arg_tys.len()
                ),
                hint,
            );
            return false;
        }
        true
    }

    fn extract_sequence_element_type(&mut self, name: &str, subject_ty: &Ty, span: &Span) -> Ty {
        match subject_ty {
            Ty::List(inner) => *inner.clone(),
            Ty::Str => Ty::Str,
            Ty::Any => Ty::Any,
            _ => {
                self.error(
                    span,
                    format!(
                        "Argument 1 to '{}' expects list or str, got {}",
                        name, subject_ty
                    ),
                    None,
                );
                Ty::Any
            }
        }
    }

    fn validate_callback_param_type(
        &mut self,
        name: &str,
        cb_params: &[Ty],
        elem_ty: &Ty,
        cb_arg_idx: usize,
        span: &Span,
    ) {
        if let Some(p) = cb_params.first() {
            if !p.assignable_from(elem_ty) && *elem_ty != Ty::Any {
                self.error(
                    span,
                    format!(
                        "Argument {} to '{}' expects fn({}), got fn({})",
                        cb_arg_idx + 1,
                        name,
                        elem_ty,
                        p
                    ),
                    None,
                );
            }
        }
    }

    fn require_callback_arg(
        &mut self,
        name: &str,
        func_ty: &Ty,
        cb_arg_idx: usize,
        span: &Span,
    ) -> Option<(Vec<Ty>, Ty)> {
        if let Ty::Fn { params, ret, .. } = func_ty {
            Some((params.clone(), *ret.clone()))
        } else if *func_ty != Ty::Any {
            self.error(
                span,
                format!(
                    "Argument {} to '{}' expects function, got {}",
                    cb_arg_idx + 1,
                    name,
                    func_ty
                ),
                None,
            );
            None
        } else {
            None
        }
    }

    fn check_typed_ecs_builtin(
        &mut self,
        name: &str,
        arg_tys: &[Ty],
        arg_exprs: &[&Expr],
    ) -> Option<Ty> {
        match name {
            "get" | "require" | "require_all" | "set_at" | "keys" | "values" | "entries" | "set" | "transition" | "contains" | "sort" | "reverse" | "slice" | "append" | "extend" | "flat_map" | "map" | "filter" | "reduce" | "group_by" | "sort_by" | "zip" => self.check_typed_collection_builtin(name, arg_tys, arg_exprs),
            "unwrap" | "expect" | "unwrap_or" | "map_or" | "query_where" | "query_map" | "query_count" => self.check_typed_query_builtin(name, arg_tys, arg_exprs),
            "world_digest" | "entities" | "spawn" | "get_resource" | "res" | "set_resource" => self.check_typed_world_builtin(name, arg_tys, arg_exprs),
            _ => None,
        }
    }}