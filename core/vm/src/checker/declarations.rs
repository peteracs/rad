use super::*;
use crate::ast::*;
use crate::types::*;
use std::collections::HashMap;
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
            Stmt::Emit(_) | Stmt::Schedule(_) | Stmt::Update(_) => false,
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
    }

    fn stmt_is_conservatively_readonly(
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
                self.expr_is_conservatively_readonly(&s.value)
            }
            Stmt::LetElse(le) => {
                if le.mutable {
                    if let Some(name) = le.primary_binding_name() {
                        local_muts.insert(name);
                    }
                }
                self.expr_is_conservatively_readonly(&le.subject)
                    && self.block_is_conservatively_readonly_with_locals(&le.else_block, local_muts)
            }
            Stmt::Assign(s) => {
                if let Expr::Ident(name, _) = &s.target {
                    if local_muts.contains(name) {
                        return self.expr_is_conservatively_readonly(&s.value);
                    }
                }
                false
            }
            Stmt::If(s) => {
                self.expr_is_conservatively_readonly(&s.condition)
                    && self.block_is_conservatively_readonly_with_locals(&s.then_block, local_muts)
                    && s.else_block
                        .as_ref()
                        .map(|b| self.block_is_conservatively_readonly_with_locals(b, local_muts))
                        .unwrap_or(true)
            }
            Stmt::While(s) => {
                self.expr_is_conservatively_readonly(&s.condition)
                    && self.block_is_conservatively_readonly_with_locals(&s.body, local_muts)
            }
            Stmt::For(s) => {
                for binding in &s.bindings {
                    local_muts.insert(binding.clone());
                }
                self.expr_is_conservatively_readonly(&s.iterable)
                    && self.block_is_conservatively_readonly_with_locals(&s.body, local_muts)
            }
            Stmt::Return(s) => s
                .value
                .as_ref()
                .map(|e| self.expr_is_conservatively_readonly(e))
                .unwrap_or(true),
            Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::Emit(_) | Stmt::Schedule(_) | Stmt::Update(_) => false,
            Stmt::Match(m) => {
                self.expr_is_conservatively_readonly(&m.subject)
                    && m.cases.iter().all(|case| {
                        (match &case.pattern {
                            Pattern::Literal(e) => self.expr_is_conservatively_readonly(e),
                            _ => true,
                        }) && case
                            .guard
                            .as_ref()
                            .map(|e| self.expr_is_conservatively_readonly(e))
                            .unwrap_or(true)
                            && self.block_is_conservatively_readonly_with_locals(
                                &case.body, local_muts,
                            )
                    })
            }
            Stmt::Expr(s) => self.expr_is_conservatively_readonly(&s.expr),
            Stmt::OnceGuardPass(_) | Stmt::Error(_) => true,
        }
    }

    fn expr_is_conservatively_pure(&self, expr: &Expr) -> bool {
        match expr {
            Expr::IntLit(_, _)
            | Expr::FloatLit(_, _)
            | Expr::StrLit(_, _)
            | Expr::BoolLit(_, _)
            | Expr::NilLit(_)
            | Expr::Ident(_, _)
            | Expr::StateRef(_, _, _)
            | Expr::SystemRef(_, _)
            | Expr::QueryExpr(_, _) => true,
            Expr::TupleLit(items, _) => items.iter().all(|e| self.expr_is_conservatively_pure(e)),
            Expr::Spread(expr, _) => self.expr_is_conservatively_pure(expr),
            Expr::ListLit(items, _) => items.iter().all(|e| self.expr_is_conservatively_pure(e)),
            Expr::MapLit(entries, _) => entries.iter().all(|(k, v)| {
                self.expr_is_conservatively_pure(k) && self.expr_is_conservatively_pure(v)
            }),
            Expr::FStringExpr(parts, _) => parts.iter().all(|part| match part {
                FStringPart::Lit(_) => true,
                FStringPart::Expr(e, _) => self.expr_is_conservatively_pure(e),
            }),
            Expr::Binary(l, _, r, _) => {
                self.expr_is_conservatively_pure(l) && self.expr_is_conservatively_pure(r)
            }
            Expr::Unary(_, e, _) => self.expr_is_conservatively_pure(e),
            Expr::Pipe(l, r, _) => {
                self.expr_is_conservatively_pure(l) && self.expr_is_conservatively_pure(r)
            }
            Expr::Call(callee, args, _) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if super::diagnostics::is_impure_builtin(name) {
                        return false;
                    }
                    if crate::builtins::builtin_type_scheme(name)
                        .map(|sig| !sig.is_pure)
                        .unwrap_or(false)
                    {
                        return false;
                    }
                    if let Some(sig) = self.functions.get(name) {
                        if !sig.is_pure {
                            return false;
                        }
                    } else if !crate::builtins::is_builtin(name) {
                        return false;
                    }
                } else {
                    return false;
                }
                self.expr_is_conservatively_pure(callee)
                    && args.iter().all(|e| self.expr_is_conservatively_pure(e))
            }
            Expr::Field(obj, _, _) => self.expr_is_conservatively_pure(obj),
            Expr::Index(obj, idx, _) => {
                self.expr_is_conservatively_pure(obj) && self.expr_is_conservatively_pure(idx)
            }
            Expr::ComponentExpr(_, fields, rest, _) => {
                fields
                    .iter()
                    .all(|(_, e)| self.expr_is_conservatively_pure(e))
                    && rest
                        .as_ref()
                        .map(|e| self.expr_is_conservatively_pure(e))
                        .unwrap_or(true)
            }
            Expr::VariantExpr(_, _, fields, _) => fields
                .iter()
                .all(|(_, e)| self.expr_is_conservatively_pure(e)),
            Expr::MatchExpr(m, _) => {
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
                            && self.block_is_conservatively_pure(&case.body)
                    })
            }
            Expr::IfExpr(c, t, e, _) => {
                self.expr_is_conservatively_pure(c)
                    && self.expr_is_conservatively_pure(t)
                    && self.expr_is_conservatively_pure(e)
            }
            Expr::FnExpr(_, _, _, _, _, body, _) => self.block_is_conservatively_pure(body),
            Expr::Await(_, _) | Expr::AsyncCall(_, _, _) => false,
            Expr::Try(e, _) => self.expr_is_conservatively_pure(e),
            Expr::EntityLiteral(_, _, _) => false,
            Expr::Error(_) => true,
        }
    }

    fn expr_is_conservatively_readonly(&self, expr: &Expr) -> bool {
        match expr {
            Expr::IntLit(_, _)
            | Expr::FloatLit(_, _)
            | Expr::StrLit(_, _)
            | Expr::BoolLit(_, _)
            | Expr::NilLit(_)
            | Expr::Ident(_, _)
            | Expr::StateRef(_, _, _)
            | Expr::SystemRef(_, _)
            | Expr::QueryExpr(_, _) => true,
            Expr::TupleLit(items, _) => items
                .iter()
                .all(|e| self.expr_is_conservatively_readonly(e)),
            Expr::Spread(expr, _) => self.expr_is_conservatively_readonly(expr),
            Expr::ListLit(items, _) => items
                .iter()
                .all(|e| self.expr_is_conservatively_readonly(e)),
            Expr::MapLit(entries, _) => entries.iter().all(|(k, v)| {
                self.expr_is_conservatively_readonly(k) && self.expr_is_conservatively_readonly(v)
            }),
            Expr::FStringExpr(parts, _) => parts.iter().all(|part| match part {
                FStringPart::Lit(_) => true,
                FStringPart::Expr(e, _) => self.expr_is_conservatively_readonly(e),
            }),
            Expr::Binary(l, _, r, _) => {
                self.expr_is_conservatively_readonly(l) && self.expr_is_conservatively_readonly(r)
            }
            Expr::Unary(_, e, _) => self.expr_is_conservatively_readonly(e),
            Expr::Pipe(l, r, _) => {
                self.expr_is_conservatively_readonly(l) && self.expr_is_conservatively_readonly(r)
            }
            Expr::Call(callee, args, _) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if super::diagnostics::is_impure_builtin(name) {
                        return false;
                    }
                    if !super::diagnostics::is_readonly_builtin(name)
                        && crate::builtins::builtin_type_scheme(name)
                            .map(|sig| !sig.is_pure)
                            .unwrap_or(false)
                    {
                        return false;
                    }
                    if let Some(sig) = self.functions.get(name) {
                        if !(sig.is_pure || sig.effects.is_readonly()) {
                            return false;
                        }
                    } else if !crate::builtins::is_builtin(name) {
                        return false;
                    }
                } else {
                    return false;
                }
                self.expr_is_conservatively_readonly(callee)
                    && args.iter().all(|e| self.expr_is_conservatively_readonly(e))
            }
            Expr::Field(obj, _, _) => self.expr_is_conservatively_readonly(obj),
            Expr::Index(obj, idx, _) => {
                self.expr_is_conservatively_readonly(obj)
                    && self.expr_is_conservatively_readonly(idx)
            }
            Expr::ComponentExpr(_, fields, rest, _) => {
                fields
                    .iter()
                    .all(|(_, e)| self.expr_is_conservatively_readonly(e))
                    && rest
                        .as_ref()
                        .map(|e| self.expr_is_conservatively_readonly(e))
                        .unwrap_or(true)
            }
            Expr::VariantExpr(_, _, fields, _) => fields
                .iter()
                .all(|(_, e)| self.expr_is_conservatively_readonly(e)),
            Expr::MatchExpr(m, _) => {
                self.expr_is_conservatively_readonly(&m.subject)
                    && m.cases.iter().all(|case| {
                        (match &case.pattern {
                            Pattern::Literal(e) => self.expr_is_conservatively_readonly(e),
                            _ => true,
                        }) && case
                            .guard
                            .as_ref()
                            .map(|e| self.expr_is_conservatively_readonly(e))
                            .unwrap_or(true)
                            && self.block_is_conservatively_readonly(&case.body)
                    })
            }
            Expr::IfExpr(c, t, e, _) => {
                self.expr_is_conservatively_readonly(c)
                    && self.expr_is_conservatively_readonly(t)
                    && self.expr_is_conservatively_readonly(e)
            }
            Expr::FnExpr(_, _, _, _, _, body, _) => self.block_is_conservatively_readonly(body),
            Expr::Await(_, _) | Expr::AsyncCall(_, _, _) => false,
            Expr::Try(e, _) => self.expr_is_conservatively_readonly(e),
            Expr::EntityLiteral(_, _, _) => false,
            Expr::Error(_) => true,
        }
    }

    /// Like `block_is_conservatively_pure`, but returns a human-readable
    /// description of the *first* purity breach found (or `None` if pure).
    pub(super) fn find_block_purity_breach(&self, block: &Block) -> Option<String> {
        let mut local_muts = std::collections::HashSet::new();
        self.find_block_purity_breach_with_locals(block, &mut local_muts)
    }

    /// Like [`find_block_purity_breach`], but read-only ECS builtins and
    /// `readonly fn` calls are NOT breaches. This is the `query_where`
    /// predicate contract: world READS during iteration are safe (the
    /// entity list is snapshotted before the predicate runs) — only writes,
    /// IO, events, and unverifiable calls are rejected.
    pub(super) fn find_block_readonly_breach(&self, block: &Block) -> Option<String> {
        self.purity_allow_read_ecs.set(true);
        let breach = self.find_block_purity_breach(block);
        self.purity_allow_read_ecs.set(false);
        breach
    }

    /// Does this effect set stay within reads? (`Unrestricted` does not —
    /// it allows everything.)
    pub(super) fn effect_set_is_read_only(effects: &crate::types::EffectSet) -> bool {
        use crate::types::Effect;
        !(effects.allows(Effect::IO)
            || effects.allows(Effect::ECS)
            || effects.allows(Effect::Event)
            || effects.allows(Effect::Async))
    }

    fn find_block_purity_breach_with_locals(
        &self,
        block: &Block,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> Option<String> {
        for stmt in &block.stmts {
            if let Some(reason) = self.find_stmt_purity_breach(stmt, local_muts) {
                return Some(reason);
            }
        }
        None
    }

    fn find_stmt_purity_breach(
        &self,
        stmt: &Stmt,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> Option<String> {
        match stmt {
            Stmt::Let(s) => {
                if s.mutable {
                    for n in &s.names {
                        local_muts.insert(n.clone());
                    }
                }
                self.find_expr_purity_breach(&s.value)
            }
            Stmt::LetElse(le) => {
                if le.mutable {
                    if let Some(name) = le.primary_binding_name() {
                        local_muts.insert(name);
                    }
                }
                self.find_expr_purity_breach(&le.subject).or_else(|| {
                    self.find_block_purity_breach_with_locals(&le.else_block, local_muts)
                })
            }
            Stmt::Assign(s) => {
                if let Expr::Ident(name, _) = &s.target {
                    if local_muts.contains(name) {
                        return self.find_expr_purity_breach(&s.value);
                    }
                }
                Some("mutates a non-local variable".to_string())
            }
            Stmt::If(s) => self
                .find_expr_purity_breach(&s.condition)
                .or_else(|| self.find_block_purity_breach_with_locals(&s.then_block, local_muts))
                .or_else(|| {
                    s.else_block
                        .as_ref()
                        .and_then(|b| self.find_block_purity_breach_with_locals(b, local_muts))
                }),
            Stmt::While(s) => self
                .find_expr_purity_breach(&s.condition)
                .or_else(|| self.find_block_purity_breach_with_locals(&s.body, local_muts)),
            Stmt::For(s) => {
                for binding in &s.bindings {
                    local_muts.insert(binding.clone());
                }
                self.find_expr_purity_breach(&s.iterable)
                    .or_else(|| self.find_block_purity_breach_with_locals(&s.body, local_muts))
            }
            Stmt::Return(s) => s
                .value
                .as_ref()
                .and_then(|e| self.find_expr_purity_breach(e)),
            Stmt::Break(_) | Stmt::Continue(_) => None,
            Stmt::Emit(_) => Some("emits an event".to_string()),
            Stmt::Schedule(_) => Some("runs a system schedule".to_string()),
            Stmt::Update(_) => Some("calls impure builtin 'set'".to_string()),
            Stmt::Match(m) => self.find_expr_purity_breach(&m.subject).or_else(|| {
                m.cases.iter().find_map(|case| {
                    case.guard
                        .as_ref()
                        .and_then(|e| self.find_expr_purity_breach(e))
                        .or_else(|| {
                            self.find_block_purity_breach_with_locals(&case.body, local_muts)
                        })
                })
            }),
            Stmt::Expr(s) => self.find_expr_purity_breach(&s.expr),
            Stmt::OnceGuardPass(_) | Stmt::Error(_) => None,
        }
    }

    fn find_expr_purity_breach(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::IntLit(_, _)
            | Expr::FloatLit(_, _)
            | Expr::StrLit(_, _)
            | Expr::BoolLit(_, _)
            | Expr::NilLit(_)
            | Expr::Ident(_, _)
            | Expr::StateRef(_, _, _)
            | Expr::SystemRef(_, _)
            | Expr::QueryExpr(_, _) => None,
            Expr::TupleLit(items, _) => items.iter().find_map(|e| self.find_expr_purity_breach(e)),
            Expr::Spread(expr, _) => self.find_expr_purity_breach(expr),
            Expr::ListLit(items, _) => items.iter().find_map(|e| self.find_expr_purity_breach(e)),
            Expr::MapLit(entries, _) => entries.iter().find_map(|(k, v)| {
                self.find_expr_purity_breach(k)
                    .or_else(|| self.find_expr_purity_breach(v))
            }),
            Expr::FStringExpr(parts, _) => parts.iter().find_map(|part| match part {
                FStringPart::Lit(_) => None,
                FStringPart::Expr(e, _) => self.find_expr_purity_breach(e),
            }),
            Expr::Binary(l, _, r, _) => self
                .find_expr_purity_breach(l)
                .or_else(|| self.find_expr_purity_breach(r)),
            Expr::Unary(_, e, _) => self.find_expr_purity_breach(e),
            Expr::Pipe(l, r, _) => self
                .find_expr_purity_breach(l)
                .or_else(|| self.find_expr_purity_breach(r)),
            Expr::Call(callee, args, _) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    // Read-tolerant mode (`find_block_readonly_breach`):
                    // world reads are not breaches. The read set is the
                    // curated `is_readonly_builtin` list, which shares no
                    // member with `is_impure_builtin` (rand_*, IO, writes,
                    // fork/commit all stay breaches).
                    let read_ok = self.purity_allow_read_ecs.get()
                        && super::diagnostics::is_readonly_builtin(name);
                    if !read_ok {
                        if super::diagnostics::is_impure_builtin(name) {
                            return Some(format!("calls impure builtin '{}'", name));
                        }
                        if crate::builtins::builtin_type_scheme(name)
                            .map(|sig| !sig.is_pure)
                            .unwrap_or(false)
                        {
                            return Some(format!("calls impure builtin '{}'", name));
                        }
                    }
                    if let Some(sig) = self.functions.get(name) {
                        let readonly_fn_ok = self.purity_allow_read_ecs.get()
                            && Self::effect_set_is_read_only(&sig.effects);
                        if !sig.is_pure && !readonly_fn_ok {
                            if let Some(inner) = self.purity_breach_reasons.get(name) {
                                return Some(format!("calls '{}' which {}", name, inner));
                            }
                            return Some(format!(
                                "calls '{}' which is not declared `pure fn`",
                                name
                            ));
                        }
                    } else if !crate::builtins::is_builtin(name) {
                        return Some(format!(
                            "calls '{}' whose purity could not be verified",
                            name
                        ));
                    }
                } else {
                    return Some("calls a dynamic (non-named) function".to_string());
                }
                args.iter().find_map(|e| self.find_expr_purity_breach(e))
            }
            Expr::Field(obj, _, _) => self.find_expr_purity_breach(obj),
            Expr::Index(obj, idx, _) => self
                .find_expr_purity_breach(obj)
                .or_else(|| self.find_expr_purity_breach(idx)),
            Expr::ComponentExpr(_, fields, rest, _) => fields
                .iter()
                .find_map(|(_, e)| self.find_expr_purity_breach(e))
                .or_else(|| rest.as_ref().and_then(|e| self.find_expr_purity_breach(e))),
            Expr::VariantExpr(_, _, fields, _) => fields
                .iter()
                .find_map(|(_, e)| self.find_expr_purity_breach(e)),
            Expr::MatchExpr(m, _) => self.find_expr_purity_breach(&m.subject).or_else(|| {
                m.cases.iter().find_map(|case| {
                    case.guard
                        .as_ref()
                        .and_then(|e| self.find_expr_purity_breach(e))
                        .or_else(|| self.find_block_purity_breach(&case.body))
                })
            }),
            Expr::IfExpr(c, t, e, _) => self
                .find_expr_purity_breach(c)
                .or_else(|| self.find_expr_purity_breach(t))
                .or_else(|| self.find_expr_purity_breach(e)),
            Expr::FnExpr(_, _, _, _, _, body, _) => self.find_block_purity_breach(body),
            Expr::Await(_, _) | Expr::AsyncCall(_, _, _) => Some("uses async/await".to_string()),
            Expr::Try(e, _) => self.find_expr_purity_breach(e),
            Expr::EntityLiteral(_, _, _) => Some("spawns an entity".to_string()),
            Expr::Error(_) => None,
        }
    }

    /// Like `find_block_purity_breach`, but permits ECS mutations (set, spawn,
    /// remove, despawn). Returns `Some(reason)` only when the block performs IO
    /// or Event effects — the operations forbidden inside `simulate()`.
    fn assign_target_root_ident(expr: &Expr) -> Option<&str> {
        match expr {
            Expr::Ident(name, _) => Some(name),
            Expr::Field(inner, _, _) | Expr::Index(inner, _, _) => {
                Self::assign_target_root_ident(inner)
            }
            _ => None,
        }
    }

    /// Simulation safety for a SYSTEM body: the body itself must be
    /// breach-free, and so must every handler transitively reachable
    /// through the events it emits (handlers may emit further events).
    pub(super) fn find_system_sim_breach(
        &self,
        block: &Block,
        sys_mut_params: std::collections::HashSet<String>,
    ) -> Option<String> {
        if let Some(reason) = self.find_block_simulation_breach(block, sys_mut_params) {
            return Some(reason);
        }
        let mut queue = Vec::new();
        Self::collect_emits_in_block(block, &mut queue);
        let mut seen = std::collections::HashSet::new();
        while let Some(ev) = queue.pop() {
            if !seen.insert(ev.clone()) {
                continue;
            }
            // raw name, canonical resolution, and last segment all match
            // (cross-module emits may use any of the three spellings)
            let mut names = vec![ev.clone(), self.resolve_canonical_name(&ev)];
            if let Some(last) = ev.rsplit('.').next() {
                names.push(last.to_string());
            }
            names.dedup();
            for key in names {
                if let Some(blocks) = self.event_handler_blocks.get(&key) {
                    for hb in blocks {
                        if let Some(reason) =
                            self.find_block_simulation_breach(hb, std::collections::HashSet::new())
                        {
                            return Some(format!("handler `on {}` {}", key, reason));
                        }
                        Self::collect_emits_in_block(hb, &mut queue);
                    }
                }
            }
        }
        None
    }

    fn collect_emits_in_block(block: &Block, out: &mut Vec<String>) {
        use crate::visitor::AstVisitor;
        struct EmitCollector<'a> {
            out: &'a mut Vec<String>,
        }
        impl<'a> AstVisitor for EmitCollector<'a> {
            fn visit_stmt(&mut self, stmt: &Stmt) {
                if let Stmt::Emit(e) = stmt {
                    self.out.push(e.event_name.clone());
                }
                crate::visitor::walk_stmt(self, stmt);
            }
        }
        let mut c = EmitCollector { out };
        c.visit_block(block);
    }

    pub(super) fn find_block_simulation_breach(
        &self,
        block: &Block,
        initial_muts: std::collections::HashSet<String>,
    ) -> Option<String> {
        let mut local_muts = initial_muts;
        self.find_block_sim_breach_with_locals(block, &mut local_muts)
    }

    fn find_block_sim_breach_with_locals(
        &self,
        block: &Block,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> Option<String> {
        for stmt in &block.stmts {
            if let Some(reason) = self.find_stmt_sim_breach(stmt, local_muts) {
                return Some(reason);
            }
        }
        None
    }

    fn find_stmt_sim_breach(
        &self,
        stmt: &Stmt,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> Option<String> {
        match stmt {
            Stmt::Let(s) => {
                if s.mutable {
                    for n in &s.names {
                        local_muts.insert(n.clone());
                    }
                }
                self.find_expr_sim_breach(&s.value)
            }
            Stmt::LetElse(le) => {
                if le.mutable {
                    if let Some(name) = le.primary_binding_name() {
                        local_muts.insert(name);
                    }
                }
                self.find_expr_sim_breach(&le.subject)
                    .or_else(|| self.find_block_sim_breach_with_locals(&le.else_block, local_muts))
            }
            Stmt::Assign(s) => {
                let root = Self::assign_target_root_ident(&s.target);
                if let Some(name) = root {
                    if local_muts.contains(name) {
                        return self.find_expr_sim_breach(&s.value);
                    }
                }
                Some("mutates a non-local variable".to_string())
            }
            Stmt::If(s) => self
                .find_expr_sim_breach(&s.condition)
                .or_else(|| self.find_block_sim_breach_with_locals(&s.then_block, local_muts))
                .or_else(|| {
                    s.else_block
                        .as_ref()
                        .and_then(|b| self.find_block_sim_breach_with_locals(b, local_muts))
                }),
            Stmt::While(s) => self
                .find_expr_sim_breach(&s.condition)
                .or_else(|| self.find_block_sim_breach_with_locals(&s.body, local_muts)),
            Stmt::For(s) => {
                for binding in &s.bindings {
                    local_muts.insert(binding.clone());
                }
                self.find_expr_sim_breach(&s.iterable)
                    .or_else(|| self.find_block_sim_breach_with_locals(&s.body, local_muts))
            }
            Stmt::Return(s) => s.value.as_ref().and_then(|e| self.find_expr_sim_breach(e)),
            Stmt::Break(_) | Stmt::Continue(_) => None,
            // `emit` is legal in simulation: events dispatch inside the
            // fork (the VM flushes per simulated tick). Safety moves to
            // the handlers — find_system_sim_breach walks them.
            Stmt::Emit(e) => e
                .fields
                .iter()
                .find_map(|(_, expr)| self.find_expr_sim_breach(expr))
                .or_else(|| e.delay.as_ref().and_then(|d| self.find_expr_sim_breach(d))),
            Stmt::Schedule(_) | Stmt::Update(_) => None,
            Stmt::Match(m) => self.find_expr_sim_breach(&m.subject).or_else(|| {
                m.cases.iter().find_map(|case| {
                    case.guard
                        .as_ref()
                        .and_then(|e| self.find_expr_sim_breach(e))
                        .or_else(|| self.find_block_sim_breach_with_locals(&case.body, local_muts))
                })
            }),
            Stmt::Expr(s) => self.find_expr_sim_breach(&s.expr),
            Stmt::OnceGuardPass(_) | Stmt::Error(_) => None,
        }
    }

    fn is_io_builtin(name: &str) -> bool {
        matches!(
            name,
            "print"
                | "eprint"
                | "write_stdout"
                | "write_stderr"
                | "flush_stdout"
                | "input"
                | "readline"
                | "read_stdin_all"
                | "read_file"
                | "write_file"
                | "append_file"
                | "file_exists"
                | "remove_file"
                | "list_dir"
                | "create_dir"
                | "remove_dir"
                | "read_file_bytes"
                | "write_file_bytes"
                | "http_get"
                | "http_post"
                | "http_post_json"
                | "http_request"
                | "tcp_connect"
                | "tcp_listen"
                | "tcp_accept"
                | "tcp_accept_timeout"
                | "tcp_read"
                | "tcp_write"
                | "tcp_close"
                | "udp_bind"
                | "udp_recv_from"
                | "udp_recv_from_timeout"
                | "udp_recv_from_bytes"
                | "udp_recv_from_bytes_timeout"
                | "udp_recv_bytebuf"
                | "udp_recv_bytebuf_timeout"
                | "udp_send_to"
                | "udp_send_to_bytes"
                | "udp_send_bytebuf"
                | "udp_close"
                | "now_unix_s"
                | "now_unix_ms"
                | "clock"
                | "rand_int"
                | "rand_float"
                | "rand_bool"
                | "rand_seed"
                | "load_extension"
                | "log"
                | "metric"
        )
    }

    /// Draw-from-the-stream randomness only. `rand_seed` stays banned even
    /// under the lenient walk: re-seeding inside a speculated system would
    /// collapse `simulate_par`'s per-fork divergence.
    fn is_rand_builtin(name: &str) -> bool {
        matches!(name, "rand_int" | "rand_float" | "rand_bool")
    }

    fn is_event_builtin(name: &str) -> bool {
        matches!(name, "emit" | "transition")
    }

    fn is_simulation_unsafe_builtin(name: &str) -> bool {
        matches!(name, "commit")
    }

    fn find_expr_sim_breach(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::IntLit(_, _)
            | Expr::FloatLit(_, _)
            | Expr::StrLit(_, _)
            | Expr::BoolLit(_, _)
            | Expr::NilLit(_)
            | Expr::Ident(_, _)
            | Expr::StateRef(_, _, _)
            | Expr::SystemRef(_, _)
            | Expr::QueryExpr(_, _) => None,
            Expr::TupleLit(items, _) => items.iter().find_map(|e| self.find_expr_sim_breach(e)),
            Expr::Spread(expr, _) => self.find_expr_sim_breach(expr),
            Expr::ListLit(items, _) => items.iter().find_map(|e| self.find_expr_sim_breach(e)),
            Expr::MapLit(entries, _) => entries.iter().find_map(|(k, v)| {
                self.find_expr_sim_breach(k)
                    .or_else(|| self.find_expr_sim_breach(v))
            }),
            Expr::FStringExpr(parts, _) => parts.iter().find_map(|part| match part {
                FStringPart::Lit(_) => None,
                FStringPart::Expr(e, _) => self.find_expr_sim_breach(e),
            }),
            Expr::Binary(l, _, r, _) => self
                .find_expr_sim_breach(l)
                .or_else(|| self.find_expr_sim_breach(r)),
            Expr::Unary(_, e, _) => self.find_expr_sim_breach(e),
            Expr::Pipe(l, r, _) => self
                .find_expr_sim_breach(l)
                .or_else(|| self.find_expr_sim_breach(r)),
            Expr::Call(callee, args, _) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if Self::is_io_builtin(name)
                        && !(self.sim_breach_allow_rand && Self::is_rand_builtin(name))
                    {
                        return Some(format!(
                            "calls IO builtin '{}' (forbidden in simulation)",
                            name
                        ));
                    }
                    if Self::is_event_builtin(name) {
                        return Some(format!(
                            "calls event builtin '{}' (forbidden in simulation)",
                            name
                        ));
                    }
                    if Self::is_simulation_unsafe_builtin(name) {
                        return Some(format!(
                            "calls '{}' (forbidden in simulation — would corrupt the forked world)",
                            name
                        ));
                    }
                    if let Some(sig) = self.functions.get(name) {
                        if sig.effects.allows(crate::types::Effect::IO) && !sig.effects.is_pure() {
                            return Some(format!("calls '{}' which has IO effects", name));
                        }
                        if sig.effects.allows(crate::types::Effect::Event) && !sig.effects.is_pure()
                        {
                            return Some(format!("calls '{}' which has Event effects", name));
                        }
                    }
                }
                args.iter().find_map(|e| self.find_expr_sim_breach(e))
            }
            Expr::Field(obj, _, _) => self.find_expr_sim_breach(obj),
            Expr::Index(obj, idx, _) => self
                .find_expr_sim_breach(obj)
                .or_else(|| self.find_expr_sim_breach(idx)),
            Expr::ComponentExpr(_, fields, rest, _) => fields
                .iter()
                .find_map(|(_, e)| self.find_expr_sim_breach(e))
                .or_else(|| rest.as_ref().and_then(|e| self.find_expr_sim_breach(e))),
            Expr::VariantExpr(_, _, fields, _) => fields
                .iter()
                .find_map(|(_, e)| self.find_expr_sim_breach(e)),
            Expr::MatchExpr(m, _) => self.find_expr_sim_breach(&m.subject).or_else(|| {
                m.cases.iter().find_map(|case| {
                    case.guard
                        .as_ref()
                        .and_then(|e| self.find_expr_sim_breach(e))
                        .or_else(|| {
                            self.find_block_simulation_breach(
                                &case.body,
                                std::collections::HashSet::new(),
                            )
                        })
                })
            }),
            Expr::IfExpr(c, t, e, _) => self
                .find_expr_sim_breach(c)
                .or_else(|| self.find_expr_sim_breach(t))
                .or_else(|| self.find_expr_sim_breach(e)),
            Expr::FnExpr(_, _, _, _, _, body, _) => {
                self.find_block_simulation_breach(body, std::collections::HashSet::new())
            }
            Expr::Await(_, _) | Expr::AsyncCall(_, _, _) => {
                Some("uses async/await (forbidden in simulation)".to_string())
            }
            Expr::Try(e, _) => self.find_expr_sim_breach(e),
            Expr::EntityLiteral(_, _, _) => None,
            Expr::Error(_) => None,
        }
    }

    fn check_system_cycles(&mut self) {
        let names: Vec<String> = self.systems.keys().cloned().collect();
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        for name in &names {
            graph.insert(name.clone(), Vec::new());
        }
        let mut system_spans: HashMap<String, Span> = HashMap::new();
        for (name, (after, before, span)) in &self.system_deps {
            system_spans.insert(name.clone(), span.clone());
            for dep in after {
                if self.systems.contains_key(dep) {
                    graph.entry(name.clone()).or_default().push(dep.clone());
                }
            }
            for dep in before {
                if self.systems.contains_key(dep) {
                    graph.entry(dep.clone()).or_default().push(name.clone());
                }
            }
        }
        let mut visited: HashMap<String, u8> = HashMap::new();
        for name in &names {
            if self.dfs_cycle(name, &graph, &mut visited) {
                let span = system_spans.get(name).cloned().unwrap_or_default();
                self.errors.push(TypeError {
                    line: span.line,
                    col: span.col,
                    file: span.file,
                    message: format!("Circular system dependency detected involving '{}'", name),
                    hint: Some("Check after/before declarations for cycles".to_string()),
                });
                break;
            }
        }
    }
    fn dfs_cycle(
        &self,
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        visited: &mut HashMap<String, u8>,
    ) -> bool {
        match visited.get(node) {
            Some(1) => return true,
            Some(2) => return false,
            _ => {}
        }
        visited.insert(node.to_string(), 1);
        if let Some(deps) = graph.get(node) {
            for dep in deps {
                if self.dfs_cycle(dep, graph, visited) {
                    return true;
                }
            }
        }
        visited.insert(node.to_string(), 2);
        false
    }
}
