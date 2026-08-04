impl Compiler {

    fn compile_for_query_unpack(
        &mut self,
        f: &ForStmt,
        q: &crate::ast::QueryExprNode,
    ) -> Result<(), CompileError> {
        let line = f.span.line;
        self.begin_scope();

        let list_name = self.fresh_name("query_list");
        let idx_name = self.fresh_name("query_idx");

        let (without_types, remaining_filter) = if let Some(filter_expr) = &q.filter {
            Compiler::extract_query_negations(filter_expr)
        } else {
            (Vec::new(), None)
        };

        for (comp, _) in &q.components {
            let resolved = self.resolve_canonical_name(comp);
            self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
        }
        for comp in &without_types {
            let resolved = self.resolve_canonical_name(comp);
            self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
        }
        self.emit_op(Op::EcsQuery, line);
        self.emit_byte(q.components.len() as u8, line);
        self.emit_byte(without_types.len() as u8, line);

        if let Some(filter_expr) = &remaining_filter {
            let filter_scope = Compiler::new_fn_scope("__query_filter_mut");
            self.functions.push(filter_scope);
            self.add_local("__entity".to_string(), false);
            for (comp, _) in &q.components {
                self.add_local(comp.clone(), false);
            }
            self.compile_expr(filter_expr)?;
            self.emit_op(Op::Return, line);
            let filter_fn = self.functions.pop().unwrap();
            let filter_chunk_id = self.chunks.len() + 1;
            let upvalues = filter_fn.upvalues;
            self.chunks.push(filter_fn.chunk);

            for (comp, _) in &q.components {
                let resolved = self.resolve_canonical_name(comp);
                self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
            }

            // QueryFilter pops a fn/closure value — mirror the non-mut
            // query path in expr.rs. (This used to push the chunk id as a
            // bare int constant, which died at runtime with "QueryFilter:
            // expected closure or function" for every `mut` + `where` query.)
            if upvalues.is_empty() {
                let fn_val = Value::from_fn(
                    &mut self.gc,
                    crate::value::FnValue {
                        name: "__query_filter_mut".to_string(),
                        arity: (q.components.len() + 1) as u8,
                        chunk_id: filter_chunk_id,
                    },
                );
                self.emit_constant(fn_val, line);
            } else {
                self.emit_op(Op::Closure, line);
                self.emit_u16(filter_chunk_id as u16, line);
                self.emit_byte((q.components.len() + 1) as u8, line);
                self.emit_byte(upvalues.len() as u8, line);
                for uv in &upvalues {
                    self.emit_byte(if uv.is_local { 1 } else { 0 }, line);
                    self.emit_u16(uv.index, line);
                }
            }

            self.emit_op(Op::QueryFilter, line);
            self.emit_byte(q.components.len() as u8, line);
        }

        self.add_local(list_name.clone(), false);
        let list_slot = self.resolve_local(&list_name).unwrap();

        self.emit_constant_gc(line, |gc| Value::from_int(gc, 0));
        self.add_local(idx_name.clone(), true);
        let idx_slot = self.resolve_local(&idx_name).unwrap();

        let loop_start = self.current_offset();
        self.mark_label_here();
        let loop_depth = self.current().scope_depth;
        let settlement_depth = self.current().settlement_depth;
        self.current().loop_contexts.push(LoopCtx {
            loop_depth,
            settlement_depth,
            loop_start,
            break_holes: Vec::new(),
            continue_holes: Vec::new(),
            writebacks: Vec::new(),
        });

        self.emit_get_local(idx_slot, line);
        self.emit_get_local(list_slot, line);
        self.emit_op(Op::Len, line);
        self.emit_op(Op::Lt, line);
        let exit_jump = self.emit_jump(Op::JumpIfFalse, line);

        self.begin_scope(); // Scope 1: entity_slot

        // Push entity id onto stack as a local
        self.emit_get_local(list_slot, line);
        self.emit_get_local(idx_slot, line);
        self.emit_op(Op::GetIndex, line);

        let mut writebacks = Vec::new();

        let (entity_slot, bind_offset) = if f.bindings.len() == q.components.len() + 1 {
            // User wants entity id as first binding
            self.add_local(f.bindings[0].clone(), false);
            let slot = self.resolve_local(&f.bindings[0]).unwrap();
            (slot, 1)
        } else {
            // Hidden entity id local
            let entity_id_name = self.fresh_name("entity_id");
            self.add_local(entity_id_name.clone(), false);
            let slot = self.resolve_local(&entity_id_name).unwrap();
            (slot, 0)
        };

        let mut skip_jumps = Vec::new();
        for (comp, _) in &q.components {
            self.emit_get_local(entity_slot, line);
            let resolved = self.resolve_canonical_name(comp);
            let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, resolved));
            self.emit_op(Op::EcsHas, line);
            self.emit_u16(type_idx, line);
            skip_jumps.push(self.emit_jump(Op::JumpIfFalse, line));
        }

        self.begin_scope(); // Scope 2: components

        for (i, (comp, is_mut)) in q.components.iter().enumerate() {
            let bind_name = &f.bindings[i + bind_offset];
            self.emit_get_local(entity_slot, line);
            let resolved = self.resolve_canonical_name(comp);
            let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, resolved));
            self.emit_op(Op::EcsGet, line);
            self.emit_u16(type_idx, line);
            self.add_local(bind_name.clone(), *is_mut);
            let comp_slot = self.resolve_local(bind_name).unwrap();
            if *is_mut {
                writebacks.push((entity_slot, comp_slot));
            }
        }

        self.current().loop_contexts.last_mut().unwrap().writebacks = writebacks.clone();

        self.compile_body(&f.body.stmts)?;

        // Perform writebacks for normal fallthrough
        for &(e_slot, c_slot) in &writebacks {
            self.emit_get_local(e_slot, line);
            self.emit_get_local(c_slot, line);
            self.emit_op(Op::EcsSet, line);
        }

        self.end_scope(line); // End Scope 2

        for jump in skip_jumps {
            self.patch_jump(jump);
        }

        self.end_scope(line); // End Scope 1

        let continue_target = self.current_offset();
        self.mark_label_here();

        self.emit_get_local(idx_slot, line);
        self.emit_constant_gc(line, |gc| Value::from_int(gc, 1));
        self.emit_op(Op::Add, line);
        self.emit_op(Op::SetLocal, line);
        self.emit_u16(idx_slot, line);

        let loop_end = self.current_offset();
        let delta = loop_end - loop_start + 3;
        self.emit_op(Op::JumpBack, line);
        self.emit_u16(delta as u16, line);

        self.patch_jump(exit_jump);
        let ctx = self.current().loop_contexts.pop().unwrap();
        for hole in ctx.break_holes {
            self.patch_jump(hole);
        }
        for hole in ctx.continue_holes {
            self.patch_jump_to(hole, continue_target);
        }

        self.end_scope(line);
        Ok(())
    }

    fn compile_return(&mut self, r: &ReturnStmt) -> Result<(), CompileError> {
        let line = r.span.line;
        if let Some(val) = &r.value {
            self.allow_pipe_fusion = true;
            self.compile_expr(val)?;
        } else {
            self.emit_constant(Value::NIL, line);
        }

        // Perform writebacks for all active loops before returning
        let mut all_writebacks = Vec::new();
        for ctx in &self.current().loop_contexts {
            all_writebacks.extend(ctx.writebacks.clone());
        }
        if !all_writebacks.is_empty() {
            // The return value is currently on top of the stack. We need to save it, do writebacks, and push it back.
            let ret_val_name = self.fresh_name("ret_val");
            self.add_local(ret_val_name.clone(), false);
            let ret_slot = self.resolve_local(&ret_val_name).unwrap();

            for (e_slot, c_slot) in all_writebacks {
                self.emit_get_local(e_slot, line);
                self.emit_get_local(c_slot, line);
                self.emit_op(Op::EcsSet, line);
            }

            self.emit_get_local(ret_slot, line);
        }

        self.emit_op(Op::Return, line);
        Ok(())
    }

    fn compile_break(&mut self, b: &BreakStmt) -> Result<(), CompileError> {
        let line = b.span.line;
        let current_settlement_depth = self.current().settlement_depth;
        let (loop_depth, target_settlement_depth) = self
            .current()
            .loop_contexts
            .last()
            .map(|ctx| (ctx.loop_depth, ctx.settlement_depth))
            .ok_or_else(|| CompileError {
                message: "'break' used outside of a loop".into(),
                line,
                col: b.span.col,
            })?;
        if target_settlement_depth != current_settlement_depth {
            return Err(CompileError {
                message: "`break` cannot cross a settlement boundary".into(),
                line,
                col: b.span.col,
            });
        }

        // Perform writebacks for the current loop before breaking
        let writebacks = self
            .current()
            .loop_contexts
            .last()
            .unwrap()
            .writebacks
            .clone();
        for (e_slot, c_slot) in writebacks {
            self.emit_get_local(e_slot, line);
            self.emit_get_local(c_slot, line);
            self.emit_op(Op::EcsSet, line);
        }

        let pop_count = self
            .current()
            .locals
            .iter()
            .rev()
            .take_while(|l| l.depth > loop_depth)
            .count();

        self.emit_pops(pop_count, line);

        let hole = self.emit_jump(Op::Jump, line);
        self.current()
            .loop_contexts
            .last_mut()
            .unwrap()
            .break_holes
            .push(hole);
        Ok(())
    }

    fn compile_continue(&mut self, c: &ContinueStmt) -> Result<(), CompileError> {
        let line = c.span.line;
        let current_settlement_depth = self.current().settlement_depth;
        let (loop_depth, target_settlement_depth) = self
            .current()
            .loop_contexts
            .last()
            .map(|ctx| (ctx.loop_depth, ctx.settlement_depth))
            .ok_or_else(|| CompileError {
                message: "'continue' used outside of a loop".into(),
                line,
                col: c.span.col,
            })?;
        if target_settlement_depth != current_settlement_depth {
            return Err(CompileError {
                message: "`continue` cannot cross a settlement boundary".into(),
                line,
                col: c.span.col,
            });
        }

        // Perform writebacks for the current loop before continuing
        let writebacks = self
            .current()
            .loop_contexts
            .last()
            .unwrap()
            .writebacks
            .clone();
        for (e_slot, c_slot) in writebacks {
            self.emit_get_local(e_slot, line);
            self.emit_get_local(c_slot, line);
            self.emit_op(Op::EcsSet, line);
        }

        let pop_count = self
            .current()
            .locals
            .iter()
            .rev()
            .take_while(|l| l.depth > loop_depth)
            .count();

        self.emit_pops(pop_count, line);

        let hole = self.emit_jump(Op::Jump, line);
        self.current()
            .loop_contexts
            .last_mut()
            .unwrap()
            .continue_holes
            .push(hole);
        Ok(())
    }

    fn compile_emit(&mut self, e: &EmitStmt) -> Result<(), CompileError> {
        let line = e.span.line;
        let resolved_event = self.resolve_canonical_name(&e.event_name);
        let name_idx = self.add_constant_gc(|gc| Value::from_string(gc, resolved_event.clone()));
        let field_count = e.fields.len() as u16;
        if let Some(delay) = &e.delay {
            self.compile_expr(delay)?;
        }
        for (_, fexpr) in &e.fields {
            self.compile_expr(fexpr)?;
        }
        self.emit_op(Op::MakeCompSlot, line);
        self.emit_u16(name_idx, line);
        self.emit_u16(field_count, line);
        if e.delay.is_some() {
            self.emit_op(Op::EmitAfter, line);
        } else {
            self.emit_op(Op::Emit, line);
        }
        Ok(())
    }

    fn compile_schedule(&mut self, s: &ScheduleStmt) -> Result<(), CompileError> {
        let line = s.span.line;
        let mut expanded = Vec::new();
        for sys in &s.systems {
            if let Some(phase_systems) = self.phases.get(sys).cloned() {
                expanded.extend(phase_systems);
            } else {
                expanded.push(sys.clone());
            }
        }
        let count = Self::checked_u16(expanded.len(), "schedule", line)?;
        // `schedule serial [...]` shares RunSchedule's operand layout but
        // runs one system at a time in topological order (dogfood seq 83).
        self.emit_op(
            if s.serial {
                Op::RunScheduleSerial
            } else {
                Op::RunSchedule
            },
            line,
        );
        self.emit_u16(count, line);
        for sys in &expanded {
            let resolved_sys = self.resolve_canonical_name(sys);
            let name_idx = self.add_constant_gc(|gc| Value::system_ref(gc, resolved_sys));
            self.emit_u16(name_idx, line);
        }
        Ok(())
    }

    fn compile_update(&mut self, u: &UpdateStmt) -> Result<(), CompileError> {
        let span = &u.span;
        let line = span.line;

        self.begin_scope();

        let comp_name_expr = Expr::Ident(u.comp_name.clone(), span.clone());
        let mut ent_ref_for_set: Option<Expr> = None;
        let get_call = if let Some(entity_expr) = &u.entity_expr {
            self.compile_expr(entity_expr)?;
            let ent_tmp = self.fresh_name("update_ent_");
            self.add_local(ent_tmp.clone(), false);
            let ent_ref = Expr::Ident(ent_tmp, span.clone());
            ent_ref_for_set = Some(ent_ref.clone());
            Expr::Call(
                Box::new(Expr::Ident("get".to_string(), span.clone())),
                vec![ent_ref, comp_name_expr.clone()],
                span.clone(),
            )
        } else {
            Expr::Call(
                Box::new(Expr::Ident("get_resource".to_string(), span.clone())),
                vec![comp_name_expr.clone()],
                span.clone(),
            )
        };
        let unwrap_call = Expr::Call(
            Box::new(Expr::Ident("unwrap".to_string(), span.clone())),
            vec![get_call],
            span.clone(),
        );
        // Fold the update entries into one expression per field, in written
        // order. Indexed entries (`vals[i] = x`) become `set_at(base, i, x)`
        // where base is the previous entry for that field, or the current
        // component value when the indexed write is the first mention.
        let mut folded: Vec<(String, Expr)> = Vec::new();
        for fu in &u.field_updates {
            let next = match &fu.index {
                None => fu.value.clone(),
                Some(idx) => {
                    let base = match folded.iter().find(|(n, _)| *n == fu.name) {
                        Some((_, prev)) => prev.clone(),
                        None => Expr::Field(
                            Box::new(unwrap_call.clone()),
                            fu.name.clone(),
                            span.clone(),
                        ),
                    };
                    Expr::Call(
                        Box::new(Expr::Ident("set_at".to_string(), span.clone())),
                        vec![base, idx.clone(), fu.value.clone()],
                        span.clone(),
                    )
                }
            };
            match folded.iter_mut().find(|(n, _)| *n == fu.name) {
                Some((_, slot)) => *slot = next,
                None => folded.push((fu.name.clone(), next)),
            }
        }
        let comp_expr = Expr::ComponentExpr(
            u.comp_name.clone(),
            folded,
            Some(Box::new(unwrap_call)),
            span.clone(),
        );
        let set_call = if let Some(ent_ref) = ent_ref_for_set {
            Expr::Call(
                Box::new(Expr::Ident("set".to_string(), span.clone())),
                vec![ent_ref, comp_expr],
                span.clone(),
            )
        } else {
            Expr::Call(
                Box::new(Expr::Ident("set_resource".to_string(), span.clone())),
                vec![comp_name_expr, comp_expr],
                span.clone(),
            )
        };
        self.compile_expr(&set_call)?;
        self.emit_op(Op::Pop, line);

        self.end_scope(line);
        Ok(())
    }

    fn compile_match(&mut self, m: &MatchStmt) -> Result<(), CompileError> {
        let line = m.span.line;
        self.begin_scope();
        self.compile_expr(&m.subject)?;
        let subject_local_name = self.fresh_name("match_subject");
        self.add_local(subject_local_name.clone(), false);
        let subject_slot = self
            .resolve_local(&subject_local_name)
            .ok_or(CompileError {
                message: "Internal compiler error: failed to resolve match subject local"
                    .to_string(),
                line,
                col: m.span.col,
            })?;

        let mut end_jumps = Vec::new();

        for case in &m.cases {
            let next_case_hole = match &case.pattern {
                Pattern::Wildcard => None,
                Pattern::Literal(lit) => {
                    self.emit_get_local(subject_slot, line);
                    self.compile_expr(lit)?;
                    self.emit_op(Op::Eq, line);
                    Some(self.emit_jump(Op::JumpIfFalse, line))
                }
                Pattern::Variant { path, .. } => {
                    let variant_name = path.last().unwrap();
                    let pattern_idx =
                        self.add_constant_gc(|gc| Value::from_string(gc, variant_name.clone()));
                    self.emit_op(Op::MatchState, line);
                    self.emit_u16(pattern_idx, line);
                    let hole = self.current_offset();
                    self.emit_u16(0xFFFF, line);
                    Some(hole)
                }
                Pattern::HasComponent { component, .. } => {
                    self.emit_get_local(subject_slot, line);
                    let comp_idx =
                        self.add_constant_gc(|gc| Value::from_string(gc, component.clone()));
                    self.emit_op(Op::EcsHas, line);
                    self.emit_u16(comp_idx, line);
                    Some(self.emit_jump(Op::JumpIfFalse, line))
                }
            };

            self.begin_scope();

            let bindings = match &case.pattern {
                Pattern::Variant {
                    bindings,
                    pattern_bindings,
                    ..
                } => {
                    if !pattern_bindings.is_empty() {
                        pattern_bindings.clone()
                    } else {
                        bindings
                            .iter()
                            .map(|name| MatchBinding {
                                name: name.clone(),
                                path: vec![name.clone()],
                            })
                            .collect()
                    }
                }
                _ => vec![],
            };
            if let Pattern::HasComponent {
                component,
                binding: Some(bind_name),
            } = &case.pattern
            {
                self.emit_get_local(subject_slot, line);
                let comp_idx = self.add_constant_gc(|gc| Value::from_string(gc, component.clone()));
                self.emit_op(Op::EcsGet, line);
                self.emit_u16(comp_idx, line);
                self.add_local(bind_name.clone(), false);
            }
            for binding in &bindings {
                self.emit_get_local(subject_slot, line);
                for segment in &binding.path {
                    let field_idx =
                        self.add_constant_gc(|gc| Value::from_string(gc, segment.clone()));
                    self.emit_op(Op::GetField, line);
                    self.emit_u16(field_idx, line);
                }
                self.add_local(binding.name.clone(), false);
            }

            let mut guard_fail_hole = None;
            if let Some(guard) = &case.guard {
                self.compile_expr(guard)?;
                guard_fail_hole = Some(self.emit_jump(Op::JumpIfFalse, line));
            }

            self.compile_body(&case.body.stmts)?;
            self.end_scope(line);

            let end_j = self.emit_jump(Op::Jump, line);
            end_jumps.push(end_j);

            if let Some(guard_hole) = guard_fail_hole {
                self.patch_jump(guard_hole);
                for _ in 0..bindings.len() {
                    self.emit_op(Op::Pop, line);
                }
            }

            if let Some(hole) = next_case_hole {
                self.patch_jump(hole);
            }
        }

        for j in end_jumps {
            self.patch_jump(j);
        }
        self.end_scope(line);
        Ok(())
    }
}