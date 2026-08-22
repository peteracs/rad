impl Checker {
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
        for (mode, authorities) in [
            ("reads", &decl.authority_reads),
            ("writes", &decl.authority_writes),
        ] {
            let mut seen = std::collections::HashSet::new();
            for authority in authorities {
                if !seen.insert(authority) {
                    self.error(
                        &decl.span,
                        format!(
                            "System '{}' declares '{} {}' more than once",
                            decl.name, mode, authority
                        ),
                        Some("Remove the duplicate authority-only signature entry".to_string()),
                    );
                    continue;
                }
                if authority == "*" {
                    continue;
                }
                if matches!(
                    authority.as_str(),
                    "$entities" | "$entity_names" | "$entity_identity"
                ) {
                    continue;
                }
                let resolved = self.resolve_canonical_name(authority);
                if !self.components.contains_key(&resolved)
                    && !self.resources.contains_key(&resolved)
                {
                    self.error(
                        &decl.span,
                        format!(
                            "System '{}' declares authority over unknown component/resource '{}'",
                            decl.name, authority
                        ),
                        Some(format!(
                            "Declare '{}', import it, or correct the authority-only signature entry",
                            authority
                        )),
                    );
                }
            }
        }
        let mut seen_emits = std::collections::HashSet::new();
        for authority in &decl.authority_emits {
            if !seen_emits.insert(authority) {
                self.error(
                    &decl.span,
                    format!(
                        "System '{}' declares 'emits {}' more than once",
                        decl.name, authority
                    ),
                    Some("Remove the duplicate event permission".to_string()),
                );
                continue;
            }
            if authority == "*" || matches!(authority.as_str(), "$events" | "$transition") {
                continue;
            }
            let resolved = self.resolve_canonical_name(authority);
            if !self.events.contains_key(&resolved) {
                self.error(
                    &decl.span,
                    format!(
                        "System '{}' declares emission authority over unknown event '{}'",
                        decl.name, authority
                    ),
                    Some(format!(
                        "Declare event '{}', import it, or correct the `emits` permission",
                        authority
                    )),
                );
            }
        }
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
}
