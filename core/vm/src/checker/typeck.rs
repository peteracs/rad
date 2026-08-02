use std::collections::HashMap;

use super::diagnostics::{
    ignored_immutable_transform_name, is_builtin, is_impure_builtin, is_readonly_builtin,
    suggest_did_you_mean, suggest_type_fix,
};
use super::is_cross_file;
use super::*;
use crate::ast::*;
use crate::simulate_syntax::{self, SystemsListForm};
use crate::types::*;
use crate::value::Builtin;

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
    }

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
    }

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
        let in_loop = self.scopes.iter().rev().any(|s| s.in_loop);
        if !in_loop {
            self.error(
                &stmt.span,
                "'break' used outside of a loop".to_string(),
                None,
            );
        }
    }

    fn check_continue(&mut self, stmt: &ContinueStmt) {
        let in_loop = self.scopes.iter().rev().any(|s| s.in_loop);
        if !in_loop {
            self.error(
                &stmt.span,
                "'continue' used outside of a loop".to_string(),
                None,
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
    }

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
            Expr::Field(obj, field_name, span) => {
                if let Expr::Ident(alias_name, _) = obj.as_ref() {
                    if self.lookup(alias_name).is_none()
                        && self.module_aliases.contains_key(alias_name)
                    {
                        if let Some(mangled) = self.resolve_alias_member(alias_name, field_name) {
                            return self.check_expr(&Expr::Ident(mangled, span.clone()));
                        }
                        let is_let_member = self.alias_decls.get(alias_name).is_some_and(|decls| {
                            decls.iter().any(|d| {
                                matches!(d, Decl::Stmt(Stmt::Let(l))
                                    if l.names.first().map(|s| s.as_str()) == Some(field_name))
                            })
                        });
                        if is_let_member {
                            self.error(
                                span,
                                format!(
                                    "let constant '{}' is not accessible through module alias '{}'",
                                    field_name, alias_name
                                ),
                                Some("Top-level lets export through a bare `use` import; or wrap the constant in a pub fn".to_string()),
                            );
                            return Ty::Any;
                        }
                        let is_private = self.alias_decls.get(alias_name).is_some_and(|decls| {
                            decls
                                .iter()
                                .any(|d| super::decl_name(d) == Some(field_name))
                        });
                        if is_private {
                            self.error(
                                span,
                                format!(
                                    "'{}' is private in module alias '{}'",
                                    field_name, alias_name
                                ),
                                Some(format!(
                                    "Add `pub` to the declaration of '{}' in the imported module",
                                    field_name
                                )),
                            );
                        } else {
                            self.error(
                                span,
                                format!(
                                    "Module alias '{}' has no member '{}'",
                                    alias_name, field_name
                                ),
                                None,
                            );
                        }
                        return Ty::Any;
                    }
                }
                let obj_ty = self.check_expr(obj);
                match &obj_ty {
                    Ty::Component(comp_name) => {
                        if let Some(comp) = self.components.get(comp_name) {
                            match comp.field_type(field_name) {
                                Some(ty) => ty.clone(),
                                None => {
                                    self.error(
                                        span,
                                        format!(
                                            "No field '{}' on component '{}'",
                                            field_name, comp_name
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
                                    Ty::Any
                                }
                            }
                        } else if let Some(res) = self.resources.get(comp_name) {
                            // Resource values (e.g. from `res(R)`) share the
                            // Component type shape; validate fields the same way.
                            match res.fields.iter().find(|(n, _)| n == field_name) {
                                Some((_, ty)) => ty.clone(),
                                None => {
                                    self.error(
                                        span,
                                        format!(
                                            "No field '{}' on resource '{}'",
                                            field_name, comp_name
                                        ),
                                        Some(format!(
                                            "Available fields: {}",
                                            res.fields
                                                .iter()
                                                .map(|(n, _)| n.as_str())
                                                .collect::<Vec<_>>()
                                                .join(", ")
                                        )),
                                    );
                                    Ty::Any
                                }
                            }
                        } else {
                            Ty::Any
                        }
                    }
                    Ty::Struct(struct_name) => {
                        if let Some(st) = self.structs.get(struct_name) {
                            match st.field_type(field_name) {
                                Some(ty) => ty.clone(),
                                None => {
                                    self.error(
                                        span,
                                        format!(
                                            "No field '{}' on struct '{}'",
                                            field_name, struct_name
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
                                    Ty::Any
                                }
                            }
                        } else {
                            Ty::Any
                        }
                    }
                    Ty::Event(event_name) => {
                        if let Some(evt) = self.events.get(event_name) {
                            if let Some(ft) = evt.field_type(field_name) {
                                ft.clone()
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
                                Ty::Any
                            }
                        } else {
                            Ty::Any
                        }
                    }
                    Ty::Map(_, value_ty) => *value_ty.clone(),
                    Ty::SumType(name) => {
                        if let Some(sum) = self.sum_types.get(name).cloned() {
                            self.check_sum_type_field_access(&sum, &obj_ty, field_name, span)
                        } else {
                            Ty::Any
                        }
                    }
                    Ty::App(name, _) => {
                        if let Some(sum) = self.sum_types.get(name).cloned() {
                            self.check_sum_type_field_access(&sum, &obj_ty, field_name, span)
                        } else {
                            Ty::Any
                        }
                    }
                    Ty::Any => Ty::Any,
                    other => {
                        self.error(
                            span,
                            format!("Type {} has no field '{}'", other, field_name),
                            None,
                        );
                        Ty::Any
                    }
                }
            }
            Expr::Index(obj, idx, _) => {
                let obj_ty = self.check_expr(obj);
                let idx_ty = self.check_expr(idx);
                match obj_ty {
                    Ty::List(inner) => {
                        if idx_ty != Ty::Int && idx_ty != Ty::Any {
                            self.error(
                                idx.span(),
                                format!("List index must be int, got {}", idx_ty),
                                None,
                            );
                        }
                        *inner
                    }
                    Ty::Str => {
                        if idx_ty != Ty::Int && idx_ty != Ty::Any {
                            self.error(
                                idx.span(),
                                format!("String index must be int, got {}", idx_ty),
                                None,
                            );
                        }
                        Ty::Int
                    }
                    Ty::Map(key_ty, value_ty) => {
                        if !key_ty.assignable_from(&idx_ty) && idx_ty != Ty::Any {
                            self.error(
                                idx.span(),
                                format!("Map key type is {}, got {}", key_ty, idx_ty),
                                None,
                            );
                        }
                        *value_ty
                    }
                    Ty::Tuple(tys) => {
                        if idx_ty != Ty::Int && idx_ty != Ty::Any {
                            self.error(
                                idx.span(),
                                format!("Tuple index must be int, got {}", idx_ty),
                                None,
                            );
                        }
                        if let Expr::IntLit(i, _) = &**idx {
                            if *i >= 0 && (*i as usize) < tys.len() {
                                return tys[*i as usize].clone();
                            } else {
                                self.error(
                                    idx.span(),
                                    format!("Tuple index out of bounds: {} (len {})", i, tys.len()),
                                    None,
                                );
                            }
                        }
                        // If we can't statically determine the index, return Any
                        // or should we return a union of all types? Let's just return Any for now,
                        // or better, the union. But Rad doesn't have true union types yet, so Any.
                        Ty::Any
                    }
                    Ty::Any => Ty::Any,
                    other => {
                        self.error(
                            expr.span(),
                            format!("Cannot index into type {}", other),
                            None,
                        );
                        Ty::Any
                    }
                }
            }
            Expr::ComponentExpr(name, fields, rest, span) => {
                let name = &self.resolve_canonical_name(name);
                if let Some(st) = self.structs.get(name).cloned() {
                    if let Some(rest_expr) = rest {
                        let rest_ty = self.check_expr(rest_expr);
                        if rest_ty != Ty::Any
                            && !matches!(rest_ty, Ty::Struct(ref sn) if sn == name)
                        {
                            self.error(
                                rest_expr.span(),
                                format!(
                                    "Struct update `..` for '{}' expects base value of type {}, got {}",
                                    name, name, rest_ty
                                ),
                                Some(format!("Use `..base` where `base` has type {}", name)),
                            );
                        }
                    }
                    let mut seen_fields = std::collections::HashSet::new();
                    for (field_name, field_expr) in fields {
                        if !seen_fields.insert(field_name.clone()) {
                            self.error(
                                field_expr.span(),
                                format!("Duplicate field '{}' in struct '{}'", field_name, name),
                                None,
                            );
                        }
                        let actual_ty = self.check_expr(field_expr);
                        match st.field_type(field_name) {
                            Some(expected_ty) => {
                                if !expected_ty.assignable_from(&actual_ty) && actual_ty != Ty::Any
                                {
                                    self.error(
                                        field_expr.span(),
                                        format!(
                                            "Type error in '{}.{}': expected {}, got {}",
                                            name, field_name, expected_ty, actual_ty
                                        ),
                                        None,
                                    );
                                }
                            }
                            None => {
                                self.error(
                                    span,
                                    format!("Unknown field '{}' on struct '{}'", field_name, name),
                                    None,
                                );
                            }
                        }
                    }
                    if rest.is_none() {
                        for (fname, _) in &st.fields {
                            if !fields.iter().any(|(n, _)| n == fname)
                                && !self.field_is_defaultable(name, fname)
                            {
                                self.error(
                                    span,
                                    format!("Missing field '{}' in struct '{}'", fname, name),
                                    Some(
                                        "fields with declared defaults may be omitted".to_string(),
                                    ),
                                );
                            }
                        }
                    }
                    Ty::Struct(name.clone())
                } else if let Some(comp) = self.components.get(name).cloned() {
                    if !comp.is_pub && is_cross_file(comp.file_id, span.file) {
                        self.error(
                            span,
                            format!("Component '{}' is private", name),
                            Some(format!("Add `pub` to the declaration of '{}'", name)),
                        );
                    }
                    if let Some(rest_expr) = rest {
                        let rest_ty = self.check_expr(rest_expr);
                        if rest_ty != Ty::Any
                            && !matches!(rest_ty, Ty::Component(ref comp_name) if comp_name == name)
                        {
                            self.error(
                                rest_expr.span(),
                                format!(
                                    "Component update `..` for '{}' expects base value of type {}, got {}",
                                    name, name, rest_ty
                                ),
                                Some(format!("Use `..base` where `base` has type {}", name)),
                            );
                        }
                    }
                    let mut seen_fields = std::collections::HashSet::new();
                    for (field_name, field_expr) in fields {
                        if !seen_fields.insert(field_name.clone()) {
                            self.error(
                                field_expr.span(),
                                format!("Duplicate field '{}' in component '{}'", field_name, name),
                                None,
                            );
                        }
                        let actual_ty = self.check_expr(field_expr);
                        match comp.field_type(field_name) {
                            Some(expected_ty) => {
                                if !expected_ty.assignable_from(&actual_ty) && actual_ty != Ty::Any
                                {
                                    self.error(
                                        field_expr.span(),
                                        format!(
                                            "Type error in '{}.{}': expected {}, got {}",
                                            name, field_name, expected_ty, actual_ty
                                        ),
                                        None,
                                    );
                                }
                            }
                            None => {
                                self.error(
                                    span,
                                    format!(
                                        "Unknown field '{}' on component '{}'",
                                        field_name, name
                                    ),
                                    None,
                                );
                            }
                        }
                    }
                    if rest.is_none() {
                        for (fname, _) in &comp.fields {
                            if !fields.iter().any(|(n, _)| n == fname)
                                && !self.field_is_defaultable(name, fname)
                            {
                                self.error(
                                    span,
                                    format!("Missing field '{}' in component '{}'", fname, name),
                                    Some(
                                        "fields with declared defaults may be omitted".to_string(),
                                    ),
                                );
                            }
                        }
                    }
                    Ty::Component(name.clone())
                } else if let Some(res) = self.resources.get(name).cloned() {
                    if !res.is_pub && is_cross_file(res.file_id, span.file) {
                        self.error(
                            span,
                            format!("Resource '{}' is private", name),
                            Some(format!("Add `pub` to the declaration of '{}'", name)),
                        );
                    }
                    if let Some(rest_expr) = rest {
                        let rest_ty = self.check_expr(rest_expr);
                        if rest_ty != Ty::Any
                            && !matches!(rest_ty, Ty::Component(ref comp_name) if comp_name == name)
                        {
                            self.error(
                                rest_expr.span(),
                                format!(
                                    "Resource update `..` for '{}' expects base value of type {}, got {}",
                                    name, name, rest_ty
                                ),
                                Some(format!("Use `..base` where `base` has type {}", name)),
                            );
                        }
                    }
                    let mut seen_fields = std::collections::HashSet::new();
                    for (field_name, field_expr) in fields {
                        if !seen_fields.insert(field_name.clone()) {
                            self.error(
                                field_expr.span(),
                                format!("Duplicate field '{}' in resource '{}'", field_name, name),
                                None,
                            );
                        }
                        let actual_ty = self.check_expr(field_expr);
                        match res.field_type(field_name) {
                            Some(expected_ty) => {
                                if !expected_ty.assignable_from(&actual_ty) && actual_ty != Ty::Any
                                {
                                    self.error(
                                        field_expr.span(),
                                        format!(
                                            "Type error in '{}.{}': expected {}, got {}",
                                            name, field_name, expected_ty, actual_ty
                                        ),
                                        None,
                                    );
                                }
                            }
                            None => {
                                self.error(
                                    span,
                                    format!(
                                        "Unknown field '{}' on resource '{}'",
                                        field_name, name
                                    ),
                                    None,
                                );
                            }
                        }
                    }
                    if rest.is_none() {
                        for (fname, _) in &res.fields {
                            if !fields.iter().any(|(n, _)| n == fname)
                                && !self.field_is_defaultable(name, fname)
                            {
                                self.error(
                                    span,
                                    format!("Missing field '{}' in resource '{}'", fname, name),
                                    Some(
                                        "fields with declared defaults may be omitted".to_string(),
                                    ),
                                );
                            }
                        }
                    }
                    Ty::Component(name.clone())
                } else {
                    self.error(
                        span,
                        format!("Unknown component or struct type '{}'", name),
                        None,
                    );
                    Ty::Component(name.clone())
                }
            }
            Expr::VariantExpr(type_name, variant_name, fields, span) => {
                let type_name = &self.resolve_canonical_name(type_name);
                match self.sum_types.get(type_name).cloned() {
                    Some(sum_type) => {
                        if !sum_type.is_pub && is_cross_file(sum_type.file_id, span.file) {
                            self.error(
                                span,
                                format!("Type '{}' is private", type_name),
                                Some(format!("Add `pub` to the declaration of '{}'", type_name)),
                            );
                        }
                        let param_vars: Vec<(String, Ty)> = sum_type
                            .type_params
                            .iter()
                            .map(|p| (p.clone(), self.fresh_var()))
                            .collect();

                        match sum_type.variants.iter().find(|v| v.name == *variant_name) {
                            Some(variant) => {
                                let mut seen_fields = std::collections::HashSet::new();
                                for (field_name, field_expr) in fields {
                                    if !seen_fields.insert(field_name.clone()) {
                                        self.error(
                                            field_expr.span(),
                                            format!(
                                                "Duplicate field '{}' in variant '{}::{}'",
                                                field_name, type_name, variant_name
                                            ),
                                            None,
                                        );
                                    }
                                    let actual_ty = self.check_expr(field_expr);
                                    match variant.fields.iter().find(|(n, _)| n == field_name) {
                                        Some((_, expected_ty)) => {
                                            let instantiated = self
                                                .substitute_type_params_with_vars(
                                                    expected_ty,
                                                    &param_vars,
                                                );
                                            if self.subst.unify(&instantiated, &actual_ty).is_err()
                                            {
                                                let resolved_expected =
                                                    self.resolve_ty(&instantiated);
                                                self.error(
                                                    field_expr.span(),
                                                    format!(
                                                        "Type error in '{}::{}' field '{}': expected {}, got {}",
                                                        type_name, variant_name, field_name, resolved_expected, actual_ty
                                                    ),
                                                    None,
                                                );
                                            }
                                        }
                                        None => {
                                            self.error(
                                                span,
                                                format!(
                                                    "Unknown field '{}' in variant '{}::{}'",
                                                    field_name, type_name, variant_name
                                                ),
                                                None,
                                            );
                                        }
                                    }
                                }
                                for (fname, _) in &variant.fields {
                                    if !fields.iter().any(|(n, _)| n == fname) {
                                        self.error(
                                            span,
                                            format!(
                                                "Missing field '{}' in variant '{}::{}'",
                                                fname, type_name, variant_name
                                            ),
                                            None,
                                        );
                                    }
                                }
                            }
                            None => {
                                self.error(
                                    span,
                                    format!(
                                        "Unknown variant '{}' in type '{}'",
                                        variant_name, type_name
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

                        if sum_type.type_params.is_empty() {
                            Ty::SumType(type_name.clone())
                        } else {
                            let resolved_args: Vec<Ty> = param_vars
                                .iter()
                                .map(|(_, var)| self.resolve_ty(var))
                                .collect();
                            Ty::App(type_name.clone(), resolved_args)
                        }
                    }
                    None => {
                        self.error(span, format!("Unknown type '{}'", type_name), None);
                        Ty::SumType(type_name.clone())
                    }
                }
            }
            Expr::StateRef(machine, state, span) => {
                let machine = &self.resolve_canonical_name(machine);
                let sm_opt = self.state_machines.get(machine).cloned();
                let st_opt = self.sum_types.get(machine).cloned();

                let is_sm_state = sm_opt.as_ref().is_some_and(|sm| sm.has_state(state));
                let variant_opt = st_opt
                    .as_ref()
                    .and_then(|st| st.variants.iter().find(|v| v.name == *state));

                let instantiate_sum = |st: &crate::types::SumTypeDef| {
                    if st.type_params.is_empty() {
                        Ty::SumType(machine.clone())
                    } else {
                        let args = st.type_params.iter().map(|_| Ty::Any).collect::<Vec<_>>();
                        Ty::App(machine.clone(), args)
                    }
                };

                if let (true, Some(variant)) = (is_sm_state, variant_opt) {
                    if self.options.compat_v0_5_dx {
                        if variant.fields.is_empty() {
                            if self.options.warn_compat {
                                self.warning(
                                    span,
                                    format!("Warning[W2501]: '{}::{}' resolves to sum variant, but '{}' is also a state machine", machine, state, machine),
                                    Some("Use explicit braces for sum variants or add context to disambiguate".to_string()),
                                );
                            }
                            self.variant_shorthand
                                .insert((machine.clone(), state.clone()));
                            return instantiate_sum(st_opt.as_ref().unwrap());
                        } else {
                            self.error(
                                span,
                                format!("Error[E2501]: Ambiguous reference '{}::{}' — matches both sum variant and state machine state", machine, state),
                                Some(format!("Use '{}::{} {{ ... }}' for the sum variant, or remove the sum type to use the state machine", machine, state)),
                            );
                            return Ty::State(machine.clone());
                        }
                    } else {
                        return Ty::State(machine.clone());
                    }
                }

                if is_sm_state {
                    if let Some(sm) = &sm_opt {
                        if !sm.is_pub && is_cross_file(sm.file_id, span.file) {
                            self.error(
                                span,
                                format!("State machine '{}' is private", machine),
                                Some(format!("Add `pub` to the declaration of '{}'", machine)),
                            );
                        }
                    }
                    return Ty::State(machine.clone());
                }

                if let Some(variant) = variant_opt {
                    let st = st_opt.as_ref().unwrap();
                    if !st.is_pub && is_cross_file(st.file_id, span.file) {
                        self.error(
                            span,
                            format!("Type '{}' is private", machine),
                            Some(format!("Add `pub` to the declaration of '{}'", machine)),
                        );
                    }

                    if !variant.fields.is_empty() {
                        self.error(
                            span,
                            format!("Variant '{}::{}' has fields — use '{}::{} {{ ... }}' to construct it", machine, state, machine, state),
                            None,
                        );
                    } else if !self.options.compat_v0_5_dx {
                        self.error(
                            span,
                            format!("Error[E2502]: Zero-field variant shorthand '{}::{}' requires --compat-v0.5-dx", machine, state),
                            Some(format!("Use '{}::{} {{ }}' or pass --compat-v0.5-dx", machine, state)),
                        );
                    }

                    self.variant_shorthand
                        .insert((machine.clone(), state.clone()));
                    return instantiate_sum(st);
                }

                if let Some(st) = st_opt {
                    self.error(
                        span,
                        format!("Unknown variant '{}' in type '{}'", state, machine),
                        Some(format!(
                            "Known variants: {}",
                            st.variants
                                .iter()
                                .map(|v| v.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    );
                    return instantiate_sum(&st);
                }

                if let Some(sm) = sm_opt {
                    self.error(
                        span,
                        format!("Unknown state '{}' in machine '{}'", state, machine),
                        Some(format!("Known states: {}", sm.states.join(", "))),
                    );
                    return Ty::State(machine.clone());
                }

                self.error(
                    span,
                    format!("Unknown state machine or type '{}'", machine),
                    None,
                );
                Ty::State(machine.clone())
            }
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
            "get" if arg_tys.len() == 2 => {
                if let Some(Expr::Ident(sn, _)) = arg_exprs.get(1) {
                    if self.structs.contains_key(sn.as_str()) {
                        self.error(
                            arg_exprs.get(1).unwrap().span(),
                            format!("Cannot use `get()` with struct type '{}'; only components can be used with ECS operations", sn),
                            Some(format!("Declare '{}' with `component` instead of `struct` to use it with ECS", sn)),
                        );
                    }
                }
                let comp_name = self.resolve_component_name(arg_exprs.get(1), arg_tys.get(1));
                if let Some(cn) = comp_name {
                    if self.components.contains_key(&cn) {
                        return Some(Ty::App("Option".to_string(), vec![Ty::Component(cn)]));
                    }
                }
                Some(Ty::App("Option".to_string(), vec![Ty::Any]))
            }
            "require" if arg_tys.len() == 2 => {
                let comp_name = self.resolve_component_name(arg_exprs.get(1), arg_tys.get(1));
                if let Some(cn) = comp_name {
                    if self.components.contains_key(&cn) {
                        return Some(Ty::Component(cn));
                    }
                }
                Some(Ty::Any)
            }
            "require_all" if arg_tys.len() >= 2 => Some(Ty::List(Box::new(Ty::Any))),
            // set_at: (T, K, V) -> T for T = list or map. The key must fit
            // the collection (int for lists, the key type for maps) and the
            // result keeps the collection's own type.
            "set_at" if arg_tys.len() == 3 => match &arg_tys[0] {
                Ty::List(_) => {
                    if arg_tys[1] != Ty::Int && arg_tys[1] != Ty::Any {
                        self.error(
                            arg_exprs
                                .get(1)
                                .map(|e| e.span())
                                .unwrap_or(&Span::default()),
                            format!("set_at() list index must be int, got {}", arg_tys[1]),
                            None,
                        );
                    }
                    Some(arg_tys[0].clone())
                }
                Ty::Map(key_ty, _) => {
                    if **key_ty != Ty::Any
                        && arg_tys[1] != Ty::Any
                        && self.subst.unify(key_ty, &arg_tys[1]).is_err()
                    {
                        self.error(
                            arg_exprs
                                .get(1)
                                .map(|e| e.span())
                                .unwrap_or(&Span::default()),
                            format!("set_at() map key expects {}, got {}", key_ty, arg_tys[1]),
                            None,
                        );
                    }
                    Some(arg_tys[0].clone())
                }
                Ty::Any => Some(Ty::Any),
                other => {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!("set_at() expects a list or map, got {}", other),
                        None,
                    );
                    Some(Ty::Any)
                }
            },
            "keys" if arg_tys.len() == 1 => match &arg_tys[0] {
                Ty::Component(_) | Ty::Struct(_) => Some(Ty::List(Box::new(Ty::Str))),
                Ty::Map(key_ty, _) => Some(Ty::List(key_ty.clone())),
                Ty::Any => Some(Ty::List(Box::new(Ty::Any))),
                _ => None,
            },
            "values" if arg_tys.len() == 1 => match &arg_tys[0] {
                Ty::Component(_) | Ty::Struct(_) => Some(Ty::List(Box::new(Ty::Any))),
                Ty::Map(_, val_ty) => Some(Ty::List(val_ty.clone())),
                Ty::Any => Some(Ty::List(Box::new(Ty::Any))),
                _ => None,
            },
            "entries" if arg_tys.len() == 1 => {
                match &arg_tys[0] {
                    Ty::Component(_) | Ty::Struct(_) => {
                        Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))))
                    }
                    Ty::Map(_key_ty, _val_ty) => {
                        // The inner list has type Any because it contains both K and V
                        Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))))
                    }
                    Ty::Any => Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any))))),
                    _ => None,
                }
            }
            "set" if arg_tys.len() == 2 => {
                if let Some(Ty::Struct(sn)) = arg_tys.get(1) {
                    self.error(
                        arg_exprs
                            .get(1)
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!(
                            "Cannot use `set()` with struct type '{}'; \
                             only components can be used with ECS operations",
                            sn
                        ),
                        Some(format!(
                            "Declare '{}' with `component` instead of `struct` to use it with ECS",
                            sn
                        )),
                    );
                }
                if let Some(Ty::Component(comp_name)) = arg_tys.get(1) {
                    if let Some(comp_type) = self.components.get(comp_name).cloned() {
                        if let Some(Expr::ComponentExpr(_, fields, _, _)) = arg_exprs.get(1) {
                            for (field_name, field_expr) in fields {
                                // The field_expr is already checked in check_expr when arg_tys was populated
                                // We just need to check if it's assignable
                                let actual_ty = self.check_expr(field_expr);
                                if let Some(expected_ty) = comp_type.field_type(field_name) {
                                    if !expected_ty.assignable_from(&actual_ty)
                                        && actual_ty != Ty::Any
                                    {
                                        self.error(
                                            field_expr.span(),
                                            format!(
                                                "Type error in '{}.{}': expected {}, got {}",
                                                comp_name, field_name, expected_ty, actual_ty
                                            ),
                                            None,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Some(Ty::Nil)
            }
            "transition" if arg_tys.len() == 2 => {
                let state_ty = self.resolve_ty(&arg_tys[0]);
                Some(Ty::App("Result".to_string(), vec![state_ty, Ty::Str]))
            }
            "contains" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::Bool);
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let expected_elem = match &subject_ty {
                    Ty::List(inner) => Some(inner.as_ref().clone()),
                    Ty::Str => Some(Ty::Str),
                    Ty::Map(key, _) => Some(key.as_ref().clone()),
                    Ty::Any => None,
                    _ => {
                        self.error(
                            arg_exprs[0].span(),
                            format!(
                                "Argument 1 to 'contains' expects list, str, or map, got {}",
                                subject_ty
                            ),
                            None,
                        );
                        None
                    }
                };
                if let Some(expected) = expected_elem {
                    if !expected.assignable_from(&arg_tys[1]) && arg_tys[1] != Ty::Any {
                        self.error(
                            arg_exprs[1].span(),
                            format!(
                                "Argument 2 to 'contains' expects {}, got {}",
                                expected, arg_tys[1]
                            ),
                            None,
                        );
                    }
                }
                Some(Ty::Bool)
            }
            "sort" | "reverse" => {
                if !self.check_builtin_arg_count(name, 1, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                match subject_ty {
                    Ty::List(inner) => Some(Ty::List(inner)),
                    Ty::Str => Some(Ty::Str),
                    _ => Some(Ty::Any),
                }
            }
            "slice" => {
                if arg_tys.len() < 2 || arg_tys.len() > 3 {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!(
                            "Function 'slice' expects 2 to 3 argument(s), got {}",
                            arg_tys.len()
                        ),
                        None,
                    );
                    return Some(Ty::Any);
                }
                for i in 1..arg_tys.len() {
                    if !Ty::Int.assignable_from(&arg_tys[i]) && arg_tys[i] != Ty::Any {
                        self.error(
                            arg_exprs[i].span(),
                            format!(
                                "Argument {} to 'slice' expects int, got {}",
                                i + 1,
                                arg_tys[i]
                            ),
                            None,
                        );
                    }
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                match subject_ty {
                    Ty::List(inner) => Some(Ty::List(inner)),
                    Ty::Str => Some(Ty::Str),
                    _ => Some(Ty::Any),
                }
            }
            "append" | "extend" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let mut valid = true;
                for i in 0..2 {
                    let ty = self.resolve_ty(&arg_tys[i]);
                    if !matches!(ty, Ty::List(_) | Ty::Str | Ty::Any) {
                        self.error(
                            arg_exprs[i].span(),
                            format!(
                                "Argument {} to '{}' expects list or str, got {}",
                                i + 1,
                                name,
                                ty
                            ),
                            None,
                        );
                        valid = false;
                    }
                }
                if !valid {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let t1 = match self.resolve_ty(&arg_tys[0]) {
                    Ty::List(inner) => *inner,
                    Ty::Str => Ty::Str,
                    _ => Ty::Any,
                };
                let t2 = match self.resolve_ty(&arg_tys[1]) {
                    Ty::List(inner) => *inner,
                    Ty::Str => Ty::Str,
                    _ => Ty::Any,
                };
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                match subject_ty {
                    Ty::Str => Some(Ty::Str),
                    _ => {
                        if t1 == t2 && t1 != Ty::Any {
                            Some(Ty::List(Box::new(t1)))
                        } else {
                            Some(Ty::List(Box::new(Ty::Any)))
                        }
                    }
                }
            }
            "flat_map" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                let mut ret_ty = Ty::Any;
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                    if let Ty::List(inner) = ret {
                        ret_ty = *inner;
                    } else if ret != Ty::Any {
                        self.error(
                            arg_exprs[1].span(),
                            format!(
                                "Argument 2 to 'flat_map' callback must return a list, got {}",
                                ret
                            ),
                            None,
                        );
                    }
                }
                Some(Ty::List(Box::new(ret_ty)))
            }
            "map" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                let mut ret_ty = Ty::Any;
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                    ret_ty = ret;
                }
                Some(Ty::List(Box::new(ret_ty)))
            }
            "filter" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                    if ret != Ty::Bool && ret != Ty::Any {
                        self.error(
                            arg_exprs[1].span(),
                            format!(
                                "Argument 2 to 'filter' callback must return bool, got {}",
                                ret
                            ),
                            None,
                        );
                    }
                }
                Some(Ty::List(Box::new(elem_ty)))
            }
            "reduce" => {
                if !self.check_builtin_arg_count(name, 3, arg_tys, arg_exprs) {
                    return Some(Ty::Any);
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let acc_ty = self.resolve_ty(&arg_tys[1]);
                let func_ty = self.resolve_ty(&arg_tys[2]);
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 2, arg_exprs[2].span())
                {
                    if params.len() >= 2 {
                        if !params[0].assignable_from(&acc_ty) && acc_ty != Ty::Any {
                            self.error(
                                arg_exprs[2].span(),
                                format!(
                                    "Argument 3 to 'reduce' expects fn({}, ...), got fn({}, ...)",
                                    acc_ty, params[0]
                                ),
                                None,
                            );
                        }
                        if !params[1].assignable_from(&elem_ty) && elem_ty != Ty::Any {
                            self.error(
                                arg_exprs[2].span(),
                                format!(
                                    "Argument 3 to 'reduce' expects fn(..., {}), got fn(..., {})",
                                    elem_ty, params[1]
                                ),
                                None,
                            );
                        }
                    }
                    if !acc_ty.assignable_from(&ret) && ret != Ty::Any && acc_ty != Ty::Any {
                        self.error(
                            arg_exprs[2].span(),
                            format!(
                                "Argument 3 to 'reduce' callback must return {}, got {}",
                                acc_ty, ret
                            ),
                            None,
                        );
                    }
                }
                Some(acc_ty)
            }
            "group_by" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::Map(
                        Box::new(Ty::Str),
                        Box::new(Ty::List(Box::new(Ty::Any))),
                    ));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                    if !ret.is_valid_map_key() {
                        self.error(
                            arg_exprs[1].span(),
                            format!(
                                "Argument 2 to 'group_by' callback must return a valid map key (str, int, bool, entity, or tuples of those), got {}",
                                ret
                            ),
                            None,
                        );
                        return Some(Ty::Map(
                            Box::new(Ty::Any),
                            Box::new(Ty::List(Box::new(elem_ty))),
                        ));
                    }
                    // the result map is keyed by whatever the key fn returns
                    return Some(Ty::Map(
                        Box::new(ret),
                        Box::new(Ty::List(Box::new(elem_ty))),
                    ));
                }
                Some(Ty::Map(
                    Box::new(Ty::Any),
                    Box::new(Ty::List(Box::new(elem_ty))),
                ))
            }
            "sort_by" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                if let Some((params, _ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                }
                match subject_ty {
                    Ty::List(inner) => Some(Ty::List(inner)),
                    Ty::Str => Some(Ty::Str),
                    _ => Some(Ty::Any),
                }
            }
            "zip" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))));
                }
                let mut valid = true;
                for i in 0..2 {
                    let ty = self.resolve_ty(&arg_tys[i]);
                    if !matches!(ty, Ty::List(_) | Ty::Str | Ty::Any) {
                        self.error(
                            arg_exprs[i].span(),
                            format!(
                                "Argument {} to 'zip' expects list or str, got {}",
                                i + 1,
                                ty
                            ),
                            None,
                        );
                        valid = false;
                    }
                }
                if !valid {
                    return Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))));
                }
                let t1 = match self.resolve_ty(&arg_tys[0]) {
                    Ty::List(inner) => *inner,
                    Ty::Str => Ty::Str,
                    _ => Ty::Any,
                };
                let t2 = match self.resolve_ty(&arg_tys[1]) {
                    Ty::List(inner) => *inner,
                    Ty::Str => Ty::Str,
                    _ => Ty::Any,
                };
                if t1 == t2 && t1 != Ty::Any {
                    Some(Ty::List(Box::new(Ty::List(Box::new(t1)))))
                } else {
                    Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))))
                }
            }
            "unwrap" if arg_tys.len() == 1 => {
                let container_ty = self.resolve_ty(&arg_tys[0]);
                match container_ty {
                    Ty::App(name, args) if name == "Option" && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    Ty::App(name, args) if name == "Result" && args.len() == 2 => {
                        Some(args[0].clone())
                    }
                    Ty::SumType(name) if name == "Option" || name == "Result" => Some(Ty::Any),
                    Ty::Any => Some(Ty::Any),
                    _ => None, // Fall through to normal type checking to report error
                }
            }
            "expect" if arg_tys.len() == 2 => {
                let container_ty = self.resolve_ty(&arg_tys[0]);
                match container_ty {
                    Ty::App(name, args) if name == "Option" && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    Ty::App(name, args) if name == "Result" && args.len() == 2 => {
                        Some(args[0].clone())
                    }
                    Ty::SumType(name) if name == "Option" || name == "Result" => Some(Ty::Any),
                    Ty::Any => Some(Ty::Any),
                    _ => None, // Fall through to normal type checking to report error
                }
            }
            "unwrap_or" if arg_tys.len() == 2 => {
                let container_ty = self.resolve_ty(&arg_tys[0]);
                match container_ty {
                    Ty::App(name, args) if name == "Option" && args.len() == 1 => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::App(name, args) if name == "Result" && args.len() == 2 => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::SumType(name) if name == "Option" || name == "Result" => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::Any => Some(self.resolve_ty(&arg_tys[1])),
                    _ => None,
                }
            }
            "map_or" if arg_tys.len() == 3 => {
                let container_ty = self.resolve_ty(&arg_tys[0]);
                match container_ty {
                    Ty::App(name, args) if name == "Option" && args.len() == 1 => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::App(name, args) if name == "Result" && args.len() == 2 => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::SumType(name) if name == "Option" || name == "Result" => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::Any => Some(self.resolve_ty(&arg_tys[1])),
                    _ => None,
                }
            }
            "query_where" => {
                if arg_tys.len() < 2 {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!(
                            "Function 'query_where' expects at least 2 argument(s), got {}",
                            arg_tys.len()
                        ),
                        None,
                    );
                } else {
                    for i in 0..arg_tys.len() - 1 {
                        if !Ty::Str.assignable_from(&arg_tys[i]) && arg_tys[i] != Ty::Any {
                            self.error(
                                arg_exprs[i].span(),
                                format!(
                                    "Argument {} to 'query_where' expects str, got {}",
                                    i + 1,
                                    arg_tys[i]
                                ),
                                None,
                            );
                        }
                    }
                    let pred_ty = &arg_tys[arg_tys.len() - 1];
                    let pred_expr = arg_exprs[arg_tys.len() - 1];
                    let expected_pred = Ty::Fn {
                        params: vec![Ty::EntityId],
                        ret: Box::new(Ty::Bool),
                        purity: FnPurity::Pure,
                    };
                    if !expected_pred.assignable_from(pred_ty) && *pred_ty != Ty::Any {
                        // Separate an EFFECT mismatch (right shape, but the
                        // predicate has side effects) from a genuine shape
                        // mismatch. The former otherwise renders as "expects
                        // pure fn(entity) -> bool, got fn(entity) -> bool" —
                        // two identical-looking types differing by one
                        // easily-missed word, with no clue why. Name the
                        // offending call, reusing the pipeline breach finder.
                        let impure_shape = Ty::Fn {
                            params: vec![Ty::EntityId],
                            ret: Box::new(Ty::Bool),
                            purity: FnPurity::Impure,
                        };
                        if impure_shape.assignable_from(pred_ty) {
                            // World READS are fine: query_where snapshots
                            // the matching entity list before the predicate
                            // runs, so `get`/`res`/`has`/`readonly fn` in
                            // the body observe a stable world. The contract
                            // is read-only, not pure — filtering by
                            // component values is the builtin's whole
                            // purpose. Only writes, IO, events, and
                            // unverifiable calls remain breaches.
                            // A value whose TYPE already vouches (a pure or
                            // readonly fn type — explicit annotation, vouched
                            // closure, named pure/readonly fn, or a promoted
                            // callback param) needs no body walk:
                            // assignability guarantees only conforming values
                            // inhabit it.
                            let type_vouches = matches!(
                                pred_ty,
                                Ty::Fn {
                                    purity: FnPurity::Pure | FnPurity::Readonly,
                                    ..
                                }
                            );
                            let breach = if type_vouches {
                                None
                            } else {
                                match pred_expr {
                                    Expr::FnExpr(_, _, _, _, _, body, _) => {
                                        self.find_block_readonly_breach(body)
                                    }
                                    Expr::Ident(name, _) => {
                                        let resolved = self.resolve_canonical_name(name);
                                        match self
                                            .functions
                                            .get(&resolved)
                                            .or_else(|| self.functions.get(name.as_str()))
                                        {
                                            Some(sig)
                                                if sig.is_pure
                                                    || Self::effect_set_is_read_only(
                                                        &sig.effects,
                                                    ) =>
                                            {
                                                None
                                            }
                                            _ => Some(format!(
                                                "'{}' is not a `pure fn` or `readonly fn`",
                                                name
                                            )),
                                        }
                                    }
                                    _ => Some(
                                        "cannot be verified as read-only (only inline closures and named fns are analyzable)"
                                            .to_string(),
                                    ),
                                }
                            };
                            if let Some(reason) = breach {
                                let hint = format!(
                                    "the predicate {} — query_where runs it during iteration, so world reads are allowed but writes, IO, and events are not",
                                    reason
                                );
                                self.error(
                                    pred_expr.span(),
                                    format!(
                                        "Argument {} to 'query_where' must be a read-only predicate, but the one given has side effects",
                                        arg_tys.len()
                                    ),
                                    Some(hint),
                                );
                            }
                        } else {
                            self.error(
                                pred_expr.span(),
                                format!(
                                    "Argument {} to 'query_where' expects {}, got {}",
                                    arg_tys.len(),
                                    expected_pred,
                                    pred_ty
                                ),
                                None,
                            );
                        }
                    }
                }
                Some(Ty::List(Box::new(Ty::EntityId)))
            }
            "query_map" => {
                let mut ret_ty = Ty::Any;
                if arg_tys.len() < 2 {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!(
                            "Function 'query_map' expects at least 2 argument(s), got {}",
                            arg_tys.len()
                        ),
                        None,
                    );
                } else {
                    for i in 0..arg_tys.len() - 1 {
                        if !Ty::Str.assignable_from(&arg_tys[i]) && arg_tys[i] != Ty::Any {
                            self.error(
                                arg_exprs[i].span(),
                                format!(
                                    "Argument {} to 'query_map' expects str, got {}",
                                    i + 1,
                                    arg_tys[i]
                                ),
                                None,
                            );
                        }
                    }
                    let map_fn_ty = &arg_tys[arg_tys.len() - 1];
                    let map_expr = arg_exprs[arg_tys.len() - 1];
                    if let Ty::Fn { ret, purity, .. } = map_fn_ty {
                        ret_ty = *ret.clone();
                        // Same read-only contract as query_where's predicate
                        // (the mapper runs during iteration over a
                        // snapshotted entity list): world reads and
                        // `readonly fn` calls are fine, writes/IO/events are
                        // not. A Pure or Readonly TYPE already vouches;
                        // only impure-typed mappers need the body walk.
                        if matches!(purity, FnPurity::Impure) {
                            let breach = match map_expr {
                                Expr::FnExpr(_, _, _, _, _, body, _) => {
                                    self.find_block_readonly_breach(body)
                                }
                                Expr::Ident(name, _) => {
                                    let resolved = self.resolve_canonical_name(name);
                                    match self
                                        .functions
                                        .get(&resolved)
                                        .or_else(|| self.functions.get(name.as_str()))
                                    {
                                        Some(sig)
                                            if sig.is_pure
                                                || Self::effect_set_is_read_only(
                                                    &sig.effects,
                                                ) =>
                                        {
                                            None
                                        }
                                        _ => Some(format!(
                                            "'{}' is not a `pure fn` or `readonly fn`",
                                            name
                                        )),
                                    }
                                }
                                _ => Some(
                                    "cannot be verified as read-only (only inline closures and named fns are analyzable)"
                                        .to_string(),
                                ),
                            };
                            if let Some(reason) = breach {
                                let hint = format!(
                                    "the mapper {} — query_map runs it during iteration, so world reads are allowed but writes, IO, and events are not",
                                    reason
                                );
                                self.error(
                                    map_expr.span(),
                                    format!(
                                        "Argument {} to 'query_map' must be a read-only mapper, but the one given has side effects",
                                        arg_tys.len()
                                    ),
                                    Some(hint),
                                );
                            }
                        }
                    } else if *map_fn_ty != Ty::Any {
                        self.error(
                            arg_exprs[arg_tys.len() - 1].span(),
                            format!(
                                "Argument {} to 'query_map' expects function, got {}",
                                arg_tys.len(),
                                map_fn_ty
                            ),
                            None,
                        );
                    }
                }
                Some(Ty::List(Box::new(ret_ty)))
            }
            "query_count" => {
                if arg_tys.is_empty() {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        "Function 'query_count' expects at least 1 argument(s), got 0".to_string(),
                        None,
                    );
                } else {
                    for i in 0..arg_tys.len() {
                        if !Ty::Str.assignable_from(&arg_tys[i]) && arg_tys[i] != Ty::Any {
                            self.error(
                                arg_exprs[i].span(),
                                format!(
                                    "Argument {} to 'query_count' expects str, got {}",
                                    i + 1,
                                    arg_tys[i]
                                ),
                                None,
                            );
                        }
                    }
                }
                Some(Ty::Int)
            }
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

/// Variant/state names matched by arms with NO guard. Only these make a
/// match exhaustive: a guarded arm is skipped when its guard is false, so
/// the match can fall through and evaluate to nil.
fn unguarded_variant_names(stmt: &MatchStmt) -> Vec<&String> {
    variant_names_where(stmt, |c| c.guard.is_none())
}

/// Variant/state names that appear ONLY on guarded arms, used to tell the
/// author "you wrote this arm, but it is conditional" instead of the
/// misleading "variant is not covered".
fn guarded_variant_names(stmt: &MatchStmt) -> Vec<&String> {
    variant_names_where(stmt, |c| c.guard.is_some())
}

fn variant_names_where(stmt: &MatchStmt, keep: fn(&MatchCase) -> bool) -> Vec<&String> {
    stmt.cases
        .iter()
        .filter(|c| keep(c))
        .filter_map(|c| match &c.pattern {
            Pattern::Variant { path, .. } => path.last(),
            _ => None,
        })
        .collect()
}
