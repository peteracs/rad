use super::*;

impl Compiler {
    fn canonical_phase(&self, phase: &PhaseDecl) -> (String, Vec<String>) {
        (
            self.resolve_canonical_name(&phase.name),
            phase
                .systems
                .iter()
                .map(|system| self.resolve_canonical_name(system))
                .collect(),
        )
    }

    pub(crate) fn compile_decl(&mut self, decl: &Decl) -> Result<(), CompileError> {
        let prev_file_scope = self.current_file_scope.clone();
        if let Some(span) = decl.span() {
            if let Some(file_id) = span.file {
                if let Some(scope) = self.file_private_scopes.get(&file_id.0).cloned() {
                    self.current_file_scope = Some(scope);
                } else {
                    self.current_file_scope = None;
                }
            } else {
                self.current_file_scope = None;
            }
        } else {
            self.current_file_scope = None;
        }

        let result = match decl {
            Decl::Component(c) => self.compile_component_decl(c),
            Decl::Resource(r) => self.compile_resource_decl(r),
            Decl::Struct(s) => self.compile_struct_decl(s),
            Decl::Intent(i) => self.compile_intent_decl(i),
            Decl::Law(l) => self.compile_law_decl(l),
            Decl::Resolver(r) => self.compile_resolver_decl(r),
            Decl::Constraint(c) => self.compile_constraint_decl(c),
            Decl::Entity(e) => self.compile_entity_decl(e),
            Decl::State(s) => self.compile_state_decl(s),
            Decl::System(s) => self.compile_system_decl(s),
            Decl::Event(_e) => Ok(()),
            Decl::Phase(p) => {
                let (name, resolved) = self.canonical_phase(p);
                self.phases.insert(name.clone(), resolved.clone());
                if p.serial && !self.serial_phases.iter().any(|(n, _)| n == &name) {
                    // Resolve member names now, while the module scope is
                    // live — group stamping runs after scopes are gone.
                    self.serial_phases.push((name, resolved));
                }
                Ok(())
            }
            Decl::OnHandler(handler) => self.compile_on_handler(handler),
            Decl::Migration(migration) => self.compile_migration_decl(migration),
            Decl::Fn(function) => self.compile_fn_decl(function),
            Decl::NativeType(native) => self.compile_native_type_decl(native),
            Decl::MaterializedView(view) => self.compile_materialized_view_decl(view),
            Decl::Model(model) => self.compile_model_decl(model),
            Decl::Type(_) | Decl::Use(_) | Decl::TypeAlias(_) | Decl::Error => Ok(()),
            Decl::Test(test) => self.compile_test_decl(test),
            Decl::Stmt(statement) => self.compile_stmt(statement),
        };

        self.current_file_scope = prev_file_scope;
        result
    }

    /// Register compile-time declaration metadata before hoisted bodies are
    /// compiled. This mirrors the real pass without emitting bytecode.
    pub(crate) fn predeclare_decl_metadata(&mut self, decl: &Decl) {
        let prev_file_scope = self.current_file_scope.clone();
        self.current_file_scope = decl
            .span()
            .and_then(|span| span.file)
            .and_then(|file_id| self.file_private_scopes.get(&file_id.0).cloned());

        match decl {
            Decl::Component(component) => {
                let resolved = self
                    .resolve_current_alias(&component.name)
                    .unwrap_or_else(|| component.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::Resource(resource) => {
                let resolved = self
                    .resolve_current_alias(&resource.name)
                    .unwrap_or_else(|| resource.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::Struct(structure) => {
                let resolved = self
                    .resolve_current_alias(&structure.name)
                    .unwrap_or_else(|| structure.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::NativeType(native) => {
                let resolved = self.resolve_canonical_name(&native.name);
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::MaterializedView(view) => {
                let resolved = self.resolve_canonical_name(&view.name);
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::Intent(intent) => {
                let resolved = self
                    .resolve_current_alias(&intent.name)
                    .unwrap_or_else(|| intent.name.clone());
                let key = intent
                    .fields
                    .iter()
                    .find(|field| field.is_key)
                    .map(|field| field.name.clone())
                    .unwrap_or_default();
                self.intent_types.entry(resolved).or_insert_with(|| {
                    (
                        key,
                        intent
                            .fields
                            .iter()
                            .map(|field| field.name.clone())
                            .collect(),
                    )
                });
            }
            Decl::Law(law) => {
                let resolved = self
                    .resolve_current_alias(&law.name)
                    .unwrap_or_else(|| law.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::Entity(entity) => {
                let resolved = self
                    .resolve_current_alias(&entity.name)
                    .unwrap_or_else(|| entity.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::Fn(function) => {
                self.global_mutability
                    .entry(function.name.clone())
                    .or_insert(false);
                // The view-kernel analyzer resolves a `visit_view` callback
                // through this table while the *system* body compiles. Without
                // it `analyze_kernel_function` finds nothing, silently declines
                // to fuse, and the traversal falls back to ordinary calls —
                // 2.4x the frame cost and ~1 runtime allocation per row, with
                // no diagnostic and an unchanged `query-plan`. Both spellings
                // are registered because a call site may use either.
                let resolved = self
                    .resolve_current_alias(&function.name)
                    .unwrap_or_else(|| function.name.clone());
                self.function_declarations
                    .entry(function.name.clone())
                    .or_insert_with(|| function.clone());
                self.function_declarations
                    .entry(resolved)
                    .or_insert_with(|| function.clone());
            }
            Decl::System(system) => {
                let resolved = self
                    .resolve_current_alias(&system.name)
                    .unwrap_or_else(|| system.name.clone());
                self.declared_systems.insert(resolved);
            }
            Decl::Phase(phase) => {
                let (name, systems) = self.canonical_phase(phase);
                self.phases.entry(name).or_insert(systems);
            }
            Decl::Stmt(statement) => match statement {
                Stmt::Let(binding) => {
                    for name in &binding.names {
                        self.global_mutability
                            .entry(name.clone())
                            .or_insert(binding.mutable);
                    }
                }
                Stmt::LetElse(binding) => {
                    if let Some(primary) = binding.primary_binding_name() {
                        self.global_mutability
                            .entry(primary)
                            .or_insert(binding.mutable);
                    }
                }
                _ => {}
            },
            _ => {}
        }

        self.current_file_scope = prev_file_scope;
    }

    fn compile_native_type_decl(&mut self, native: &NativeTypeDecl) -> Result<(), CompileError> {
        let resolved = self.resolve_canonical_name(&native.name);
        let mut descriptor = crate::native_types::NativeTypeDescriptor::from(native);
        descriptor.name = resolved.as_str().into();
        self.native_types
            .insert(resolved.clone(), descriptor.clone());
        let line = native.span.line;
        self.emit_constant_gc(line, |gc| Value::from_native_type(gc, descriptor));
        let slot = self.ensure_global_slot(&resolved);
        self.emit_op(Op::DefGlobal, line);
        self.emit_u16(slot, line);
        self.global_mutability.insert(resolved, false);
        Ok(())
    }

    fn compile_materialized_view_decl(
        &mut self,
        view: &MaterializedViewDecl,
    ) -> Result<(), CompileError> {
        let resolved = self.resolve_canonical_name(&view.name);
        let mut predicate = view.predicate.clone();
        for clause in &mut predicate.clauses {
            clause.component = self.resolve_canonical_name(&clause.component);
        }
        self.materialized_views.push(MaterializedViewInfo {
            name: resolved.clone(),
            dependencies: view
                .dependencies
                .iter()
                .map(|dependency| self.resolve_canonical_name(dependency))
                .collect(),
            key: view
                .key
                .as_ref()
                .map(|(component, field)| (self.resolve_canonical_name(component), field.clone())),
            predicate,
        });
        let slot = self.ensure_global_slot(&resolved);
        self.emit_constant_gc(view.span.line, |gc| {
            Value::from_string(gc, resolved.clone())
        });
        self.emit_op(Op::DefGlobal, view.span.line);
        self.emit_u16(slot, view.span.line);
        self.global_mutability.insert(resolved, false);
        Ok(())
    }

    fn compile_component_decl(&mut self, c: &ComponentDecl) -> Result<(), CompileError> {
        let resolved = self
            .resolve_current_alias(&c.name)
            .unwrap_or_else(|| c.name.clone());
        if c.version > 0 {
            self.component_versions.insert(resolved.clone(), c.version);
        }
        self.global_mutability.insert(resolved.clone(), false);
        let slot = self.ensure_global_slot(&resolved);
        self.emit_constant_gc(c.span.line, |gc| Value::from_string(gc, resolved.clone()));
        self.emit_op(Op::DefGlobal, c.span.line);
        self.emit_u16(slot, c.span.line);
        Ok(())
    }

    fn compile_resource_decl(&mut self, r: &ResourceDecl) -> Result<(), CompileError> {
        let resolved = self
            .resolve_current_alias(&r.name)
            .unwrap_or_else(|| r.name.clone());
        if r.version > 0 {
            self.component_versions.insert(resolved.clone(), r.version);
        }
        let defaults = self
            .resource_types
            .get(&resolved)
            .cloned()
            .unwrap_or_else(|| super::Compiler::component_fields_as_defaults(&r.fields));
        let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, resolved.clone()));
        let field_count = defaults.len();
        for (_, _, expr) in &defaults {
            self.compile_expr(expr)?;
        }
        self.emit_op(Op::InitResource, r.span.line);
        self.emit_u16(type_idx, r.span.line);
        self.emit_u16(field_count as u16, r.span.line);

        self.global_mutability.insert(resolved.clone(), false);
        let slot = self.ensure_global_slot(&resolved);
        self.emit_constant_gc(r.span.line, |gc| Value::from_string(gc, resolved.clone()));
        self.emit_op(Op::DefGlobal, r.span.line);
        self.emit_u16(slot, r.span.line);
        Ok(())
    }

    fn compile_struct_decl(&mut self, s: &StructDecl) -> Result<(), CompileError> {
        let resolved = self
            .resolve_current_alias(&s.name)
            .unwrap_or_else(|| s.name.clone());
        self.global_mutability.insert(resolved.clone(), false);
        let slot = self.ensure_global_slot(&resolved);
        self.emit_constant_gc(s.span.line, |gc| Value::from_string(gc, resolved.clone()));
        self.emit_op(Op::DefGlobal, s.span.line);
        self.emit_u16(slot, s.span.line);
        Ok(())
    }

    pub(crate) fn compile_component_inits(
        &mut self,
        components: &[ComponentEntry],
        line: u32,
    ) -> Result<(), CompileError> {
        for entry in components {
            match entry {
                ComponentEntry::Expr(expr) => {
                    self.compile_expr(expr)?;
                }
                ComponentEntry::Init(ci) => {
                    self.compile_component_init(ci, line)?;
                }
            }
        }
        Ok(())
    }

    fn compile_component_init(
        &mut self,
        ci: &ComponentInit,
        line: u32,
    ) -> Result<(), CompileError> {
        if ci.comp_name.contains("::") {
            let parts: Vec<&str> = ci.comp_name.split("::").collect();
            let machine_name = parts[0];
            let state_name = parts[1];
            let resolved_machine = self.resolve_canonical_name(machine_name);

            let state_val =
                Value::from_state(&mut self.gc, resolved_machine, state_name.to_string());
            self.emit_constant(state_val, line);
            return Ok(());
        }

        let resolved_comp = self.resolve_canonical_name(&ci.comp_name);
        let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, resolved_comp.clone()));
        let defaults = self
            .component_types
            .get(&resolved_comp)
            .cloned()
            .unwrap_or_default();
        let mut all_fields: Vec<(String, Option<&Expr>)> = Vec::new();
        for (fname, _, fexpr) in &defaults {
            all_fields.push((fname.clone(), Some(fexpr)));
        }
        for (fname, fexpr) in &ci.fields {
            if let Some(existing) = all_fields.iter_mut().find(|(n, _)| n == fname) {
                existing.1 = Some(fexpr);
            } else {
                all_fields.push((fname.clone(), Some(fexpr)));
            }
        }

        if let Some(slot_order) = self.component_field_order(&resolved_comp) {
            let field_count = slot_order.len();
            for slot_name in &slot_order {
                if let Some((_, Some(expr))) = all_fields.iter().find(|(n, _)| n == slot_name) {
                    self.compile_expr(expr)?;
                } else {
                    self.emit_constant(Value::NIL, line);
                }
            }
            self.emit_op(Op::MakeCompSlot, line);
            self.emit_u16(type_idx, line);
            self.emit_u16(field_count as u16, line);
        } else {
            let field_count = all_fields.len();
            for (fname, fexpr) in &all_fields {
                self.emit_constant_gc(line, |gc| Value::from_string(gc, fname.clone()));
                if let Some(expr) = fexpr {
                    self.compile_expr(expr)?;
                } else {
                    self.emit_constant(Value::NIL, line);
                }
            }
            self.emit_op(Op::MakeComp, line);
            self.emit_u16(type_idx, line);
            self.emit_u16(field_count as u16, line);
        }
        Ok(())
    }

    fn compile_entity_decl(&mut self, e: &EntityDecl) -> Result<(), CompileError> {
        let line = e.span.line;
        self.compile_component_inits(&e.components, line)?;

        let resolved_name = self
            .resolve_current_alias(&e.name)
            .unwrap_or_else(|| e.name.clone());

        let comp_count = e.components.len() as u8;
        let name_idx = self.add_constant_gc(|gc| Value::from_string(gc, resolved_name.clone()));
        self.emit_op(Op::EcsSpawn, line);
        self.emit_byte(comp_count, line);
        self.emit_byte(0, line);
        self.emit_u16(name_idx, line);

        self.global_mutability.insert(resolved_name.clone(), false);
        let slot = self.ensure_global_slot(&resolved_name);
        self.emit_op(Op::DefGlobal, line);
        self.emit_u16(slot, line);
        Ok(())
    }

    fn compile_state_decl(&mut self, s: &StateDecl) -> Result<(), CompileError> {
        let mut states = HashMap::new();
        for state_def in &s.states {
            let mut transitions = Vec::new();
            for (ev, target, guard) in &state_def.transitions {
                let guard_chunk_id = if let Some(guard_expr) = guard {
                    Some(self.compile_state_guard(guard_expr, s.span.line)?)
                } else {
                    None
                };
                let resolved_event = self.resolve_canonical_name(ev);
                transitions.push(StateTransitionInfo {
                    event: resolved_event,
                    target: target.clone(),
                    guard_chunk_id,
                });
            }
            states.insert(state_def.name.clone(), transitions);
        }
        let resolved_name = self
            .resolve_current_alias(&s.name)
            .unwrap_or_else(|| s.name.clone());
        self.state_machines.push(StateMachineInfo {
            name: resolved_name,
            states,
        });
        Ok(())
    }

    fn compile_state_guard(&mut self, guard_expr: &Expr, line: u32) -> Result<usize, CompileError> {
        let fn_scope = Self::new_fn_scope("state_guard");
        self.functions.push(fn_scope);
        self.compile_expr(guard_expr)?;
        self.emit_op(Op::Return, line);
        let scope = self.functions.pop().unwrap();
        let chunk_id = self.chunks.len() + 1;
        self.chunks.push(scope.chunk);
        Ok(chunk_id)
    }

    pub(crate) fn compile_fn_decl(&mut self, f: &FnDecl) -> Result<(), CompileError> {
        let line = f.span.line;
        let mut fn_scope = Self::new_fn_scope(&f.name);
        fn_scope.unique_locals = super::escape::find_unique_locals(&f.body);
        self.functions.push(fn_scope);

        for (i, param) in f.params.iter().enumerate() {
            let is_mut = f.param_muts.get(i).copied().unwrap_or(false);
            self.add_local(param.clone(), is_mut);
        }

        let optimized_body = if self.should_optimize_expressions(&f.name) {
            super::expression_optimizer::optimize_ecs_function_block(&f.body)
        } else {
            f.body.clone()
        };

        let causal_callable = self.may_run_in_causal_region(&f.name);
        if causal_callable {
            self.causal_lowering_depth += 1;
        }
        let body_result = self.compile_body(&optimized_body.stmts);
        if causal_callable {
            self.causal_lowering_depth -= 1;
        }
        body_result?;

        self.emit_constant(Value::NIL, line);
        self.emit_op(Op::Return, line);

        let scope = self.functions.pop().unwrap();
        let fn_chunk = scope.chunk;
        let upvalues = scope.upvalues;

        let chunk_id = self.chunks.len() + 1;
        self.chunks.push(fn_chunk);

        self.global_mutability.insert(f.name.clone(), false);

        let slot = self.ensure_global_slot(&f.name);
        if upvalues.is_empty() {
            let fn_val = Value::from_fn(
                &mut self.gc,
                FnValue {
                    name: f.name.clone(),
                    arity: f.params.len() as u8,
                    chunk_id,
                },
            );
            self.emit_constant(fn_val, line);
            self.emit_op(Op::DefGlobal, line);
            self.emit_u16(slot, line);
        } else {
            self.emit_op(Op::Closure, line);
            self.emit_u16(chunk_id as u16, line);
            self.emit_byte(f.params.len() as u8, line);
            self.emit_byte(upvalues.len() as u8, line);
            for uv in &upvalues {
                self.emit_byte(if uv.is_local { 1 } else { 0 }, line);
                self.emit_u16(uv.index, line);
            }
            self.emit_op(Op::DefGlobal, line);
            self.emit_u16(slot, line);
        }
        Ok(())
    }

    fn compile_system_decl(&mut self, s: &SystemDecl) -> Result<(), CompileError> {
        let line = s.span.line;
        let mut fn_scope = Self::new_fn_scope(&format!("system_{}", s.name));
        fn_scope.unique_locals = super::escape::find_unique_locals(&s.body);
        self.functions.push(fn_scope);

        for (pname, is_mut, _) in &s.params {
            self.add_local(pname.clone(), *is_mut);
        }
        self.add_local("self".to_string(), false);

        let optimized_body = super::expression_optimizer::optimize_system_block(&s.body);

        self.compile_body(&optimized_body.stmts)?;

        self.emit_constant(Value::NIL, line);
        self.emit_op(Op::Return, line);

        let scope = self.functions.pop().unwrap();
        let chunk_id = self.chunks.len() + 1;
        self.chunks.push(scope.chunk);

        let mut params: Vec<SystemParam> = s
            .params
            .iter()
            .map(|(name, is_mut, comp_type)| {
                let resolved_comp = self.resolve_canonical_name(comp_type);
                SystemParam {
                    name: name.clone(),
                    is_mut: *is_mut,
                    is_accum: s.accum_params.contains(name),
                    comp_type: resolved_comp,
                    is_resource: self
                        .resource_types
                        .contains_key(&self.resolve_canonical_name(comp_type)),
                }
            })
            .collect();

        let resolved_name = self
            .resolve_current_alias(&s.name)
            .unwrap_or_else(|| s.name.clone());
        // Scheduler metadata comes from the same transitive graph that
        // enforces system authority. Deferred event/schedule edges execute
        // outside this batch, so batching uses the synchronous effect set.
        let effects = {
            let authority = self
                .authority
                .as_ref()
                .expect("compile installs authority before lowering");
            authority
                .resolve(&resolved_name)
                .or_else(|_| authority.resolve(&s.name))
                .map(|callable| callable.synchronous.clone())
                .map_err(|message| CompileError {
                    message: format!(
                        "Missing authority graph entry for system '{}': {}",
                        s.name, message
                    ),
                    line: s.span.line,
                    col: s.span.col,
                })?
        };
        let mut body_writes = effects
            .writes
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let mut body_reads = effects
            .reads
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        if effects.unknown {
            body_writes.insert("*".to_string());
            body_reads.insert("*".to_string());
        }
        {
            let declared_mut: std::collections::HashSet<&str> = params
                .iter()
                .filter(|p| p.is_mut)
                .map(|p| p.comp_type.as_str())
                .collect();
            let declared_any: std::collections::HashSet<&str> =
                params.iter().map(|p| p.comp_type.as_str()).collect();
            body_writes.retain(|w| !declared_mut.contains(w.as_str()));
            body_reads.retain(|r| !declared_any.contains(r.as_str()) && !body_writes.contains(r));
        }
        for w in body_writes {
            params.push(SystemParam {
                name: "__body_write".to_string(),
                is_mut: true,
                is_accum: false,
                comp_type: w,
                is_resource: true,
            });
        }
        for r in body_reads {
            params.push(SystemParam {
                name: "__body_read".to_string(),
                is_mut: false,
                is_accum: false,
                comp_type: r,
                is_resource: true,
            });
        }
        let mut resolved_after = Vec::new();
        for dep in &s.after {
            resolved_after.push(self.resolve_canonical_name(dep));
        }
        let mut resolved_before = Vec::new();
        for dep in &s.before {
            resolved_before.push(self.resolve_canonical_name(dep));
        }
        self.systems.push(SystemChunkInfo {
            name: resolved_name,
            params,
            chunk_id,
            after: resolved_after,
            before: resolved_before,
            serial_group: None,
            instruction_budget: s.contracts.instruction_budget,
        });
        Ok(())
    }

    /// `migrate X(old) { return X { … } }` — compiled like a one-parameter
    /// function; `load_world` invokes the chunk with the persisted fields as
    /// a map and takes the returned component.
    fn compile_migration_decl(&mut self, m: &MigrationDecl) -> Result<(), CompileError> {
        let line = m.span.line;
        let mut fn_scope = Self::new_fn_scope(&format!("migrate_{}", m.component));
        fn_scope.unique_locals = super::escape::find_unique_locals(&m.body);
        self.functions.push(fn_scope);

        self.add_local(m.param_name.clone(), false);
        let param_slot = self.resolve_local(&m.param_name).unwrap_or(0);
        // Optional `from_version` (dogfood seq 69): a second local right
        // after `old`, filled by the loader with the save's declared
        // schema version for this type.
        let version_slot = m.version_param.as_ref().map(|vp| {
            self.add_local(vp.clone(), false);
            self.resolve_local(vp).unwrap_or(param_slot + 1)
        });

        self.compile_body(&m.body.stmts)?;

        // Fallthrough (no explicit `return`) yields NIL, which load_world
        // rejects with a clear error.
        self.emit_constant(Value::NIL, line);
        self.emit_op(Op::Return, line);

        let scope = self.functions.pop().unwrap();
        let chunk_id = self.chunks.len() + 1;
        self.chunks.push(scope.chunk);

        let resolved = self.resolve_canonical_name(&m.component);
        self.migrations.push(MigrationChunkInfo {
            component: resolved,
            param_slot,
            version_slot,
            chunk_id,
        });
        Ok(())
    }

    fn compile_on_handler(&mut self, h: &OnHandler) -> Result<(), CompileError> {
        let line = h.span.line;
        let mut fn_scope = Self::new_fn_scope(&format!("on_{}", h.event_name));
        fn_scope.unique_locals = super::escape::find_unique_locals(&h.body);
        self.functions.push(fn_scope);

        self.add_local(h.param_name.clone(), false);
        let param_slot = self.resolve_local(&h.param_name).unwrap_or(0);

        self.compile_body(&h.body.stmts)?;

        self.emit_constant(Value::NIL, line);
        self.emit_op(Op::Return, line);

        let scope = self.functions.pop().unwrap();
        let chunk_id = self.chunks.len() + 1;
        self.chunks.push(scope.chunk);

        let resolved_event = self.resolve_canonical_name(&h.event_name);

        self.handlers.push(HandlerChunkInfo {
            event_name: resolved_event,
            param_name: h.param_name.clone(),
            param_slot,
            chunk_id,
            once: h.once,
            is_async: h.is_async,
            has_guard: h.has_guard,
            contracts: h.contracts.clone(),
        });
        Ok(())
    }

    fn compile_test_decl(&mut self, t: &TestDecl) -> Result<(), CompileError> {
        let line = t.span.line;
        if t.shared_world {
            self.shared_world_tests.insert(t.name.clone());
        }
        let test_name = format!("__test_{}", t.name);
        let mut test_scope = Compiler::new_fn_scope(&test_name);
        test_scope.unique_locals = super::escape::find_unique_locals(&t.body);
        test_scope.scope_depth = 1;
        self.functions.push(test_scope);

        for (name, gen_expr) in &t.generators {
            self.compile_expr(gen_expr)?;
            self.add_local(name.clone(), false);
        }

        self.compile_body(&t.body.stmts)?;
        self.emit_constant(Value::NIL, line);
        self.emit_op(Op::Return, line);

        let scope = self.functions.pop().unwrap();
        let chunk_id = self.chunks.len() + 1;
        self.chunks.push(scope.chunk);

        self.global_mutability.insert(test_name.clone(), false);
        let slot = self.ensure_global_slot(&test_name);
        let fn_val = Value::from_fn(
            &mut self.gc,
            FnValue {
                name: test_name,
                arity: 0,
                chunk_id,
            },
        );
        self.emit_constant(fn_val, line);
        self.emit_op(Op::DefGlobal, line);
        self.emit_u16(slot, line);
        Ok(())
    }

    fn compile_model_decl(&mut self, model: &ModelDecl) -> Result<(), CompileError> {
        let line = model.span.line;
        let test_name = format!("__test_model_{}", model.name);
        let mut test_scope = Compiler::new_fn_scope(&test_name);
        test_scope.scope_depth = 1;
        self.functions.push(test_scope);

        let command_labels = model
            .commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                let label = match command {
                    Expr::Ident(name, _) => name.clone(),
                    _ => format!("command_{}", index + 1),
                };
                Expr::StrLit(label, model.span.clone())
            })
            .collect::<Vec<_>>();
        let invariant_closures = model
            .invariants
            .iter()
            .cloned()
            .map(|body| {
                Expr::FnExpr(
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Some(TypeExpr::Named("bool".to_string())),
                    body,
                    model.span.clone(),
                )
            })
            .collect::<Vec<_>>();
        let temporal = model
            .temporal
            .iter()
            .map(|clause| {
                let canonical = |name: &str| self.resolve_canonical_name(name);
                let encoded = match clause {
                    TemporalClause::Always(name) => format!("always|{}", canonical(name)),
                    TemporalClause::Eventually(name) => {
                        format!("eventually|{}", canonical(name))
                    }
                    TemporalClause::Until {
                        condition,
                        terminal,
                    } => format!("until|{}|{}", canonical(condition), canonical(terminal)),
                    TemporalClause::LeadsTo {
                        trigger,
                        consequence,
                    } => format!("leads_to|{}|{}", canonical(trigger), canonical(consequence)),
                    TemporalClause::ExactlyOnce(name) => {
                        format!("exactly_once|{}", canonical(name))
                    }
                    TemporalClause::NeverAfter {
                        prohibited,
                        terminal,
                    } => format!(
                        "never_after|{}|{}",
                        canonical(prohibited),
                        canonical(terminal)
                    ),
                    TemporalClause::EventuallyWithin {
                        trigger,
                        consequence,
                        bound,
                    } => format!(
                        "eventually_within|{}|{}|{bound}",
                        canonical(trigger),
                        canonical(consequence)
                    ),
                };
                Expr::StrLit(encoded, model.span.clone())
            })
            .collect::<Vec<_>>();
        let call = Expr::Call(
            Box::new(Expr::Ident("model_check".to_string(), model.span.clone())),
            vec![
                Expr::StrLit(model.name.clone(), model.span.clone()),
                Expr::ListLit(model.commands.clone(), model.span.clone()),
                Expr::ListLit(command_labels, model.span.clone()),
                Expr::ListLit(invariant_closures, model.span.clone()),
                Expr::ListLit(temporal, model.span.clone()),
                Expr::IntLit(i64::from(model.runs), model.span.clone()),
                Expr::IntLit(i64::from(model.max_commands), model.span.clone()),
                Expr::IntLit(model.seed as i64, model.span.clone()),
            ],
            model.span.clone(),
        );
        self.compile_expr(&call)?;
        self.emit_op(Op::PopCheckErr, line);
        self.emit_constant(Value::NIL, line);
        self.emit_op(Op::Return, line);

        let scope = self.functions.pop().expect("model function scope");
        let chunk_id = self.chunks.len() + 1;
        self.chunks.push(scope.chunk);
        self.global_mutability.insert(test_name.clone(), false);
        let slot = self.ensure_global_slot(&test_name);
        let fn_val = Value::from_fn(
            &mut self.gc,
            FnValue {
                name: test_name,
                arity: 0,
                chunk_id,
            },
        );
        self.emit_constant(fn_val, line);
        self.emit_op(Op::DefGlobal, line);
        self.emit_u16(slot, line);
        Ok(())
    }
}
