impl Checker {

fn check_expr_operations(&mut self, expr: &Expr) -> Ty {
        match expr {
            Expr::Ident(name, span) => {
                if let Some(redirected) = self.redirect_alias_name(name) {
                    return self.check_expr(&Expr::Ident(redirected, span.clone()));
                }
                match self.lookup_with_depth(name) {
                    Some((binding, depth)) => {
                        if binding.is_unique {
                            if let Some(anon_base_depth) = self.anon_fn_scope_bases.last().copied()
                            {
                                if depth < anon_base_depth {
                                    self.error(
                                        span,
                                        format!(
                                            "Cannot alias unique binding '{}' by capturing it in a closure",
                                            name
                                        ),
                                        Some(
                                            "Use the unique value directly outside the closure, or move/rename it before capture"
                                                .to_string(),
                                        ),
                                    );
                                }
                            }
                        }
                        self.mark_var_read(name);
                        binding.ty.clone()
                    }
                    None => {
                        if is_builtin(name) {
                            if let Some(sig) = crate::builtins::builtin_type_scheme(name) {
                                let mut mapping = std::collections::HashMap::new();
                                for tp in &sig.type_params {
                                    mapping.insert(tp.clone(), self.fresh_var());
                                }
                                let params = sig
                                    .params
                                    .iter()
                                    .map(|p| self.substitute_type_params(p, &mapping))
                                    .collect();
                                let ret = Box::new(self.substitute_type_params(&sig.ret, &mapping));
                                // First-class builtins carry their real
                                // purity rank: the readonly read family
                                // (res, get, get_resource, …) is Readonly so
                                // it satisfies readonly-callback params.
                                let purity = if sig.is_pure {
                                    FnPurity::Pure
                                } else if is_readonly_builtin(name) {
                                    FnPurity::Readonly
                                } else {
                                    FnPurity::Impure
                                };
                                Ty::Fn {
                                    params,
                                    ret,
                                    purity,
                                }
                            } else {
                                Ty::Fn {
                                    params: vec![],
                                    ret: Box::new(Ty::Any),
                                    purity: if is_impure_builtin(name) {
                                        FnPurity::Impure
                                    } else {
                                        FnPurity::Pure
                                    },
                                }
                            }
                        } else {
                            let hint = if self.functions.contains_key(name) {
                                Some(format!(
                                "Function '{}' exists, but RAD resolves only names already in scope here. Move its declaration above this usage or call it through a previously bound function value.",
                                name
                            ))
                            } else {
                                let cands = self.scope_binding_hint_names();
                                let refs: Vec<&str> = cands.iter().map(|s| s.as_str()).collect();
                                suggest_did_you_mean(name, &refs)
                            };
                            self.error(span, format!("Undefined variable '{}'", name), hint);
                            Ty::Any
                        }
                    }
                }
            }
            Expr::Binary(left, op, right, span) => {
                if matches!(op, BinOp::Is) {
                    let lt = self.check_expr(left);
                    if let Expr::Ident(name, _) = &**right {
                        // Check if `name` is a valid variant for `lt`
                        let resolved_lt = self.resolve_ty(&lt);
                        match resolved_lt {
                            Ty::SumType(ref type_name) | Ty::App(ref type_name, _) => {
                                if let Some(sum_type) = self.sum_types.get(type_name).cloned() {
                                    if !sum_type.variants.iter().any(|v| v.name == *name) {
                                        self.error(
                                            span,
                                            format!(
                                                "Unknown variant '{}' for type '{}'",
                                                name, type_name
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
                            Ty::State(ref machine) => {
                                if let Some(sm) = self.state_machines.get(machine).cloned() {
                                    if !sm.states.iter().any(|s| s == name) {
                                        self.error(
                                            span,
                                            format!(
                                                "Unknown state '{}' for machine '{}'",
                                                name, machine
                                            ),
                                            Some(format!("Known states: {}", sm.states.join(", "))),
                                        );
                                    }
                                }
                            }
                            Ty::Any => {} // OK
                            _ => {
                                self.error(
                                    span,
                                    format!(
                                        "Operator 'is' expects a sum type or state machine, got {}",
                                        resolved_lt
                                    ),
                                    None,
                                );
                            }
                        }
                    } else {
                        self.error(
                            span,
                            "Right side of 'is' must be an identifier".to_string(),
                            None,
                        );
                    }
                    return Ty::Bool;
                }
                let lt = self.check_expr(left);
                let rt = self.check_expr(right);
                self.check_binary_op(&lt, op, &rt, span)
            }
            Expr::Unary(op, operand, _) => {
                let t = self.check_expr(operand);
                match op {
                    UnaryOp::Neg => {
                        if t.is_numeric() || t == Ty::Any || matches!(t, Ty::Tuple(_)) {
                            t
                        } else {
                            self.error(
                                operand.span(),
                                format!("Unary '-' requires a numeric type, got {}", t),
                                None,
                            );
                            Ty::Any
                        }
                    }
                    UnaryOp::Not => {
                        if t != Ty::Bool && t != Ty::Any {
                            self.error(
                                operand.span(),
                                format!("Unary '!' requires bool, got {}", t),
                                None,
                            );
                        }
                        Ty::Bool
                    }
                    UnaryOp::BitNot => {
                        if t != Ty::Int && t != Ty::Any {
                            self.error(
                                operand.span(),
                                format!("Bitwise '~' requires int, got {}", t),
                                Some(
                                    "~ flips all 64 bits; for bool negation use `not`".to_string(),
                                ),
                            );
                        }
                        Ty::Int
                    }
                }
            }
            Expr::Pipe(left, right, span) => {
                let input_ty = self.check_expr(left);
                self.push_scope();
                if let Some(scope) = self.scopes.last_mut() {
                    scope.in_pipeline = true;
                }

                let ret_ty = match right.as_ref() {
                    Expr::Call(callee, args, _) => {
                        let callee_ty = self.check_expr(callee);
                        let mut arg_tys = vec![input_ty];
                        let mut arg_exprs: Vec<&Expr> = vec![left];
                        for arg in args {
                            if let Expr::Spread(inner, s_span) = arg {
                                let inner_ty = self.check_expr(inner);
                                if let Ty::Tuple(tys) = inner_ty {
                                    self.spread_lengths.insert(s_span.clone(), tys.len());
                                    let len = tys.len();
                                    arg_tys.extend(tys);
                                    for _ in 0..len {
                                        arg_exprs.push(arg);
                                    }
                                } else {
                                    self.error(
                                        s_span,
                                        format!(
                                            "Spread operator requires a tuple, got {}",
                                            inner_ty
                                        ),
                                        None,
                                    );
                                    arg_tys.push(Ty::Any);
                                    arg_exprs.push(arg);
                                    self.spread_lengths.insert(s_span.clone(), 1);
                                }
                            } else {
                                arg_tys.push(self.check_expr(arg));
                                arg_exprs.push(arg);
                            }
                        }
                        self.check_call_with_types_and_exprs(
                            callee, callee_ty, &arg_tys, &arg_exprs, span,
                        )
                    }
                    Expr::Ident(_, _) | Expr::FnExpr(_, _, _, _, _, _, _) => {
                        let callee_ty = self.check_expr(right);
                        self.check_call_with_types_and_exprs(
                            right,
                            callee_ty,
                            &[input_ty],
                            &[left.as_ref()],
                            span,
                        )
                    }
                    _ => {
                        self.error(
                            span,
                            "Right side of pipe must be a function or function call".to_string(),
                            None,
                        );
                        Ty::Any
                    }
                };

                self.pop_scope();
                ret_ty
            }
            Expr::Call(callee, args, span) => {
                let resolved_callee_name: Option<String> =
                    if let Expr::Field(obj, member, _) = callee.as_ref() {
                        if let Expr::Ident(alias, _) = obj.as_ref() {
                            self.resolve_alias_member(alias, member)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                let callee_name_ref: Option<&str> = if let Expr::Ident(name, _) = callee.as_ref() {
                    Some(name.as_str())
                } else {
                    resolved_callee_name.as_deref()
                };
                if let Some(name) = callee_name_ref {
                    if let Some(ty) = self.check_resolver_fact_write_call(name, args, span) {
                        return ty;
                    }
                    if let Some(ty) = self.check_constraint_read_call(name, args, span) {
                        return ty;
                    }
                    self.check_law_call_context(name, span);
                    if let Some(sys) = self.systems.get(name).cloned() {
                        if !sys.is_pub && is_cross_file(sys.file_id, span.file) {
                            self.error(
                                span,
                                format!("System '{}' is private", name),
                                Some(format!("Add `pub` to the declaration of '{}'", name)),
                            );
                        }
                        if !args.is_empty() {
                            self.error(
                                span,
                                format!(
                                    "System '{}' takes no explicit arguments — its parameters come from the ECS world",
                                    name
                                ),
                                Some(format!("Use `{}()` with no arguments", name)),
                            );
                        }
                        let in_pipeline = self.scopes.iter().rev().any(|s| s.in_pipeline);
                        if in_pipeline {
                            self.error(
                                span,
                                "Cannot run systems inside a pipeline".to_string(),
                                Some("Pipeline functions must be pure".to_string()),
                            );
                        }
                        return Ty::Nil;
                    }
                }
                let callee_ty = self.check_expr(callee);
                let mut arg_tys = Vec::new();
                let mut arg_exprs = Vec::new();
                for arg in args {
                    if let Expr::Spread(inner, s_span) = arg {
                        let inner_ty = self.check_expr(inner);
                        if let Ty::Tuple(tys) = inner_ty {
                            self.spread_lengths.insert(s_span.clone(), tys.len());
                            let len = tys.len();
                            arg_tys.extend(tys);
                            for _ in 0..len {
                                arg_exprs.push(arg);
                            }
                        } else {
                            self.error(
                                s_span,
                                format!("Spread operator requires a tuple, got {}", inner_ty),
                                None,
                            );
                            arg_tys.push(Ty::Any);
                            arg_exprs.push(arg);
                            self.spread_lengths.insert(s_span.clone(), 1);
                        }
                    } else {
                        if let Expr::Ident(name, arg_span) = arg {
                            if self.binding_is_unique(name) {
                                let moved_back_to_same_binding =
                                    self.current_assign_target.as_deref() == Some(name.as_str());
                                if !moved_back_to_same_binding {
                                    self.error(
                                        arg_span,
                                        format!(
                                            "Cannot alias unique binding '{}' by passing it as an argument",
                                            name
                                        ),
                                        Some(
                                            "Use the unique binding directly in-place, or move it with reassignment to the same name"
                                                .to_string(),
                                        ),
                                    );
                                }
                            }
                        }
                        arg_tys.push(self.check_expr(arg));
                        arg_exprs.push(arg);
                    }
                }
                self.check_call_with_types_and_exprs(callee, callee_ty, &arg_tys, &arg_exprs, span)
            }
            Expr::Try(inner, span) => {
                let inner_ty = self.check_expr(inner);
                match inner_ty {
                    Ty::App(name, args) if name == "Result" && args.len() == 2 => {
                        let ok_ty = args[0].clone();
                        let err_ty = args[1].clone();
                        if self.current_fn_name.is_some() {
                            self.current_fn_returns.push((
                                Ty::App("Result".to_string(), vec![Ty::Void, err_ty]),
                                span.clone(),
                            ));
                        }
                        ok_ty
                    }
                    Ty::App(name, args) if name == "Option" && args.len() == 1 => {
                        let some_ty = args[0].clone();
                        if self.current_fn_name.is_some() {
                            self.current_fn_returns.push((
                                Ty::App("Option".to_string(), vec![Ty::Void]),
                                span.clone(),
                            ));
                        }
                        some_ty
                    }
                    Ty::SumType(name) if name == "Result" => {
                        if self.current_fn_name.is_some() {
                            self.current_fn_returns
                                .push((Ty::SumType("Result".to_string()), span.clone()));
                        }
                        Ty::Any
                    }
                    Ty::SumType(name) if name == "Option" => {
                        if self.current_fn_name.is_some() {
                            self.current_fn_returns
                                .push((Ty::SumType("Option".to_string()), span.clone()));
                        }
                        Ty::Any
                    }
                    Ty::Any => Ty::Any,
                    other => {
                        self.error(
                            span,
                            format!(
                                "`?` operator can only be used on Result or Option, got {}",
                                other
                            ),
                            None,
                        );
                        Ty::Any
                    }
                }
            }
            Expr::Await(inner, span) => {
                let in_async = self.scopes.iter().rev().any(|s| s.in_async);
                if !in_async && self.current_fn_name.is_some() {
                    self.error(
                        span,
                        "`await` is only allowed inside `async` functions or handlers".to_string(),
                        Some("Mark the enclosing declaration as `async`".to_string()),
                    );
                }
                if self.scopes.iter().rev().any(|s| s.in_pipeline) {
                    self.error(
                        span,
                        "`await` is not allowed inside pipelines".to_string(),
                        Some("Move async calls outside the `|>` chain".to_string()),
                    );
                }
                let inner_ty = self.check_expr(inner);
                match inner_ty {
                    Ty::Task(ret) => *ret,
                    Ty::Any => Ty::Any,
                    other => {
                        self.error(
                            span,
                            format!("`await` expects a task value, got {}", other),
                            Some("Use `await` with an `async` call result".to_string()),
                        );
                        Ty::Any
                    }
                }
            }
            Expr::AsyncCall(callee, args, span) => {
                let callee_ty = self.check_expr(callee);
                let mut arg_tys = Vec::new();
                let mut arg_exprs = Vec::new();
                for arg in args {
                    if let Expr::Spread(inner, s_span) = arg {
                        let inner_ty = self.check_expr(inner);
                        if let Ty::Tuple(tys) = inner_ty {
                            self.spread_lengths.insert(s_span.clone(), tys.len());
                            arg_tys.extend(tys.clone());
                            for _ in 0..tys.len() {
                                arg_exprs.push(arg);
                            }
                        } else {
                            self.error(
                                s_span,
                                format!("Spread operator requires a tuple, got {}", inner_ty),
                                None,
                            );
                            arg_tys.push(Ty::Any);
                            arg_exprs.push(arg);
                            self.spread_lengths.insert(s_span.clone(), 1);
                        }
                    } else {
                        arg_tys.push(self.check_expr(arg));
                        arg_exprs.push(arg);
                    }
                }
                let out_ty = self
                    .check_call_with_types_and_exprs(callee, callee_ty, &arg_tys, &arg_exprs, span);
                if self.scopes.iter().rev().any(|s| s.in_pipeline) {
                    self.error(
                        span,
                        "`async` calls are not allowed inside pipelines".to_string(),
                        Some("Move async spawning outside the `|>` chain".to_string()),
                    );
                }
                match out_ty {
                    Ty::Task(_) => out_ty,
                    other => Ty::Task(Box::new(other)),
                }
            }
            _ => unreachable!("dispatcher selected the wrong match partition"),
        }
    }}
