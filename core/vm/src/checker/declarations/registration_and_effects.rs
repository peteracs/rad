
impl Checker {
    pub(super) fn collect_declarations(&mut self, program: &Program) {
        for decl in &program.declarations {
            match decl {
                Decl::Component(c) => {
                    self.register_component(c);
                    self.define(&c.name, Ty::Str, false, c.span.clone(), c.is_pub, false);
                }
                Decl::Resource(r) => {
                    self.register_resource(r);
                    self.define(&r.name, Ty::Str, false, r.span.clone(), r.is_pub, false);
                }
                Decl::Struct(s) => {
                    self.register_struct(s);
                    self.define(&s.name, Ty::Str, false, s.span.clone(), s.is_pub, false);
                }
                Decl::Intent(i) => self.register_intent(i),
                Decl::Law(l) => self.register_law(l),
                Decl::Resolver(r) => self.register_resolver(r),
                Decl::Constraint(c) => self.register_constraint(c),
                Decl::State(s) => {
                    self.register_state_machine(s);
                    self.define(&s.name, Ty::Any, false, s.span.clone(), s.is_pub, false);
                }
                Decl::System(s) => self.register_system(s),
                Decl::Event(e) => {
                    self.register_event(e);
                    self.define(&e.name, Ty::Str, false, e.span.clone(), e.is_pub, false);
                }
                Decl::Fn(f) => {
                    self.register_function(f);
                    if let Some(sig) = self.functions.get(&f.name) {
                        let fn_ty = Ty::Fn {
                            params: sig.params.clone(),
                            ret: Box::new(sig.ret.clone()),
                            purity: if sig.effects.is_pure() {
                                FnPurity::Pure
                            } else if sig.effects.is_readonly() {
                                FnPurity::Readonly
                            } else {
                                FnPurity::Impure
                            },
                        };
                        self.define(&f.name, fn_ty, false, f.span.clone(), f.is_pub, false);
                    }
                }
                Decl::Type(t) => {
                    self.register_sum_type(t);
                    self.define(&t.name, Ty::Any, false, t.span.clone(), t.is_pub, false);
                }
                Decl::TypeAlias(a) => {
                    self.register_type_alias(a);
                    self.define(&a.name, Ty::Any, false, a.span.clone(), a.is_pub, false);
                }
                Decl::Phase(p) => {
                    self.phases.insert(p.name.clone(), p.systems.clone());
                }
                Decl::Entity(e) => {
                    self.define(
                        &e.name,
                        Ty::EntityId,
                        false,
                        e.span.clone(),
                        e.is_pub,
                        false,
                    );
                }
                _ => {}
            }
        }
        self.check_system_cycles();
    }
    pub(super) fn register_sum_type(&mut self, decl: &TypeDeclNode) {
        let type_params = &decl.type_params;
        let variants: Vec<VariantType> = decl
            .variants
            .iter()
            .map(|v| {
                let fields: Vec<(String, Ty)> = v
                    .fields
                    .iter()
                    .map(|(name, expr)| {
                        // Annotated fields (`target: entity`) carry their
                        // declared type; default-valued fields infer from
                        // the literal, with bare idents as type-param refs.
                        if let Some((_, te)) = v.annotations.iter().find(|(n, _)| n == name) {
                            let ty = match te {
                                TypeExpr::Named(ident) if type_params.contains(ident) => {
                                    Ty::App(ident.clone(), vec![])
                                }
                                _ => self.type_expr_or_any(te),
                            };
                            return (name.clone(), ty);
                        }
                        let ty = if let Expr::Ident(ident, _) = expr {
                            if type_params.contains(ident) {
                                Ty::App(ident.clone(), vec![])
                            } else {
                                self.infer_literal_type(expr)
                            }
                        } else {
                            self.infer_literal_type(expr)
                        };
                        (name.clone(), ty)
                    })
                    .collect();
                VariantType {
                    name: v.name.clone(),
                    fields,
                }
            })
            .collect();
        self.sum_types.insert(
            decl.name.clone(),
            SumTypeDef {
                name: decl.name.clone(),
                type_params: decl.type_params.clone(),
                variants,
                is_pub: decl.is_pub,
                file_id: decl.span.file,
            },
        );
    }
    pub(super) fn register_component(&mut self, decl: &ComponentDecl) {
        if self.resources.contains_key(&decl.name) {
            self.error(
                &decl.span,
                format!(
                    "Component '{}' conflicts with an existing resource of the same name",
                    decl.name
                ),
                Some(
                    "Rename one of them — a component and a resource cannot share the same name."
                        .to_string(),
                ),
            );
        }
        if decl.is_pub {
            for field in &decl.fields {
                if field.type_annotation.is_none() {
                    self.error(
                        &decl.span,
                        format!(
                            "Public component '{}' requires explicit type annotations for all fields",
                            decl.name
                        ),
                        Some(format!(
                            "Add a type annotation to field '{}', e.g. `{}: Type = ...`",
                            field.name, field.name
                        )),
                    );
                }
            }
        }
        let fields: Vec<(String, Ty)> = decl
            .fields
            .iter()
            .map(|field| {
                if let Some(te) = &field.type_annotation {
                    (field.name.clone(), self.type_expr_or_any(te))
                } else {
                    (
                        field.name.clone(),
                        self.infer_literal_type(&field.default_value),
                    )
                }
            })
            .collect();
        let field_type_map: HashMap<String, Ty> = fields.iter().cloned().collect();
        let mut indexed_fields = std::collections::HashSet::new();
        for field_name in &decl.indexed_fields {
            if !field_type_map.contains_key(field_name) {
                self.error(
                    &decl.span,
                    format!(
                        "Component '{}': indexed field '{}' is not declared",
                        decl.name, field_name
                    ),
                    None,
                );
                continue;
            }
            let ty = field_type_map.get(field_name).cloned().unwrap_or(Ty::Any);
            if !matches!(ty, Ty::Int | Ty::Str | Ty::Bool | Ty::EntityId | Ty::Float) {
                self.error(
                    &decl.span,
                    format!(
                        "Component '{}.{}' cannot be indexed because it has type {}",
                        decl.name, field_name, ty
                    ),
                    Some(
                        "Indexed fields must be int, float, str, bool, or entity for deterministic hash lookup"
                            .to_string(),
                    ),
                );
                continue;
            }
            indexed_fields.insert(field_name.clone());
        }
        self.record_defaultable_fields(&decl.name, &decl.fields);
        self.components.insert(
            decl.name.clone(),
            ComponentType {
                name: decl.name.clone(),
                fields,
                is_pub: decl.is_pub,
                file_id: decl.span.file,
                indexed_fields,
            },
        );
    }

    /// Record which fields of a data declaration carry a *usable* default —
    /// an annotated `name: Type = expr` field, or an unannotated field whose
    /// initializer is a value literal (`hp: 100`). Bare type annotations
    /// (`x: float`) have no default and stay required in literals. Literals
    /// may omit defaultable fields; the compiler fills them in.
    fn record_defaultable_fields(&mut self, type_name: &str, fields: &[crate::ast::FieldDef]) {
        fn usable(f: &crate::ast::FieldDef) -> bool {
            use crate::ast::Expr;
            // Annotation-only fields have no default at all: every
            // construction must provide them.
            if f.required {
                return false;
            }
            if f.type_annotation.is_some() {
                return true;
            }
            matches!(
                f.default_value,
                Expr::IntLit(..)
                    | Expr::FloatLit(..)
                    | Expr::StrLit(..)
                    | Expr::BoolLit(..)
                    | Expr::NilLit(..)
                    | Expr::ListLit(..)
                    | Expr::MapLit(..)
            )
        }
        let set: std::collections::HashSet<String> = fields
            .iter()
            .filter(|f| usable(f))
            .map(|f| f.name.clone())
            .collect();
        if !set.is_empty() {
            self.defaultable_fields.insert(type_name.to_string(), set);
        }
    }
    pub(super) fn register_resource(&mut self, decl: &ResourceDecl) {
        // Resources auto-initialize from field defaults, so a field with
        // no default cannot exist — there is no construction site to
        // demand it at.
        for f in &decl.fields {
            if f.required {
                self.error(
                    &decl.span,
                    format!(
                        "Resource '{}' field '{}' has a type but no default — resources auto-initialize, so every field needs one",
                        decl.name, f.name
                    ),
                    Some(format!("Write `{}: <type> = <value>`", f.name)),
                );
            }
        }
        if self.resources.contains_key(&decl.name) {
            self.error(
                &decl.span,
                format!(
                    "Duplicate resource declaration '{}'; a resource with this name already exists",
                    decl.name
                ),
                None,
            );
        }
        if self.components.contains_key(&decl.name) {
            self.error(
                &decl.span,
                format!(
                    "Resource '{}' conflicts with an existing component of the same name",
                    decl.name
                ),
                Some(
                    "Rename one of them — a component and a resource cannot share the same name."
                        .to_string(),
                ),
            );
        }
        if decl.is_pub {
            for field in &decl.fields {
                if field.type_annotation.is_none() {
                    self.error(
                        &decl.span,
                        format!(
                            "Public resource '{}' requires explicit type annotations for all fields",
                            decl.name
                        ),
                        Some(format!(
                            "Add a type annotation to field '{}', e.g. `{}: Type = ...`",
                            field.name, field.name
                        )),
                    );
                }
            }
        }
        let fields: Vec<(String, Ty)> = decl
            .fields
            .iter()
            .map(|field| {
                if let Some(te) = &field.type_annotation {
                    (field.name.clone(), self.type_expr_or_any(te))
                } else {
                    (
                        field.name.clone(),
                        self.infer_literal_type(&field.default_value),
                    )
                }
            })
            .collect();
        self.record_defaultable_fields(&decl.name, &decl.fields);
        self.resources.insert(
            decl.name.clone(),
            ResourceType {
                name: decl.name.clone(),
                fields,
                is_pub: decl.is_pub,
                file_id: decl.span.file,
            },
        );
    }
    pub(super) fn register_struct(&mut self, decl: &StructDecl) {
        if decl.is_pub {
            for field in &decl.fields {
                if field.type_annotation.is_none() {
                    self.error(
                        &decl.span,
                        format!(
                            "Public struct '{}' requires explicit type annotations for all fields",
                            decl.name
                        ),
                        Some(format!(
                            "Add a type annotation to field '{}', e.g. `{}: Type = ...`",
                            field.name, field.name
                        )),
                    );
                }
            }
        }
        let fields: Vec<(String, Ty)> = decl
            .fields
            .iter()
            .map(|field| {
                if let Some(te) = &field.type_annotation {
                    (field.name.clone(), self.type_expr_or_any(te))
                } else {
                    (
                        field.name.clone(),
                        self.infer_literal_type(&field.default_value),
                    )
                }
            })
            .collect();
        self.record_defaultable_fields(&decl.name, &decl.fields);
        self.structs.insert(
            decl.name.clone(),
            StructType {
                name: decl.name.clone(),
                fields,
                is_pub: decl.is_pub,
                file_id: decl.span.file,
            },
        );
    }
    pub(super) fn register_state_machine(&mut self, decl: &StateDecl) {
        let states: Vec<String> = decl.states.iter().map(|s| s.name.clone()).collect();
        let mut transitions = HashMap::new();
        for state_def in &decl.states {
            let trans: Vec<(String, String)> = state_def
                .transitions
                .iter()
                .map(|(evt, target, _)| (evt.clone(), target.clone()))
                .collect();
            transitions.insert(state_def.name.clone(), trans);
        }
        self.state_machines.insert(
            decl.name.clone(),
            StateMachineType {
                name: decl.name.clone(),
                states,
                transitions,
                is_pub: decl.is_pub,
                file_id: decl.span.file,
            },
        );
    }
    pub(super) fn register_system(&mut self, decl: &SystemDecl) {
        let params: Vec<SystemParam> = decl
            .params
            .iter()
            .map(|(name, is_mut, comp_type)| SystemParam {
                name: name.clone(),
                component_type: comp_type.clone(),
                is_mut: *is_mut,
                is_resource: false,
            })
            .collect();
        self.systems.insert(
            decl.name.clone(),
            SystemType {
                name: decl.name.clone(),
                params,
                is_pub: decl.is_pub,
                file_id: decl.span.file,
                simulation_breach: None,
                simulation_breach_par: None,
            },
        );
        self.system_deps.insert(
            decl.name.clone(),
            (decl.after.clone(), decl.before.clone(), decl.span.clone()),
        );
    }
    pub(super) fn register_event(&mut self, decl: &EventDecl) {
        if decl.is_pub {
            for (name, type_ann) in &decl.fields {
                if type_ann.is_none() {
                    self.error(
                        &decl.span,
                        format!(
                            "Public event '{}' requires explicit type annotations for all fields",
                            decl.name
                        ),
                        Some(format!(
                            "Add a type annotation to field '{}', e.g. `{}: Type`",
                            name, name
                        )),
                    );
                }
            }
        }
        let fields: Vec<(String, Ty)> = decl
            .fields
            .iter()
            .map(|(name, type_ann)| {
                if let Some(te) = type_ann {
                    (name.clone(), self.type_expr_or_any(te))
                } else {
                    (name.clone(), Ty::Any)
                }
            })
            .collect();
        self.events.insert(
            decl.name.clone(),
            EventType {
                name: decl.name.clone(),
                is_pub: decl.is_pub,
                file_id: decl.span.file,
                fields,
            },
        );
    }
    pub(super) fn register_function(&mut self, decl: &FnDecl) {
        if decl.is_pub {
            for (idx, param) in decl.params.iter().enumerate() {
                if decl.param_types.get(idx).and_then(|t| t.as_ref()).is_none() {
                    self.error(
                        &decl.span,
                        format!(
                            "Public function '{}' requires explicit type annotations for all parameters",
                            decl.name
                        ),
                        Some(format!("Add a type annotation to parameter '{}', e.g. `{}: Type`", param, param)),
                    );
                }
            }
            if decl.return_type.is_none() {
                self.error(
                    &decl.span,
                    format!(
                        "Public function '{}' requires an explicit return type",
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
        let params = decl
            .params
            .iter()
            .enumerate()
            .map(|(idx, _)| {
                decl.param_types
                    .get(idx)
                    .and_then(|ann| ann.as_ref())
                    .map_or(Ty::Any, |te| self.type_expr_or_any(te))
            })
            .collect::<Vec<_>>();
        let ret = decl
            .return_type
            .as_ref()
            .map_or(Ty::Any, |te| self.type_expr_or_any(te));
        let ret = if decl.is_async {
            Ty::Task(Box::new(ret))
        } else {
            ret
        };
        self.pop_type_param_scope();
        let body_pure = self.block_is_conservatively_pure(&decl.body);
        let inferred_pure = !decl.is_pure && decl.effects.is_empty() && body_pure;
        if !decl.is_pure && !inferred_pure {
            if let Some(reason) = self.find_block_purity_breach(&decl.body) {
                self.purity_breach_reasons.insert(decl.name.clone(), reason);
            }
        }
        if decl.is_async && decl.is_pure {
            self.error(
                &decl.span,
                format!("Function '{}' cannot be both `async` and `pure`", decl.name),
                Some("Remove `pure` or make the function synchronous".to_string()),
            );
        }
        let effects = if decl.is_pure || inferred_pure {
            EffectSet::pure()
        } else if decl.effects.is_empty() {
            if decl.is_async {
                EffectSet::single(Effect::Async)
            } else if self.block_is_conservatively_readonly(&decl.body) {
                // Bodies that only READ the world (get/has/require/queries)
                // infer the readonly effect — the same allowance literal
                // closures get — so they compose into pipelines without an
                // annotation. Anything the walker can't vouch for stays
                // unrestricted.
                EffectSet::single(Effect::ReadECS)
            } else {
                EffectSet::unrestricted()
            }
        } else {
            let parsed: Vec<Effect> = decl
                .effects
                .iter()
                .filter_map(|e| Effect::from_name(e))
                .collect();
            let mut all = parsed;
            if decl.is_async && !all.contains(&Effect::Async) {
                all.push(Effect::Async);
            }
            EffectSet::from_vec(&all)
        };
        // A fn-typed parameter of an EXPLICITLY effect-annotated fn (pure/
        // readonly/io/ecs/event) is promoted to a `pure fn` type: the
        // annotation is the only contract the body can trust, so the callback
        // becomes part of it — callers must pass a pure function, which is
        // what makes the parameter safely callable inside the restricted body
        // at all. Inferred effect rows never promote (inference must not
        // change a signature the author didn't write). Closes the
        // higher-order laundering hole where `pure fn take(cb: fn() -> any)
        // { cb() }` compiled and ran a set_resource-calling callback.
        let params = if decl.is_pure || !decl.effects.is_empty() {
            // Promote bare `fn(...)` params to the strongest callback type
            // the declared row can actually CALL: readonly when the row
            // includes the readonly effect, pure otherwise. Explicit
            // `pure fn(...)` / `readonly fn(...)` annotations pass through
            // unchanged.
            let target = if effects.allows(Effect::ReadECS) {
                FnPurity::Readonly
            } else {
                FnPurity::Pure
            };
            Self::promote_callback_params(params, target)
        } else {
            params
        };
        self.fn_param_names
            .insert(decl.name.clone(), decl.params.clone());
        self.functions.insert(
            decl.name.clone(),
            FunctionSig {
                type_params: decl.type_params.clone(),
                params,
                ret,
                is_pure: decl.is_pure || inferred_pure || effects.is_pure(),
                effects,
            },
        );
    }

    /// See `register_function`: bare fn-typed params of explicitly
    /// effect-annotated fns are promoted to `target` (pure, or readonly for
    /// rows that include the readonly effect) in the signature (and, via the
    /// body pass in typeck, in the body scope). Explicitly written
    /// `pure fn(...)` / `readonly fn(...)` annotations are left alone.
    /// Top-level params only — anything nested is covered conservatively by
    /// the fn-value call boundary in typeck.
    pub(super) fn promote_callback_params(params: Vec<Ty>, target: FnPurity) -> Vec<Ty> {
        params
            .into_iter()
            .map(|p| match p {
                Ty::Fn {
                    params,
                    ret,
                    purity: FnPurity::Impure,
                } => Ty::Fn {
                    params,
                    ret,
                    purity: target,
                },
                other => other,
            })
            .collect()
    }

    /// Effect inference is order-dependent at registration: a call to a
    /// function declared LATER cannot be classified, so the caller
    /// degrades to unrestricted ("has IO effects") even when the callee
    /// is pure. Re-infer with the complete function table until stable —
    /// effects only narrow (pure ⊂ readonly ⊂ unrestricted), so this is
    /// a monotone fixpoint.
    pub(super) fn refine_fn_effects(&mut self, program: &Program) {
        fn rank(sig_pure: bool, effects: &EffectSet) -> u8 {
            if sig_pure || effects.is_pure() {
                0
            } else if *effects == EffectSet::single(Effect::ReadECS) {
                1
            } else {
                2
            }
        }
        for _ in 0..8 {
            let mut changed = false;
            for decl in &program.declarations {
                let Decl::Fn(f) = decl else { continue };
                // explicit annotations are contracts, not inferences
                if f.is_pure || !f.effects.is_empty() || f.is_async {
                    continue;
                }
                let Some(sig) = self.functions.get(&f.name) else {
                    continue;
                };
                let cur = rank(sig.is_pure, &sig.effects);
                if cur == 0 {
                    continue;
                }
                let new = if self.block_is_conservatively_pure(&f.body) {
                    0
                } else if self.block_is_conservatively_readonly(&f.body) {
                    1
                } else {
                    2
                };
                if new < cur {
                    if let Some(sig) = self.functions.get_mut(&f.name) {
                        match new {
                            0 => {
                                sig.is_pure = true;
                                sig.effects = EffectSet::pure();
                            }
                            _ => {
                                sig.effects = EffectSet::single(Effect::ReadECS);
                            }
                        }
                    }
                    if new == 0 {
                        self.purity_breach_reasons.remove(&f.name);
                    }
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    pub(super) fn register_type_alias(&mut self, decl: &TypeAliasDecl) {
        self.push_type_param_scope(&decl.type_params);
        let target = self.type_expr_or_any(&decl.target);
        self.pop_type_param_scope();
        self.type_aliases.insert(
            decl.name.clone(),
            TypeScheme {
                type_params: decl.type_params.clone(),
                is_pub: decl.is_pub,
                file_id: decl.span.file,
                ty: target,
            },
        );
    }

    pub(super) fn block_is_conservatively_pure(&self, block: &Block) -> bool {
        let mut local_muts = std::collections::HashSet::new();
        self.block_is_conservatively_pure_with_locals(block, &mut local_muts)
    }

    pub(super) fn block_is_conservatively_readonly(&self, block: &Block) -> bool {
        let mut local_muts = std::collections::HashSet::new();
        self.block_is_conservatively_readonly_with_locals(block, &mut local_muts)
    }

    fn seed_closure_local_muts(
        params: &[String],
        param_muts: &[bool],
        param_destructures: &[Option<Vec<String>>],
        local_muts: &mut std::collections::HashSet<String>,
    ) {
        for (i, param) in params.iter().enumerate() {
            if param_muts.get(i).copied().unwrap_or(false) {
                local_muts.insert(param.clone());
                if let Some(bindings) = param_destructures.get(i).and_then(|d| d.as_ref()) {
                    for name in bindings {
                        local_muts.insert(name.clone());
                    }
                }
            }
        }
    }

    pub(super) fn closure_body_is_conservatively_pure(
        &self,
        params: &[String],
        param_muts: &[bool],
        param_destructures: &[Option<Vec<String>>],
        body: &Block,
    ) -> bool {
        let mut local_muts = std::collections::HashSet::new();
        Self::seed_closure_local_muts(params, param_muts, param_destructures, &mut local_muts);
        self.block_is_conservatively_pure_with_locals(body, &mut local_muts)
    }

    pub(super) fn closure_body_is_conservatively_readonly(
        &self,
        params: &[String],
        param_muts: &[bool],
        param_destructures: &[Option<Vec<String>>],
        body: &Block,
    ) -> bool {
        let mut local_muts = std::collections::HashSet::new();
        Self::seed_closure_local_muts(params, param_muts, param_destructures, &mut local_muts);
        self.block_is_conservatively_readonly_with_locals(body, &mut local_muts)
    }

    fn block_is_conservatively_pure_with_locals(
        &self,
        block: &Block,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> bool {
        block
            .stmts
            .iter()
            .all(|stmt| self.stmt_is_conservatively_pure(stmt, local_muts))
    }

    fn block_is_conservatively_readonly_with_locals(
        &self,
        block: &Block,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> bool {
        block
            .stmts
            .iter()
            .all(|stmt| self.stmt_is_conservatively_readonly(stmt, local_muts))
    }

    fn stmt_is_conservatively_pure(
        &self,
        stmt: &Stmt,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> bool {
        match stmt {
            Stmt::Let(s) => {
                if s.mutable {
                    for n in &s.names {
                        local_muts.insert(n.clone());
                    }
                }
                self.expr_is_conservatively_pure(&s.value)
            }
            Stmt::LetElse(le) => {
                if le.mutable {
                    if let Some(name) = le.primary_binding_name() {
                        local_muts.insert(name);
                    }
                }
                self.expr_is_conservatively_pure(&le.subject)
                    && self.block_is_conservatively_pure_with_locals(&le.else_block, local_muts)
            }
            Stmt::Assign(s) => {
                if let Expr::Ident(name, _) = &s.target {
                    if local_muts.contains(name) {
                        return self.expr_is_conservatively_pure(&s.value);
                    }
                }
                false
            }
            Stmt::If(s) => {
                self.expr_is_conservatively_pure(&s.condition)
                    && self.block_is_conservatively_pure_with_locals(&s.then_block, local_muts)
                    && s.else_block
                        .as_ref()
                        .map(|b| self.block_is_conservatively_pure_with_locals(b, local_muts))
                        .unwrap_or(true)
            }
            Stmt::While(s) => {
                self.expr_is_conservatively_pure(&s.condition)
                    && self.block_is_conservatively_pure_with_locals(&s.body, local_muts)
            }
            Stmt::For(s) => {
                for binding in &s.bindings {
                    local_muts.insert(binding.clone());
                }
                self.expr_is_conservatively_pure(&s.iterable)
                    && self.block_is_conservatively_pure_with_locals(&s.body, local_muts)
            }
            Stmt::Return(s) => s
                .value
                .as_ref()
                .map(|e| self.expr_is_conservatively_pure(e))
                .unwrap_or(true),
            Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::Emit(_) | Stmt::Schedule(_) | Stmt::Update(_) | Stmt::Settle(_) => false,
            Stmt::Propose(s) => s
                .fields
                .iter()
                .all(|(_, expr)| self.expr_is_conservatively_pure(expr)),
            Stmt::Next(s) => {
                self.expr_is_conservatively_pure(&s.entity)
                    && s.fields
                        .iter()
                        .all(|(_, expr)| self.expr_is_conservatively_pure(expr))
            }
            Stmt::Require(s) => self.expr_is_conservatively_pure(&s.condition),
            Stmt::Match(m) => {
                self.expr_is_conservatively_pure(&m.subject)
                    && m.cases.iter().all(|case| {
                        (match &case.pattern {
                            Pattern::Literal(e) => self.expr_is_conservatively_pure(e),
                            _ => true,
                        }) && case
                            .guard
                            .as_ref()
                            .map(|e| self.expr_is_conservatively_pure(e))
                            .unwrap_or(true)
                            && self.block_is_conservatively_pure_with_locals(&case.body, local_muts)
                    })
            }
            Stmt::Expr(s) => self.expr_is_conservatively_pure(&s.expr),
            Stmt::OnceGuardPass(_) | Stmt::Error(_) => true,
        }
    }}