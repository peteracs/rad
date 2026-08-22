// Lowering value-producing control flow: conditional expressions and pattern
// matches, with the jump patching each requires.

impl Compiler {
    pub(crate) fn compile_branching_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::IfExpr(cond, then_e, else_e, span) => {
                let line = span.line;
                self.compile_expr(cond)?;
                let else_jump = self.emit_jump(Op::JumpIfFalse, line);
                self.compile_expr(then_e)?;
                let end_jump = self.emit_jump(Op::Jump, line);
                self.patch_jump(else_jump);
                self.compile_expr(else_e)?;
                self.patch_jump(end_jump);
            }
            Expr::MatchExpr(m, span) => {
                let line = span.line;
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
                        col: span.col,
                    })?;
                let result_name = self.fresh_name("match_result");
                let result_slot = self.ensure_global_slot(&result_name);
                self.emit_constant(Value::NIL, line);
                self.emit_op(Op::SetGlobal, line);
                self.emit_u16(result_slot, line);

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
                            // Op::MatchState compares the VARIANT name only
                            // ("Free", not "Cc::Free") — same convention as
                            // the statement compiler. The qualified form
                            // never matched, so every variant arm in a match
                            // EXPRESSION silently fell through to nil.
                            let variant_name = path.last().unwrap();
                            let pattern_idx = self
                                .add_constant_gc(|gc| Value::from_string(gc, variant_name.clone()));
                            self.emit_op(Op::MatchState, line);
                            self.emit_u16(pattern_idx, line);
                            let hole = self.current_offset();
                            self.emit_u16(0xFFFF, line);
                            Some(hole)
                        }
                        Pattern::HasComponent { component, .. } => {
                            self.emit_get_local(subject_slot, line);
                            let comp_idx = self.add_canonical_name_constant(component);
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
                        let comp_idx = self.add_canonical_name_constant(component);
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

                    if let Some((last_stmt, prefix)) = case.body.stmts.split_last() {
                        self.compile_body(prefix)?;
                        match last_stmt {
                            Stmt::Expr(expr_stmt) => {
                                self.compile_expr(&expr_stmt.expr)?;
                            }
                            _ => {
                                self.compile_stmt(last_stmt)?;
                                self.emit_constant(Value::NIL, line);
                            }
                        }
                    } else {
                        self.emit_constant(Value::NIL, line);
                    }
                    self.emit_op(Op::SetGlobal, line);
                    self.emit_u16(result_slot, line);
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
                self.emit_op(Op::GetGlobal, line);
                self.emit_u16(result_slot, line);
            }
            other => unreachable!(
                "compile_branching_expr received {other:?}, which its dispatcher never routes here"
            ),
        }
        Ok(())
    }
}
