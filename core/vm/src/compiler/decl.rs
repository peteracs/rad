use super::*;

impl Compiler {
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

        let res = match decl {
            Decl::Component(c) => self.compile_component_decl(c),
            Decl::Resource(r) => self.compile_resource_decl(r),
            Decl::Struct(s) => self.compile_struct_decl(s),
            Decl::Intent(i) => self.compile_intent_decl(i),
            Decl::Law(l) => self.compile_law_decl(l),
            Decl::Resolver(r) => self.compile_resolver_decl(r),
            Decl::Entity(e) => self.compile_entity_decl(e),
            Decl::State(s) => self.compile_state_decl(s),
            Decl::System(s) => self.compile_system_decl(s),
            Decl::Event(_e) => Ok(()),
            Decl::Phase(p) => {
                self.phases.insert(p.name.clone(), p.systems.clone());
                if p.serial && !self.serial_phases.iter().any(|(n, _)| n == &p.name) {
                    // Resolve member names now, while the module scope is
                    // live — group stamping runs after scopes are gone.
                    let resolved = p
                        .systems
                        .iter()
                        .map(|s| self.resolve_canonical_name(s))
                        .collect();
                    self.serial_phases.push((p.name.clone(), resolved));
                }
                Ok(())
            }
            Decl::OnHandler(h) => self.compile_on_handler(h),
            Decl::Migration(m) => self.compile_migration_decl(m),
            Decl::Fn(f) => self.compile_fn_decl(f),
            Decl::Type(_t) => Ok(()),
            Decl::Use(_) => Ok(()),
            Decl::Test(t) => self.compile_test_decl(t),
            Decl::Stmt(s) => self.compile_stmt(s),
            Decl::TypeAlias(_) => Ok(()),
            Decl::Error => Ok(()),
        };

        self.current_file_scope = prev_file_scope;
        res
    }

    /// Declaration-metadata pre-pass: registers the compile-time facts each
    /// declaration will eventually establish, before any body compiles.
    ///
    /// Top-level `fn` definitions are hoisted ahead of every other
    /// declaration (see `compile`), which moves their body compilation ahead
    /// of the declarations that follow them in source. Without this pre-pass
    /// a hoisted body could not know that a later name is a system (the call
    /// would compile as a plain global call and trap on `nil`), that a later
    /// binding is immutable (an illegal assignment would compile silently),
    /// or that a later name is a phase (a `schedule` would skip expansion).
    ///
    /// Names are registered exactly as the real pass registers them — same
    /// file-scope resolution, raw vs resolved spelling per declaration kind —
    /// and via `entry().or_insert()`, so when a name is declared twice the
    /// binding in force between the two declarations is still the earlier
    /// one, exactly as in the in-order pass.
    pub(crate) fn predeclare_decl_metadata(&mut self, decl: &Decl) {
        let prev_file_scope = self.current_file_scope.clone();
        self.current_file_scope = decl
            .span()
            .and_then(|span| span.file)
            .and_then(|file_id| self.file_private_scopes.get(&file_id.0).cloned());

        match decl {
            Decl::Component(c) => {
                let resolved = self
                    .resolve_current_alias(&c.name)
                    .unwrap_or_else(|| c.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::Resource(r) => {
                let resolved = self
                    .resolve_current_alias(&r.name)
                    .unwrap_or_else(|| r.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::Struct(s) => {
                let resolved = self
                    .resolve_current_alias(&s.name)
                    .unwrap_or_else(|| s.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::Intent(i) => {
                let resolved = self
                    .resolve_current_alias(&i.name)
                    .unwrap_or_else(|| i.name.clone());
                let key = i
                    .fields
                    .iter()
                    .find(|field| field.is_key)
                    .map(|field| field.name.clone())
                    .unwrap_or_default();
                self.intent_types.entry(resolved).or_insert_with(|| {
                    (
                        key,
                        i.fields.iter().map(|field| field.name.clone()).collect(),
                    )
                });
            }
            Decl::Law(l) => {
                let resolved = self
                    .resolve_current_alias(&l.name)
                    .unwrap_or_else(|| l.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            Decl::Entity(e) => {
                let resolved = self
                    .resolve_current_alias(&e.name)
                    .unwrap_or_else(|| e.name.clone());
                self.global_mutability.entry(resolved).or_insert(false);
            }
            // compile_fn_decl registers mutability under the raw name.
            Decl::Fn(f) => {
                self.global_mutability
                    .entry(f.name.clone())
                    .or_insert(false);
            }
            Decl::System(s) => {
                let resolved = self
                    .resolve_current_alias(&s.name)
                    .unwrap_or_else(|| s.name.clone());
                self.declared_systems.insert(resolved);
            }
            Decl::Phase(p) => {
                self.phases
                    .entry(p.name.clone())
                    .or_insert_with(|| p.systems.clone());
            }
            Decl::Stmt(stmt) => match stmt {
                // Top-level lets are globals (single, destructuring, rec).
                Stmt::Let(l) => {
                    for name in &l.names {
                        self.global_mutability
                            .entry(name.clone())
                            .or_insert(l.mutable);
                    }
                }
                Stmt::LetElse(le) => {
                    if let Some(primary) = le.primary_binding_name() {
                        self.global_mutability.entry(primary).or_insert(le.mutable);
                    }
                }
                _ => {}
            },
            _ => {}
        }

        self.current_file_scope = prev_file_scope;
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

        let optimized_body = if self.should_optimize_egraph(&f.name) {
            super::egraph::optimize_ecs_function_block(&f.body)
        } else {
            f.body.clone()
        };

        self.compile_body(&optimized_body.stmts)?;

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

        let optimized_body = super::egraph::optimize_system_block(&s.body);

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

        // Parallel conflict analysis (vm/parallel.rs) schedules from the
        // declared signature alone, so a resource written via `update(R)` or
        // `set_resource(R, ...)` in the body — or in a helper fn the body
        // calls — was invisible: two conflicting systems could share a
        // parallel batch and one side's write was silently lost (dogfood
        // bug seq 45). Collect the body's ECS accesses and append them as
        // synthetic metadata-only entries. The "__body_" name prefix makes
        // the executor skip them for injection/writeback/sandbox gating;
        // only the scheduler consumes them. "*" marks a write whose target
        // cannot be named statically (dynamic name, or a call to a fn whose
        // effects allow ECS writes) and conflicts with any ECS toucher.
        let access = collect_body_ecs_access(&s.body);
        let mut body_writes: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut body_reads: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        if access.dynamic_write {
            body_writes.insert("*".to_string());
        }
        if access.dynamic_read {
            body_reads.insert("*".to_string());
        }
        for n in &access.write_names {
            body_writes.insert(self.resolve_canonical_name(n));
        }
        for n in &access.read_names {
            body_reads.insert(self.resolve_canonical_name(n));
        }
        if let Some(co) = &self.checker_output {
            for f in &access.called_fns {
                let canon = self.resolve_canonical_name(f);
                let sig = co.functions.get(&canon).or_else(|| co.functions.get(f));
                if let Some(sig) = sig {
                    if sig.effects.allows(crate::types::Effect::ECS) {
                        body_writes.insert("*".to_string());
                        break;
                    }
                }
            }
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
        let resolved_name = self
            .resolve_current_alias(&s.name)
            .unwrap_or_else(|| s.name.clone());
        self.systems.push(SystemChunkInfo {
            name: resolved_name,
            params,
            chunk_id,
            after: resolved_after,
            before: resolved_before,
            serial_group: None,
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
        });
        Ok(())
    }

    fn compile_test_decl(&mut self, t: &TestDecl) -> Result<(), CompileError> {
        let line = t.span.line;
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
}

/// Syntactic ECS accesses of a system body (dogfood bug seq 45): resource
/// `update`s, `set`/`set_resource`/`remove` writes, `res`/`get_resource`
/// reads, plus the names of every called function so the caller can consult
/// checker effects for helpers that may write ECS state transitively. Names
/// are raw — canonical resolution happens in `compile_system_decl`.
#[derive(Default)]
pub(crate) struct BodyEcsAccess {
    pub(crate) write_names: std::collections::BTreeSet<String>,
    pub(crate) read_names: std::collections::BTreeSet<String>,
    pub(crate) called_fns: std::collections::BTreeSet<String>,
    pub(crate) dynamic_write: bool,
    pub(crate) dynamic_read: bool,
}

pub(crate) fn collect_body_ecs_access(body: &crate::ast::Block) -> BodyEcsAccess {
    use crate::ast::{Expr, Span, Stmt};
    use crate::visitor::{walk_call_expr, walk_expr, walk_stmt, AstVisitor};

    struct V {
        acc: BodyEcsAccess,
    }
    impl AstVisitor for V {
        fn visit_stmt(&mut self, stmt: &Stmt) {
            if let Stmt::Update(u) = stmt {
                // update(R) writes resource R; update(e, C) writes component C.
                self.acc.write_names.insert(u.comp_name.clone());
            }
            walk_stmt(self, stmt);
        }

        fn visit_expr(&mut self, expr: &Expr) {
            // `x |> helper` calls without parentheses: the callee is a bare
            // ident on the pipe's right-hand side, not an Expr::Call.
            if let Expr::Pipe(_, rhs, _) = expr {
                if let Expr::Ident(name, _) = rhs.as_ref() {
                    self.acc.called_fns.insert(name.clone());
                }
            }
            walk_expr(self, expr);
        }

        fn visit_call_expr(&mut self, callee: &Expr, args: &[Expr], _span: &Span) {
            match callee {
                Expr::Ident(name, _) => match name.as_str() {
                    "set_resource" => match args.first() {
                        Some(Expr::Ident(rname, _)) => {
                            self.acc.write_names.insert(rname.clone());
                        }
                        _ => self.acc.dynamic_write = true,
                    },
                    "set" => match args.get(1) {
                        Some(Expr::ComponentExpr(cname, _, _, _)) => {
                            self.acc.write_names.insert(cname.clone());
                        }
                        _ => self.acc.dynamic_write = true,
                    },
                    "remove" => match args.get(1) {
                        Some(Expr::Ident(cname, _)) | Some(Expr::StrLit(cname, _)) => {
                            self.acc.write_names.insert(cname.clone());
                        }
                        _ => self.acc.dynamic_write = true,
                    },
                    "res" | "get_resource" => match args.first() {
                        Some(Expr::Ident(rname, _)) => {
                            self.acc.read_names.insert(rname.clone());
                        }
                        _ => self.acc.dynamic_read = true,
                    },
                    _ => {
                        self.acc.called_fns.insert(name.clone());
                    }
                },
                Expr::Field(base, field, _) => {
                    // Module-qualified helper calls (`util.give(...)`).
                    if let Expr::Ident(module, _) = base.as_ref() {
                        self.acc.called_fns.insert(format!("{}.{}", module, field));
                    }
                }
                _ => {}
            }
            walk_call_expr(self, callee, args);
        }
    }

    let mut v = V {
        acc: BodyEcsAccess::default(),
    };
    crate::visitor::walk_block(&mut v, body);
    v.acc
}
