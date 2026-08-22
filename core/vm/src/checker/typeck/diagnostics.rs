impl Checker {
    /// Type-check a `system::…` path against declared `system`s.
    pub(super) fn check_system_ref_path(&mut self, path: &[String], span: &Span) -> Ty {
        if path.is_empty() {
            self.error(span, "Invalid empty `system::` reference".to_string(), None);
            return Ty::SystemRef;
        }
        let q = crate::simulate_syntax::system_ref_qualified_string(path);
        let resolved = self.resolve_canonical_name(&q);
        if self.systems.contains_key(&resolved) {
            Ty::SystemRef
        } else {
            self.error(
                span,
                format!("Unknown system '{}' in system reference", q),
                Some(
                    "Use `system::Name` where `Name` is a declared `system` (check module path and spelling)"
                        .to_string(),
                ),
            );
            Ty::SystemRef
        }
    }

    pub(crate) fn with_mixed_list_warning_suppressed<F>(&mut self, f: F) -> Ty
    where
        F: FnOnce(&mut Checker) -> Ty,
    {
        self.suppress_mixed_list_warnings += 1;
        let out = f(self);
        self.suppress_mixed_list_warnings -= 1;
        out
    }

    pub(super) fn type_mismatch_hint(&self, expected: &Ty, actual: &Ty) -> Option<String> {
        // Purity-only fn mismatch: the shapes agree but the parameter demands
        // a stricter callback (fn-typed params of effect-annotated fns are
        // promoted to pure/readonly fn types). Without this hint the two
        // types print almost identically and the error reads like nonsense.
        if let (
            Ty::Fn {
                params: exp_params,
                purity: exp_purity,
                ..
            },
            Ty::Fn {
                params: act_params,
                purity: act_purity,
                ..
            },
        ) = (expected, actual)
        {
            if exp_params.len() == act_params.len() && act_purity > exp_purity {
                let requirement = match exp_purity {
                    FnPurity::Pure => {
                        "a pure function — declare it `pure fn` (or pass a closure with no side effects)"
                    }
                    FnPurity::Readonly => {
                        "a pure or readonly function — declare it `pure fn` or `readonly fn` (or pass a closure that at most reads the world)"
                    }
                    FnPurity::Impure => unreachable!("nothing outranks Impure"),
                };
                return Some(format!(
                    "This callback parameter belongs to an effect-restricted function, so the argument must be {}",
                    requirement
                ));
            }
        }
        if *expected == Ty::Any || *actual == Ty::Any {
            return None;
        }
        suggest_type_fix(&format!("{}", expected), &format!("{}", actual))
    }

    /// Build a hint that explains the purity breach chain and suggests which
    /// function(s) to annotate with `pure fn`.
    fn build_purity_fix_hint(&self, fn_name: &str, breach_reason: &str) -> String {
        let mut fns_to_annotate = Vec::new();
        self.collect_impure_chain(fn_name, &mut fns_to_annotate, 0);

        let chain_explanation = format!("'{}' is not pure because it {}", fn_name, breach_reason);

        if fns_to_annotate.is_empty() {
            format!(
                "{}. If the function truly has no side effects, declare it as `pure fn {}`",
                chain_explanation, fn_name
            )
        } else if fns_to_annotate.len() == 1 {
            format!(
                "{}. Add `pure` to fix: `pure fn {}`",
                chain_explanation, fns_to_annotate[0]
            )
        } else {
            let fixes: Vec<String> = fns_to_annotate
                .iter()
                .map(|n| format!("`pure fn {}`", n))
                .collect();
            format!(
                "{}. Add `pure` to the chain: {}",
                chain_explanation,
                fixes.join(", then ")
            )
        }
    }

    /// Walk the purity breach chain and collect function names that would need
    /// `pure fn` to make the root function pure. Stops at impure builtins
    /// (those can't be annotated).
    /// Whether `field` of `type_name` may be omitted from a literal because
    /// its declaration carries a usable default (the compiler fills it in).
    fn field_is_defaultable(&self, type_name: &str, field: &str) -> bool {
        self.defaultable_fields
            .get(type_name)
            .is_some_and(|s| s.contains(field))
    }

    fn collect_impure_chain(&self, fn_name: &str, out: &mut Vec<String>, depth: usize) {
        if depth > 8 {
            return;
        }
        if let Some(reason) = self.purity_breach_reasons.get(fn_name) {
            if reason.starts_with("calls '") {
                if let Some(callee) = reason
                    .strip_prefix("calls '")
                    .and_then(|r| r.split('\'').next())
                {
                    if !is_impure_builtin(callee)
                        && self.functions.contains_key(callee)
                        && !out.contains(&callee.to_string())
                    {
                        self.collect_impure_chain(callee, out, depth + 1);
                    }
                }
            }
            if !out.contains(&fn_name.to_string()) {
                out.push(fn_name.to_string());
            }
        }
    }

    fn scope_binding_hint_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        for sc in &self.scopes {
            for k in sc.bindings.keys() {
                if !out.contains(k) {
                    out.push(k.clone());
                }
            }
        }
        out
    }

    fn callable_name_hint_candidates(&self) -> Vec<String> {
        let mut out: Vec<String> = self.functions.keys().cloned().collect();
        for b in Builtin::ALL {
            let n = b.name().to_string();
            if !out.contains(&n) {
                out.push(n);
            }
        }
        if !out.iter().any(|s| s == "emit") {
            out.push("emit".to_string());
        }
        for sc in &self.scopes {
            for (k, b) in &sc.bindings {
                if matches!(&b.ty, Ty::Fn { .. }) && !out.contains(k) {
                    out.push(k.clone());
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

}
