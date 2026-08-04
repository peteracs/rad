impl Checker {

fn check_expr_control_and_functions(&mut self, expr: &Expr) -> Ty {
        match expr {
            Expr::SystemRef(path, span) => self.check_system_ref_path(path, span),
            Expr::MatchExpr(m, _) => self.check_match(m),
            Expr::IfExpr(cond, then_e, else_e, span) => {
                let cond_ty = self.check_expr(cond);
                if cond_ty != Ty::Bool && cond_ty != Ty::Any {
                    self.error(
                        cond.span(),
                        format!("if-expression condition must be bool, got {}", cond_ty),
                        None,
                    );
                }
                let then_ty = self.check_expr(then_e);
                let else_ty = self.check_expr(else_e);
                if then_ty.assignable_from(&else_ty) {
                    then_ty
                } else if else_ty.assignable_from(&then_ty) {
                    else_ty
                } else {
                    if then_ty != Ty::Any && else_ty != Ty::Any {
                        self.error(
                            span,
                            format!(
                                "if-expression branches have incompatible types: {} vs {}",
                                then_ty, else_ty
                            ),
                            Some("Both branches must produce the same type".to_string()),
                        );
                    }
                    Ty::Any
                }
            }
            Expr::QueryExpr(q, span) => {
                for (comp, is_mut) in &q.components {
                    if *is_mut {
                        self.error(
                            span,
                            "Mutable queries are only allowed directly in a `for` loop".to_string(),
                            Some("Use `for h in query { mut Health } { ... }`".to_string()),
                        );
                    }
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
                for sel in &q.select {
                    if !q.components.iter().any(|(c, _)| c == sel) {
                        self.error(
                            span,
                            format!("Selected component '{}' is not in the query set", sel),
                            Some(format!("Add '{}' to the query {{ ... }} block", sel)),
                        );
                    }
                }
                if let Some(filter) = &q.filter {
                    self.push_scope();
                    for (comp, _) in &q.components {
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
                if q.select.is_empty() {
                    Ty::List(Box::new(Ty::EntityId))
                } else if q.select.len() == 1 {
                    Ty::List(Box::new(Ty::Component(q.select[0].clone())))
                } else {
                    let tys = q.select.iter().map(|s| Ty::Component(s.clone())).collect();
                    Ty::List(Box::new(Ty::Tuple(tys)))
                }
            }
            Expr::FnExpr(
                params,
                param_muts,
                param_types,
                param_destructures,
                return_type,
                body,
                span,
            ) => {
                if self.options.strict_types {
                    for (idx, param) in params.iter().enumerate() {
                        if param_types.get(idx).and_then(|t| t.as_ref()).is_none() {
                            let display_name = if param_destructures
                                .get(idx)
                                .and_then(|d| d.as_ref())
                                .is_some()
                            {
                                format!("destructured parameter at position {}", idx)
                            } else {
                                format!("'{}'", param)
                            };
                            self.error(
                                span,
                                format!(
                                    "Strict types: anonymous function parameter {} needs a type annotation",
                                    display_name
                                ),
                                Some("Add a type, e.g. `fn(x: int) -> int { ... }`".to_string()),
                            );
                        }
                    }
                    if return_type.is_none() {
                        self.error(
                            span,
                            "Strict types: anonymous functions need an explicit return type"
                                .to_string(),
                            Some("Use `fn(...) -> Type { ... }`".to_string()),
                        );
                    }
                }
                self.validate_fn_param_alignment(
                    "<anonymous function>",
                    params.len(),
                    param_types.len(),
                    span,
                );
                {
                    let mut seen_params = std::collections::HashSet::new();
                    for (idx, param) in params.iter().enumerate() {
                        if !seen_params.insert(param.clone()) {
                            self.error(
                                span,
                                format!("Duplicate parameter '{}' in anonymous function", param),
                                None,
                            );
                        }
                        if let Some(bindings) = param_destructures.get(idx).and_then(|d| d.as_ref())
                        {
                            for binding in bindings {
                                if *binding != "_" && !seen_params.insert(binding.clone()) {
                                    self.error(
                                        span,
                                        format!(
                                            "Duplicate parameter '{}' in anonymous function",
                                            binding
                                        ),
                                        None,
                                    );
                                }
                            }
                        }
                    }
                }
                let saved_fn_name = self.current_fn_name.clone();
                let saved_returns = std::mem::take(&mut self.current_fn_returns);
                let anon_scope_base = self.scopes.len();
                self.push_scope();
                self.anon_fn_scope_bases.push(anon_scope_base);
                for (idx, param) in params.iter().enumerate() {
                    let ty = param_types
                        .get(idx)
                        .and_then(|ann| ann.as_ref())
                        .map(|te| self.resolve_type_expr(te, span))
                        .unwrap_or(Ty::Any);
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
                let block_ty = self.check_block(body);
                if !self.block_diverges(body) {
                    if let Some(Stmt::Expr(expr_stmt)) = body.stmts.last() {
                        self.current_fn_returns
                            .push((block_ty, expr_stmt.span.clone()));
                    } else {
                        self.current_fn_returns.push((Ty::Nil, span.clone()));
                    }
                }
                let expected_ret = return_type
                    .as_ref()
                    .map(|ret_ann| self.resolve_type_expr(ret_ann, span));
                let inferred_ret = self.merge_return_types(span, expected_ret.as_ref());
                let ret_ty = expected_ret.unwrap_or(inferred_ret);
                self.anon_fn_scope_bases.pop();
                self.pop_scope();
                self.current_fn_name = saved_fn_name;
                self.current_fn_returns = saved_returns;
                Ty::Fn {
                    params: params
                        .iter()
                        .enumerate()
                        .map(|(idx, _)| {
                            param_types
                                .get(idx)
                                .and_then(|ann| ann.as_ref())
                                .map(|te| self.type_expr_or_any(te))
                                .unwrap_or(Ty::Any)
                        })
                        .collect(),
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
            Expr::EntityLiteral(name, components, _span) => {
                if let Some(name_expr) = name {
                    self.check_expr(name_expr);
                }
                for entry in components {
                    match entry {
                        ComponentEntry::Init(ci) => self.check_component_init(ci),
                        ComponentEntry::Expr(expr) => {
                            self.check_expr(expr);
                        }
                    }
                }
                Ty::EntityId
            }
            Expr::Error(_) => Ty::Any,
            _ => unreachable!("dispatcher selected the wrong match partition"),
        }
    }

    fn check_binary_op(&mut self, lt: &Ty, op: &BinOp, rt: &Ty, span: &Span) -> Ty {
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let mut left = self.resolve_ty(lt);
                let mut right = self.resolve_ty(rt);
                if matches!(op, BinOp::Add) {
                    if left == Ty::Str && right == Ty::Str {
                        return Ty::Str;
                    }
                    if let (Ty::List(ref l_inner), Ty::List(ref r_inner)) = (&left, &right) {
                        if !l_inner.assignable_from(r_inner)
                            && !r_inner.assignable_from(l_inner)
                            && **l_inner != Ty::Any
                            && **r_inner != Ty::Any
                        {
                            self.error(
                                span,
                                format!(
                                    "Cannot concatenate list<{}> and list<{}>: incompatible element types",
                                    l_inner, r_inner
                                ),
                                None,
                            );
                        }
                        if r_inner.assignable_from(l_inner) {
                            return right;
                        }
                        return left;
                    }
                }
                if matches!(op, BinOp::Mul)
                    && ((left == Ty::Str && right == Ty::Int)
                        || (left == Ty::Int && right == Ty::Str))
                {
                    return Ty::Str;
                }
                // Element-wise tuple math (the vector dialect): tuple±tuple
                // of matching arity, and scalar broadcast on * and /.
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
                    if let (Ty::Tuple(xs), Ty::Tuple(ys)) = (&left, &right) {
                        if xs.len() != ys.len() {
                            self.error(
                                span,
                                format!(
                                    "Tuple arity mismatch: ({}) vs ({}) elements",
                                    xs.len(),
                                    ys.len()
                                ),
                                None,
                            );
                            return Ty::Any;
                        }
                        let elems = xs
                            .iter()
                            .zip(ys.iter())
                            .map(|(x, y)| {
                                if *x == Ty::Float || *y == Ty::Float {
                                    Ty::Float
                                } else if *x == Ty::Int && *y == Ty::Int {
                                    Ty::Int
                                } else {
                                    Ty::Any
                                }
                            })
                            .collect();
                        return Ty::Tuple(elems);
                    }
                    // scalar broadcast: tuple op scalar for all four ops
                    // (`center - reach` inflates a point on every axis);
                    // scalar-on-the-left only for the commutative ones
                    if let (Ty::Tuple(xs), s) = (&left, &right) {
                        if s.is_numeric() {
                            return Ty::Tuple(broadcast_tuple_elems(xs, s));
                        }
                    }
                    if let (s, Ty::Tuple(ys)) = (&left, &right) {
                        if s.is_numeric() && matches!(op, BinOp::Mul | BinOp::Add) {
                            return Ty::Tuple(broadcast_tuple_elems(ys, s));
                        }
                    }
                }
                if let (Ty::Var(id), other) = (&left, &right) {
                    if other.is_numeric() {
                        let _ = self.subst.unify(&Ty::Var(*id), other);
                        left = self.resolve_ty(&left);
                        right = self.resolve_ty(&right);
                    }
                }
                if let (other, Ty::Var(id)) = (&left, &right) {
                    if other.is_numeric() {
                        let _ = self.subst.unify(&Ty::Var(*id), other);
                        left = self.resolve_ty(&left);
                        right = self.resolve_ty(&right);
                    }
                }
                if left.is_numeric() && right.is_numeric() {
                    if left == Ty::Float || right == Ty::Float {
                        Ty::Float
                    } else {
                        Ty::Int
                    }
                } else if left == Ty::Any || right == Ty::Any {
                    Ty::Any
                } else {
                    // The single most common newcomer trip: "text " + n.
                    // rad never auto-converts — say what to write instead.
                    let str_num_mix = (left == Ty::Str && right.is_numeric())
                        || (right == Ty::Str && left.is_numeric());
                    let hint = if *op == BinOp::Add && str_num_mix {
                        Some(
                            "Strings never auto-convert: write f\"...{x}...\" or str(x) to make it explicit"
                                .to_string(),
                        )
                    } else {
                        None
                    };
                    self.error(
                        span,
                        format!("Operator {:?} not defined for {} and {}", op, left, right),
                        hint,
                    );
                    Ty::Any
                }
            }
            BinOp::Eq | BinOp::Ne => {
                let comparable = lt == rt
                    || *lt == Ty::Any
                    || *rt == Ty::Any
                    || (lt.is_numeric() && rt.is_numeric())
                    || lt.assignable_from(rt)
                    || rt.assignable_from(lt);
                if !comparable {
                    self.error(
                        span,
                        format!("Cannot compare {} and {} with {:?}", lt, rt, op),
                        Some("Operands must be compatible or both numeric".to_string()),
                    );
                }
                Ty::Bool
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let mut left = self.resolve_ty(lt);
                let mut right = self.resolve_ty(rt);
                if let (Ty::Var(id), other) = (&left, &right) {
                    if other.is_numeric() {
                        let _ = self.subst.unify(&Ty::Var(*id), other);
                        left = self.resolve_ty(&left);
                        right = self.resolve_ty(&right);
                    }
                }
                if let (other, Ty::Var(id)) = (&left, &right) {
                    if other.is_numeric() {
                        let _ = self.subst.unify(&Ty::Var(*id), other);
                        left = self.resolve_ty(&left);
                        right = self.resolve_ty(&right);
                    }
                }
                if (left.is_numeric() && right.is_numeric()) || left == Ty::Any || right == Ty::Any
                {
                    Ty::Bool
                } else {
                    self.error(
                        span,
                        format!("Cannot compare {} and {} with {:?}", left, right, op),
                        None,
                    );
                    Ty::Bool
                }
            }
            BinOp::And | BinOp::Or => {
                if *lt != Ty::Bool && *lt != Ty::Any {
                    self.error(
                        span,
                        format!("Left operand of {:?} must be bool, got {}", op, lt),
                        None,
                    );
                }
                if *rt != Ty::Bool && *rt != Ty::Any {
                    self.error(
                        span,
                        format!("Right operand of {:?} must be bool, got {}", op, rt),
                        None,
                    );
                }
                Ty::Bool
            }
            BinOp::Is => Ty::Bool,
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                let mut left = self.resolve_ty(lt);
                let mut right = self.resolve_ty(rt);
                if let (Ty::Var(id), other) = (&left, &right) {
                    if *other == Ty::Int {
                        let _ = self.subst.unify(&Ty::Var(*id), other);
                        left = self.resolve_ty(&left);
                        right = self.resolve_ty(&right);
                    }
                }
                if let (other, Ty::Var(id)) = (&left, &right) {
                    if *other == Ty::Int {
                        let _ = self.subst.unify(&Ty::Var(*id), other);
                        left = self.resolve_ty(&left);
                        right = self.resolve_ty(&right);
                    }
                }
                if (left == Ty::Int || left == Ty::Any) && (right == Ty::Int || right == Ty::Any) {
                    Ty::Int
                } else if *op == BinOp::Shl && matches!(left, Ty::List(_)) {
                    self.error(
                        span,
                        format!("'<<' as an expression is an int left shift, got {} and {}", left, right),
                        Some("List append `xs << v` is a statement; as an expression use `push(xs, v)`. Comparison values need parens: `xs << (a > b)`".to_string()),
                    );
                    Ty::Int
                } else {
                    self.error(
                        span,
                        format!(
                            "Bitwise operator {:?} requires int operands, got {} and {}",
                            op, left, right
                        ),
                        Some("Bitwise ops work on integers only; floats have no stable bit layout here".to_string()),
                    );
                    Ty::Int
                }
            }
        }
    }

    pub(super) fn validate_fn_param_alignment(
        &mut self,
        ctx: &str,
        params_len: usize,
        param_types_len: usize,
        span: &Span,
    ) {
        if params_len != param_types_len {
            self.error(
                span,
                format!(
                    "Internal AST invariant violated: {} has {} params but {} param type entries",
                    ctx, params_len, param_types_len
                ),
                Some("Parser should keep parameter names and type annotations aligned".to_string()),
            );
        }
    }

    fn check_effect_boundary(
        &mut self,
        callee_name: &str,
        callee_effects: &EffectSet,
        span: &Span,
    ) {
        let ctx = &self.scopes.last().unwrap().effect_context;
        if ctx == &EffectSet::unrestricted() {
            return;
        }
        if !callee_effects.is_subset_of(ctx) {
            self.error(
                span,
                format!(
                    "Effect violation: function '{}' requires {} effects, but current context only allows {}",
                    callee_name, callee_effects, ctx
                ),
                Some(format!(
                    "Either widen the caller's effect annotation or move this call outside the {} context",
                    ctx
                )),
            );
        }
    }

    /// A function VALUE (fn-typed local/param, or an `any`-typed callee)
    /// carries no effect row — only its type's purity rank. Mirror the
    /// named-fn rule above: a pure value is callable anywhere, a readonly
    /// value needs a context that allows the readonly effect, and anything
    /// else is treated as requiring unrestricted effects. Closes the
    /// higher-order laundering hole where `pure fn take(cb: fn() -> any)
    /// { cb() }` compiled and ran an impure callback (set_resource included)
    /// under a pure annotation.
    fn check_fn_value_call_effect_boundary(&mut self, desc: &str, purity: FnPurity, span: &Span) {
        let ctx = self.scopes.last().unwrap().effect_context.clone();
        match purity {
            FnPurity::Pure => return,
            FnPurity::Readonly => {
                if EffectSet::single(Effect::ReadECS).is_subset_of(&ctx) {
                    return;
                }
            }
            FnPurity::Impure => {
                if ctx == EffectSet::unrestricted() {
                    return;
                }
            }
        }
        let (msg, hint) = if purity == FnPurity::Readonly {
            (
                format!(
                    "Effect violation: cannot call {} here — a readonly function value requires the readonly effect, but the current context only allows {}",
                    desc, ctx
                ),
                "Widen the enclosing effect annotation to include `readonly`, or take a `pure fn(...)`-typed callback instead".to_string(),
            )
        } else {
            (
                format!(
                    "Effect violation: cannot call {} here — its effects cannot be verified, but the current context only allows {}",
                    desc, ctx
                ),
                "Only pure function values may be called in an effect-restricted context. Accept the callback as a fn-typed parameter of this effect-annotated function (callers must then pass a pure function), or widen the enclosing effect annotation"
                    .to_string(),
            )
        };
        self.error(span, msg, Some(hint));
    }

    fn check_builtin_effect_boundary(&mut self, name: &str, span: &Span) {
        use super::diagnostics::builtin_required_effects;
        let ctx = &self.scopes.last().unwrap().effect_context;
        if ctx == &EffectSet::unrestricted() {
            return;
        }
        let required = builtin_required_effects(name);
        let forbidden = ctx.forbidden_in(&required);
        if !forbidden.is_empty() {
            let effects_str: Vec<_> = forbidden.iter().map(|e| format!("{}", e)).collect();
            self.error(
                span,
                format!(
                    "Effect violation: '{}' requires {} effect(s), but current context is {}",
                    name,
                    effects_str.join(", "),
                    ctx
                ),
                Some(format!(
                    "Add the required effect(s) to the enclosing function's annotation, e.g. `{} fn`",
                    effects_str.join(" ")
                )),
            );
        }
    }

    /// A teaching signature for arity errors: the curated/generated builtin
    /// table, or the user function's declared names and types. Errors that
    /// count arguments without saying what they are send people to the docs
    /// for information the checker already had.
    pub(super) fn signature_hint(&self, name: &str) -> Option<String> {
        if let Some(sig) = self.functions.get(name) {
            // user-declared functions carry parameter names
            if let Some(names) = self.fn_param_names.get(name) {
                let params: Vec<String> = sig
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, t)| match names.get(i) {
                        Some(n) => format!("{}: {}", n, t),
                        None => format!("{}", t),
                    })
                    .collect();
                return Some(format!(
                    "signature: fn {}({}) -> {}",
                    name,
                    params.join(", "),
                    sig.ret
                ));
            }
        }
        crate::builtins::builtin_signature_help(name).map(|s| format!("signature: {}", s))
    }

    pub(super) fn validate_call_args(
        &mut self,
        name: &str,
        params: &[Ty],
        arg_tys: &[Ty],
        span: &Span,
    ) {
        if params.len() != arg_tys.len() {
            let hint = self.signature_hint(name);
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
            return;
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
    }
}

/// Element types of `tuple OP scalar` broadcast: float scalar floats
/// everything, int scalar preserves each element's own type.
fn broadcast_tuple_elems(xs: &[Ty], scalar: &Ty) -> Vec<Ty> {
    xs.iter()
        .map(|x| {
            if *x == Ty::Float || *scalar == Ty::Float {
                Ty::Float
            } else if *x == Ty::Int && *scalar == Ty::Int {
                Ty::Int
            } else {
                Ty::Any
            }
        })
        .collect()
}
