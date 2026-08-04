impl Checker {

fn check_expr_access_and_construction(&mut self, expr: &Expr) -> Ty {
        match expr {
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
            _ => unreachable!("dispatcher selected the wrong match partition"),
        }
    }}