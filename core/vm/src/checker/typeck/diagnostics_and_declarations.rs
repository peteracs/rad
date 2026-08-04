

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

    fn with_mixed_list_warning_suppressed<F>(&mut self, f: F) -> Ty
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

    pub(super) fn check_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Component(c) => self.check_component_decl(c),
            Decl::Resource(_) => {}
            Decl::Struct(s) => self.check_struct_decl(s),
            Decl::Intent(i) => self.check_intent_decl(i),
            Decl::Law(l) => self.check_law_decl(l),
            Decl::Resolver(r) => self.check_resolver_decl(r),
            Decl::Constraint(c) => self.check_constraint_decl(c),
            Decl::Entity(e) => self.check_entity_decl(e),
            Decl::State(s) => self.check_state_decl(s),
            Decl::System(s) => self.check_system_decl(s),
            Decl::Event(_) => {}
            Decl::Phase(p) => self.check_phase_decl(p),
            Decl::OnHandler(h) => self.check_on_handler(h),
            Decl::Migration(m) => self.check_migration_decl(m),
            Decl::Fn(f) => self.check_fn_decl(f),
            Decl::Type(t) => self.check_type_decl(t),
            Decl::Use(u) => self.check_use_decl(u),
            Decl::Test(t) => self.check_test_decl(t),
            Decl::Stmt(s) => self.check_stmt(s),
            Decl::TypeAlias(_) => {}
            Decl::Error => {}
        }
    }

    fn check_use_decl(&mut self, decl: &UseStmt) {
        if let Some(contract) = &decl.contract {
            let alias = match &decl.alias {
                Some(a) => a,
                None => {
                    self.error(
                        &decl.span,
                        "Module-level contracts require an alias (`use \"...\" as Alias : Contract`)".to_string(),
                        None,
                    );
                    return;
                }
            };

            let contract_ty = match self.structs.get(contract) {
                Some(s) => s.clone(),
                None => {
                    self.error(
                        &decl.span,
                        format!("Contract '{}' not found or is not a struct", contract),
                        None,
                    );
                    return;
                }
            };

            let imported_decls = match self.alias_decls.get(alias) {
                Some(d) => d.clone(),
                None => return,
            };

            for (field_name, expected_ty) in &contract_ty.fields {
                let mut found = false;
                for d in &imported_decls {
                    if let Some(name) = super::decl_name(d) {
                        if name == field_name && super::decl_is_pub(d) {
                            found = true;
                            // Check type
                            let actual_ty = match d {
                                Decl::Fn(f) => {
                                    let mangled = format!("__mod_{}__{}", alias, f.name);
                                    if let Some(sig) = self.functions.get(&mangled) {
                                        Ty::Fn {
                                            params: sig.params.clone(),
                                            ret: Box::new(sig.ret.clone()),
                                            purity: if sig.is_pure {
                                                FnPurity::Pure
                                            } else if sig.effects.is_readonly() {
                                                FnPurity::Readonly
                                            } else {
                                                FnPurity::Impure
                                            },
                                        }
                                    } else {
                                        Ty::Any
                                    }
                                }
                                _ => Ty::Any, // TODO: check other types
                            };

                            let expected_resolved = expected_ty.clone();
                            if expected_resolved != Ty::Any
                                && actual_ty != Ty::Any
                                && !expected_resolved.assignable_from(&actual_ty)
                            {
                                let msg = format!("Module '{}' does not satisfy contract '{}': field '{}' has type {}, expected {}",
                                        alias, contract, field_name, actual_ty, expected_resolved);
                                self.error(&decl.span, msg, None);
                            }
                            break;
                        }
                    }
                }
                if !found {
                    let msg = format!(
                        "Module '{}' does not satisfy contract '{}': missing public export '{}'",
                        alias, contract, field_name
                    );
                    self.error(&decl.span, msg, None);
                }
            }
        }
    }

    fn check_test_decl(&mut self, decl: &TestDecl) {
        self.push_scope();
        for (name, gen_expr) in &decl.generators {
            let ty = self.check_expr(gen_expr);
            self.define(name, ty, false, decl.span.clone(), false, false);
        }
        self.check_block(&decl.body);
        self.pop_scope();
    }

    fn ty_contains_fn(ty: &Ty) -> bool {
        match ty {
            Ty::Fn { .. } => true,
            Ty::List(inner) => Self::ty_contains_fn(inner),
            Ty::Tuple(elems) => elems.iter().any(Self::ty_contains_fn),
            Ty::Map(k, v) => Self::ty_contains_fn(k) || Self::ty_contains_fn(v),
            Ty::Union(variants) => variants.iter().any(Self::ty_contains_fn),
            _ => false,
        }
    }

    fn check_component_decl(&mut self, decl: &ComponentDecl) {
        let mut seen = HashMap::new();
        for field in &decl.fields {
            if let Some(prev_line) = seen.get(&field.name) {
                self.error(
                    &decl.span,
                    format!(
                        "Duplicate field '{}' in component '{}'",
                        field.name, decl.name
                    ),
                    Some(format!(
                        "Field '{}' was already defined at line {}",
                        field.name, prev_line
                    )),
                );
            }
            seen.insert(field.name.clone(), decl.span.line);
            if !decl.is_pub && self.options.strict_types && field.type_annotation.is_none() {
                self.error(
                    &decl.span,
                    format!(
                        "Strict types: component field '{}.{}' requires an explicit type annotation",
                        decl.name, field.name
                    ),
                    Some("Write it as `field: Type = default_value`".to_string()),
                );
            }
        }
        let mut seen_indexed = std::collections::HashSet::new();
        for field_name in &decl.indexed_fields {
            if !seen_indexed.insert(field_name.clone()) {
                self.error(
                    &decl.span,
                    format!(
                        "Duplicate indexed field '{}' in component '{}'",
                        field_name, decl.name
                    ),
                    None,
                );
            }
        }
        if let Some(ct) = self.components.get(&decl.name).cloned() {
            for (fname, fty) in &ct.fields {
                if Self::ty_contains_fn(fty) {
                    self.error(
                        &decl.span,
                        format!(
                            "Component field '{}.{}' cannot have a function type. Components must be plain data (Law 1: Separate Data from Logic)",
                            decl.name, fname
                        ),
                        Some("Store behavior in systems or event handlers, not in component fields".to_string()),
                    );
                }
            }
        }
    }

    fn check_struct_decl(&mut self, decl: &StructDecl) {
        let mut seen = HashMap::new();
        for field in &decl.fields {
            if let Some(prev_line) = seen.get(&field.name) {
                self.error(
                    &decl.span,
                    format!("Duplicate field '{}' in struct '{}'", field.name, decl.name),
                    Some(format!(
                        "Field '{}' was already defined at line {}",
                        field.name, prev_line
                    )),
                );
            }
            seen.insert(field.name.clone(), decl.span.line);
            if !decl.is_pub && self.options.strict_types && field.type_annotation.is_none() {
                self.error(
                    &decl.span,
                    format!(
                        "Strict types: struct field '{}.{}' requires an explicit type annotation",
                        decl.name, field.name
                    ),
                    Some("Write it as `field: Type = default_value`".to_string()),
                );
            }
        }
        if let Some(st) = self.structs.get(&decl.name).cloned() {
            for (fname, fty) in &st.fields {
                if Self::ty_contains_fn(fty) {
                    self.error(
                        &decl.span,
                        format!(
                            "Struct field '{}.{}' cannot have a function type. Structs must be plain data (Law 1: Separate Data from Logic)",
                            decl.name, fname
                        ),
                        Some("Store behavior in systems or event handlers, not in struct fields".to_string()),
                    );
                }
            }
        }
    }

    fn check_type_decl(&mut self, decl: &TypeDeclNode) {
        let mut seen_variants = HashMap::new();
        for variant in &decl.variants {
            if let Some(_prev_line) = seen_variants.get(&variant.name) {
                self.error(
                    &decl.span,
                    format!(
                        "Duplicate variant '{}' in type '{}'",
                        variant.name, decl.name
                    ),
                    None,
                );
            }
            seen_variants.insert(variant.name.clone(), decl.span.line);

            let mut seen_fields = HashMap::new();
            for (field_name, _) in &variant.fields {
                if let Some(_prev_line) = seen_fields.get(field_name) {
                    self.error(
                        &decl.span,
                        format!(
                            "Duplicate field '{}' in variant '{}::{}'",
                            field_name, decl.name, variant.name
                        ),
                        None,
                    );
                }
                seen_fields.insert(field_name.clone(), decl.span.line);
            }
        }
    }

    fn check_entity_decl(&mut self, decl: &EntityDecl) {
        for entry in &decl.components {
            match entry {
                ComponentEntry::Expr(expr) => {
                    self.check_expr(expr);
                }
                ComponentEntry::Init(comp_init) => {
                    if comp_init.comp_name.contains("::") {
                        let parts: Vec<&str> = comp_init.comp_name.split("::").collect();
                        if parts.len() == 2 {
                            let machine_name = parts[0];
                            let state_name = parts[1];
                            let resolved_machine = self.resolve_canonical_name(machine_name);

                            if let Some(machine) = self.state_machines.get(&resolved_machine) {
                                if !machine.has_state(state_name) {
                                    self.error(
                                        &comp_init.span,
                                        format!(
                                            "Unknown state '{}' in machine '{}'",
                                            state_name, machine_name
                                        ),
                                        Some(format!(
                                            "Available states: {}",
                                            machine.states.join(", ")
                                        )),
                                    );
                                }
                            } else {
                                self.error(
                                    &comp_init.span,
                                    format!("Unknown state machine '{}'", machine_name),
                                    None,
                                );
                            }
                        }
                    } else {
                        self.check_component_init(comp_init);
                    }
                }
            }
        }
        self.define(
            &decl.name,
            Ty::EntityId,
            false,
            decl.span.clone(),
            false,
            false,
        );
    }

    fn check_component_init(&mut self, init: &ComponentInit) {
        let resolved_name = self.resolve_canonical_name(&init.comp_name);

        let comp_type = match self.components.get(&resolved_name) {
            Some(ct) => ct.clone(),
            None => {
                self.error(
                    &init.span,
                    format!("Unknown component type '{}'", init.comp_name),
                    None,
                );
                return;
            }
        };

        for (field_name, field_expr) in &init.fields {
            match comp_type.field_type(field_name) {
                Some(expected_ty) => {
                    let actual_ty = self.check_expr(field_expr);
                    if !expected_ty.assignable_from(&actual_ty) {
                        self.error(
                            field_expr.span(),
                            format!(
                                "Type error in '{}.{}': expected {}, got {}",
                                init.comp_name, field_name, expected_ty, actual_ty
                            ),
                            self.type_mismatch_hint(expected_ty, &actual_ty),
                        );
                    }
                }
                None => {
                    self.error(
                        field_expr.span(),
                        format!(
                            "Unknown field '{}' on component '{}'",
                            field_name, init.comp_name
                        ),
                        Some(format!(
                            "Available fields: {}",
                            comp_type
                                .fields
                                .iter()
                                .map(|(n, _)| n.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    );
                }
            }
        }

        // Required (annotation-only) fields have no default to fall back
        // on — every construction site must provide them, entity literals
        // included.
        for (fname, _) in &comp_type.fields {
            if !init.fields.iter().any(|(n, _)| n == fname)
                && !self.field_is_defaultable(&resolved_name, fname)
            {
                self.error(
                    &init.span,
                    format!(
                        "Missing field '{}' in component '{}'",
                        fname, init.comp_name
                    ),
                    Some("fields with declared defaults may be omitted".to_string()),
                );
            }
        }
    }

    fn check_state_decl(&mut self, decl: &StateDecl) {
        let machine = self.state_machines.get(&decl.name).cloned();
        let machine = match machine {
            Some(m) => m,
            None => return,
        };

        for state_def in &decl.states {
            for (_, target, guard) in &state_def.transitions {
                if !machine.has_state(target) {
                    self.error(
                        &state_def.span,
                        format!(
                            "State '{}' in machine '{}' transitions to unknown state '{}'",
                            state_def.name, decl.name, target
                        ),
                        Some(format!("Known states: {}", machine.states.join(", "))),
                    );
                }
                if let Some(guard_expr) = guard {
                    self.check_expr(guard_expr);
                }
            }
        }
    }

    fn check_system_decl(&mut self, decl: &SystemDecl) {
        for dep in &decl.after {
            let resolved_dep = self.resolve_canonical_name(dep);
            if !self.systems.contains_key(&resolved_dep) {
                self.error(
                    &decl.span,
                    format!(
                        "System '{}' declares 'after {}', but '{}' is not a known system",
                        decl.name, dep, dep
                    ),
                    None,
                );
            }
        }
        for dep in &decl.before {
            let resolved_dep = self.resolve_canonical_name(dep);
            if !self.systems.contains_key(&resolved_dep) {
                self.error(
                    &decl.span,
                    format!(
                        "System '{}' declares 'before {}', but '{}' is not a known system",
                        decl.name, dep, dep
                    ),
                    None,
                );
            }
        }

        self.push_scope();
        if let Some(scope) = self.scopes.last_mut() {
            scope.in_system = Some(decl.name.clone());
        }

        self.system_params.clear();
        let mut requested_types = std::collections::HashSet::new();
        for (param_name, is_mut, comp_type_name) in &decl.params {
            let resolved_comp = self.resolve_canonical_name(comp_type_name);
            if self.structs.contains_key(&resolved_comp) {
                self.error(
                    &decl.span,
                    format!(
                        "System '{}' parameter '{}' uses struct '{}', but system parameters must be components",
                        decl.name, param_name, comp_type_name
                    ),
                    Some(format!("Declare '{}' with `component` instead of `struct` to use it in systems", comp_type_name)),
                );
            } else if !self.components.contains_key(&resolved_comp)
                && !self.resources.contains_key(&resolved_comp)
            {
                self.error(
                    &decl.span,
                    format!(
                        "System '{}' queries unknown component/resource '{}'",
                        decl.name, comp_type_name
                    ),
                    None,
                );
            }
            if !requested_types.insert(resolved_comp.clone()) {
                self.error(
                    &decl.span,
                    format!(
                        "System '{}' queries '{}' multiple times",
                        decl.name, comp_type_name
                    ),
                    Some("An entity can only have one instance of each component type. Remove the duplicate parameter.".to_string()),
                );
            }
            self.define(
                param_name,
                Ty::Component(resolved_comp.clone()),
                *is_mut,
                decl.span.clone(),
                false,
                true,
            );
            self.system_params
                .insert(param_name.clone(), (comp_type_name.clone(), *is_mut));
        }

        // `accum` params (dogfood seq 83 IDEA 02): the batch merge folds
        // per-field numeric deltas, so the contract is checkable up front —
        // only a resource can be folded, and only int/float fields fold.
        // Reject violations here instead of merging garbage at runtime.
        for (param_name, _, comp_type_name) in &decl.params {
            if !decl.accum_params.contains(param_name) {
                continue;
            }
            let resolved = self.resolve_canonical_name(comp_type_name);
            if self.resources.contains_key(&resolved) {
                let bad: Vec<String> = self
                    .resources
                    .get(&resolved)
                    .map(|res| {
                        res.fields
                            .iter()
                            .filter(|(_, ty)| !matches!(ty, Ty::Int | Ty::Float))
                            .map(|(n, _)| n.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                if !bad.is_empty() {
                    self.error(
                        &decl.span,
                        format!(
                            "System '{}' declares '{}: accum {}', but accum folds numeric deltas and '{}' has non-numeric field(s): {}",
                            decl.name,
                            param_name,
                            comp_type_name,
                            comp_type_name,
                            bad.join(", ")
                        ),
                        Some(
                            "accum resources may only have int and float fields; aggregate other shapes through an event handler instead"
                                .to_string(),
                        ),
                    );
                }
            } else if self.components.contains_key(&resolved)
                || self.structs.contains_key(&resolved)
            {
                self.error(
                    &decl.span,
                    format!(
                        "System '{}' declares '{}: accum {}', but `accum` is only valid on resource parameters",
                        decl.name, param_name, comp_type_name
                    ),
                    Some(format!(
                        "Declare '{}' with `resource`, or use `mut` for a per-entity component parameter",
                        comp_type_name
                    )),
                );
            }
        }

        // `self` is the entity the system is currently visiting — the
        // compiler has always bound it (compile_system_decl pushes the eid
        // after the params); the checker now agrees so system bodies can
        // emit events about their own entity or call require(self, Other).
        self.define("self", Ty::EntityId, false, decl.span.clone(), false, false);

        self.check_block(&decl.body);

        let mut sys_mut_params = std::collections::HashSet::new();
        for (param_name, is_mut, _) in &decl.params {
            if *is_mut {
                sys_mut_params.insert(param_name.clone());
            }
        }
        let breach = self.find_system_sim_breach(&decl.body, sys_mut_params.clone());
        // Lenient pass for simulate_par: rand_* is legal there (per-fork
        // explicit seeds make it deterministic).
        self.sim_breach_allow_rand = true;
        let breach_par = self.find_system_sim_breach(&decl.body, sys_mut_params);
        self.sim_breach_allow_rand = false;
        let resolved_sys_name = self.resolve_canonical_name(&decl.name);
        let resource_flags: Vec<bool> = decl
            .params
            .iter()
            .map(|(_, _, comp_type)| {
                let resolved = self.resolve_canonical_name(comp_type);
                self.resources.contains_key(&resolved)
            })
            .collect();
        if let Some(sys_type) = self.systems.get_mut(&resolved_sys_name) {
            sys_type.simulation_breach = breach;
            sys_type.simulation_breach_par = breach_par;
            for (param, &is_res) in sys_type.params.iter_mut().zip(resource_flags.iter()) {
                param.is_resource = is_res;
            }
        }

        self.pop_scope();
        self.system_params.clear();
    }

    /// `migrate Health(old) { return Health { … } }` — schema migration
    /// (list item #5). The target must be a declared component or resource;
    /// `old` binds the persisted fields as `map<str, any>` because the old
    /// shape no longer exists as a type.
    fn check_migration_decl(&mut self, m: &MigrationDecl) {
        let resolved = self.resolve_canonical_name(&m.component);
        if !self.components.contains_key(&resolved) && !self.resources.contains_key(&resolved) {
            self.error(
                &m.span,
                format!(
                    "Migration for unknown component or resource '{}'",
                    m.component
                ),
                Some("migrations target a declared `component` or `resource`".to_string()),
            );
        }

        self.push_scope();
        self.define(
            &m.param_name,
            Ty::Map(Box::new(Ty::Str), Box::new(Ty::Any)),
            false,
            m.span.clone(),
            false,
            true,
        );
        // `migrate X(old, from_version)` — the save's declared schema
        // version for X, an int (0 for versionless saves; dogfood seq 69).
        if let Some(vp) = &m.version_param {
            self.define(vp, Ty::Int, false, m.span.clone(), false, true);
        }
        self.check_block(&m.body);
        self.pop_scope();
    }

    fn check_on_handler(&mut self, handler: &OnHandler) {
        let resolved_event = self.resolve_canonical_name(&handler.event_name);

        if let Some(evt) = self.events.get(&resolved_event) {
            if !evt.is_pub && is_cross_file(evt.file_id, handler.span.file) {
                self.error(
                    &handler.span,
                    format!("Event '{}' is private", handler.event_name),
                    Some(format!(
                        "Add `pub` to the declaration of '{}'",
                        handler.event_name
                    )),
                );
            }
        } else {
            self.error(
                &handler.span,
                format!("Handler for unknown event '{}'", handler.event_name),
                None,
            );
        }

        self.push_scope();
        if let Some(scope) = self.scopes.last_mut() {
            scope.in_async = handler.is_async;
        }
        let param_ty = if self.events.contains_key(&resolved_event) {
            Ty::Event(resolved_event.clone())
        } else {
            Ty::Any
        };
        self.define(
            &handler.param_name,
            param_ty,
            false,
            handler.span.clone(),
            false,
            true,
        );
        self.check_block(&handler.body);
        self.pop_scope();
    }}