impl Checker {
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
                            "Component field '{}.{}' cannot have a function type. Components must contain data, not executable behavior",
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
                            "Struct field '{}.{}' cannot have a function type. Structs must contain data, not executable behavior",
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

        if self.materialized_views.contains(&resolved_name) {
            self.error(
                &init.span,
                format!(
                    "Materialized view '{}' is runtime-maintained and cannot be mutated directly",
                    init.comp_name
                ),
                Some(
                    "change the view's declared source components inside their owning transaction"
                        .to_string(),
                ),
            );
            return;
        }

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
                    if !self.accepts_inferred_value(expected_ty, &actual_ty) {
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

}
