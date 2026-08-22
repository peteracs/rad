impl Checker {
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
            Decl::Model(m) => self.check_model_decl(m),
            Decl::Stmt(s) => self.check_stmt(s),
            Decl::TypeAlias(_) => {}
            Decl::NativeType(_) => {}
            Decl::MaterializedView(view) => self.check_materialized_view_decl(view),
            Decl::Error => {}
        }
    }

    fn check_materialized_view_decl(&mut self, view: &MaterializedViewDecl) {
        use crate::materialized_view::ViewPredicateValue;

        for dependency in &view.dependencies {
            let resolved = self.resolve_canonical_name(dependency);
            if !self.components.contains_key(&resolved) {
                self.error(
                    &view.span,
                    format!(
                        "Materialized view '{}' depends on unknown component '{}'",
                        view.name, dependency
                    ),
                    Some(
                        "Declare the component or correct the dependency; view invalidation requires an exact component set"
                            .to_string(),
                    ),
                );
            }
        }

        let fields = view
            .key
            .iter()
            .map(|(component, field)| (component, field, None))
            .chain(view.predicate.clauses.iter().map(|clause| {
                (
                    &clause.component,
                    &clause.field,
                    Some(&clause.expected),
                )
            }))
            .collect::<Vec<_>>();
        for (component_name, field_name, expected) in fields {
            let resolved = self.resolve_canonical_name(component_name);
            let Some(component) = self.components.get(&resolved).cloned() else {
                continue;
            };
            let Some((_, field_type)) = component
                .fields
                .iter()
                .find(|(name, _)| name == field_name)
            else {
                self.error(
                    &view.span,
                    format!(
                        "Materialized view '{}' references unknown field '{}.{}'",
                        view.name, component_name, field_name
                    ),
                    Some(format!(
                        "Available fields: {}",
                        component
                            .fields
                            .iter()
                            .map(|(name, _)| name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                );
                continue;
            };
            let Some(expected) = expected else {
                continue;
            };
            let compatible = matches!(
                (field_type, expected),
                (Ty::Int, ViewPredicateValue::Int(_))
                    | (Ty::Float, ViewPredicateValue::Float(_))
                    | (Ty::Str, ViewPredicateValue::Str(_))
                    | (Ty::Bool, ViewPredicateValue::Bool(_))
                    | (Ty::Any, _)
            );
            if !compatible {
                self.error(
                    &view.span,
                    format!(
                        "Materialized view '{}' compares '{}.{}' ({}) with an incompatible {} literal",
                        view.name,
                        component_name,
                        field_name,
                        field_type,
                        match expected {
                            ViewPredicateValue::Int(_) => "int",
                            ViewPredicateValue::Float(_) => "float",
                            ViewPredicateValue::Str(_) => "str",
                            ViewPredicateValue::Bool(_) => "bool",
                        }
                    ),
                    Some("Compare a field with a literal of the same semantic type".to_string()),
                );
            }
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
                    if let Some(name) = d.namespace_name() {
                        if name == field_name && d.is_public() {
                            found = true;
                            // Check type
                            let actual_ty = match d {
                                Decl::Fn(f) => {
                                    let mangled = imported_decls.canonical_symbol(&f.name);
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

    fn check_model_decl(&mut self, decl: &ModelDecl) {
        for command in &decl.commands {
            match self.check_expr(command) {
                Ty::Fn { params, .. } if params.is_empty() => {}
                Ty::Fn { params, .. } => self.error(
                    command.span(),
                    format!(
                        "Model commands must take zero arguments; this command takes {}",
                        params.len()
                    ),
                    Some("Capture generated state in the command or read it from ECS state".to_string()),
                ),
                actual => self.error(
                    command.span(),
                    format!("Model command is not callable (got {})", actual),
                    Some("List zero-argument functions in `commands [...]`".to_string()),
                ),
            }
        }
        for invariant in &decl.invariants {
            if !self.closure_body_is_conservatively_readonly(&[], &[], &[], invariant) {
                self.error(
                    &invariant.span,
                    "Model invariants must be readonly; they cannot write state, emit events, perform IO, or start async work".to_string(),
                    Some("Move the mutation into a model command and leave the invariant observational".to_string()),
                );
            }
            let expr = Expr::FnExpr(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(TypeExpr::Named("bool".to_string())),
                invariant.clone(),
                decl.span.clone(),
            );
            let _ = self.check_expr(&expr);
        }
        let observations = decl.temporal.iter().flat_map(|clause| match clause {
            TemporalClause::Always(name)
            | TemporalClause::Eventually(name)
            | TemporalClause::ExactlyOnce(name) => vec![name.as_str()],
            TemporalClause::Until {
                condition,
                terminal,
            } => vec![condition.as_str(), terminal.as_str()],
            TemporalClause::LeadsTo {
                trigger,
                consequence,
            } => vec![trigger.as_str(), consequence.as_str()],
            TemporalClause::NeverAfter {
                prohibited,
                terminal,
            } => vec![prohibited.as_str(), terminal.as_str()],
            TemporalClause::EventuallyWithin {
                trigger,
                consequence,
                ..
            } => vec![trigger.as_str(), consequence.as_str()],
        });
        for name in observations {
            if !self.components.contains_key(name)
                && !self.events.contains_key(name)
                && !self.state_machines.contains_key(name)
            {
                self.error(
                    &decl.span,
                    format!(
                        "Temporal observation '{}' is not a declared component, state, or event",
                        name
                    ),
                    None,
                );
            }
        }
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

}
