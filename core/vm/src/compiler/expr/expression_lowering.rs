impl Compiler {
    pub(crate) fn compile_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        // consume the fusion grant: only the expression the statement
        // compiler handed us directly may loop-fuse; subexpressions
        // compile with values already on the operand stack
        let pipe_fusion_ok = std::mem::take(&mut self.allow_pipe_fusion);
        match expr {
            Expr::IntLit(n, span) => {
                self.emit_constant_gc(span.line, |gc| Value::from_int(gc, *n));
            }
            Expr::FloatLit(x, span) => {
                self.emit_constant(Value::from_float(*x), span.line);
            }
            Expr::StrLit(s, span) => {
                self.emit_constant_gc(span.line, |gc| Value::from_string(gc, s.clone()));
            }
            Expr::BoolLit(b, span) => {
                self.emit_constant(Value::from_bool(*b), span.line);
            }
            Expr::NilLit(span) => {
                self.emit_constant(Value::NIL, span.line);
            }
            Expr::TupleLit(elems, span) => {
                let count = Self::checked_u16(elems.len(), "Tuple literal", span.line)?;
                for e in elems {
                    self.compile_expr(e)?;
                }
                self.emit_op(Op::MakeTuple, span.line);
                self.emit_u16(count, span.line);
            }
            Expr::Spread(_expr, _span) => {
                return Err(CompileError {
                    message: "Spread operator is not supported here".to_string(),
                    line: _span.line,
                    col: _span.col,
                });
            }
            Expr::ListLit(elems, span) => {
                let count = Self::checked_u16(elems.len(), "List literal", span.line)?;
                for e in elems {
                    self.compile_expr(e)?;
                }
                self.emit_op(Op::MakeList, span.line);
                self.emit_u16(count, span.line);
            }
            Expr::MapLit(entries, span) => {
                for (key, val) in entries {
                    self.compile_expr(key)?;
                    self.compile_expr(val)?;
                }
                self.emit_op(Op::MakeMap, span.line);
                self.emit_u16(entries.len() as u16, span.line);
            }
            Expr::FStringExpr(parts, span) => {
                if parts.is_empty() {
                    self.emit_constant_gc(span.line, |gc| Value::from_string(gc, String::new()));
                } else {
                    // All parts land on the stack as strings, then ONE
                    // ConcatN builds the result in a single exact-capacity
                    // buffer. The old lowering chained k-1 `Add`s, each
                    // re-copying the growing prefix — O(parts²) bytes moved
                    // per f-string (Tier-1 #2).
                    let mut pending: u32 = 0;
                    for part in parts {
                        match part {
                            FStringPart::Lit(s) => {
                                self.emit_constant_gc(span.line, |gc| {
                                    Value::from_string(gc, s.clone())
                                });
                            }
                            FStringPart::Expr(expr, spec) => {
                                self.compile_expr(expr)?;
                                if let Some(spec_str) = spec {
                                    self.emit_constant_gc(span.line, |gc| {
                                        Value::from_string(gc, spec_str.clone())
                                    });
                                    let fv_slot = self.ensure_global_slot("format_value");
                                    self.emit_op(Op::GetGlobal, span.line);
                                    self.emit_u16(fv_slot, span.line);
                                    self.emit_op(Op::Call, span.line);
                                    self.emit_byte(2, span.line);
                                } else {
                                    let str_slot = self.ensure_global_slot("str");
                                    self.emit_op(Op::GetGlobal, span.line);
                                    self.emit_u16(str_slot, span.line);
                                    self.emit_op(Op::Call, span.line);
                                    self.emit_byte(1, span.line);
                                }
                            }
                        }
                        pending += 1;
                        // ConcatN's count is a byte; gigantic f-strings fold
                        // in waves (the folded prefix counts as one part).
                        if pending == 255 {
                            self.emit_op(Op::ConcatN, span.line);
                            self.emit_byte(255, span.line);
                            pending = 1;
                        }
                    }
                    if pending > 1 {
                        self.emit_op(Op::ConcatN, span.line);
                        self.emit_byte(pending as u8, span.line);
                    }
                }
            }
            Expr::Ident(name, span) => {
                if let Some(slot) = self.resolve_local(name) {
                    self.emit_get_local(slot, span.line);
                } else {
                    let fn_idx = self.functions.len() - 1;
                    if let Some(uv_idx) = self.resolve_upvalue(fn_idx, name) {
                        self.emit_op(Op::GetUpvalue, span.line);
                        self.emit_u16(uv_idx, span.line);
                    } else {
                        let resolved_name = self
                            .resolve_current_alias(name)
                            .unwrap_or_else(|| name.clone());
                        let slot = self.ensure_global_slot(&resolved_name);
                        self.emit_op(Op::GetGlobal, span.line);
                        self.emit_u16(slot, span.line);
                    }
                }
            }
            Expr::Binary(..) | Expr::Unary(..) => self.compile_operator_expr(expr)?,
            Expr::Pipe(_, _, span) => {
                let line = span.line;
                if let Some((source, steps)) = Self::try_collect_fusable_pipe(expr) {
                    if !self.in_causal_region() && Self::can_vectorize_pipeline(&steps) {
                        // safe anywhere: accumulator is a global slot
                        self.compile_vectorized_pipeline(source, &steps, line)?;
                    } else if pipe_fusion_ok {
                        self.warnings.push(super::CompileWarning {
                            message: "W2505: Closure too complex for vectorization, \
                                      falling back to scalar pipeline"
                                .to_string(),
                            line,
                            col: span.col,
                        });
                        self.compile_lowered_pipeline(source, &steps, line)?;
                    } else {
                        // expression position: the scalar loop's locals
                        // would alias operand-stack values — plain calls
                        self.compile_pipe_unfused(expr)?;
                    }
                } else {
                    self.compile_pipe_unfused(expr)?;
                }
            }
            Expr::Call(..) | Expr::Await(..) | Expr::Try(..) | Expr::AsyncCall(..) => self.compile_call_expr(expr)?,
            Expr::Field(obj, field, span) => {
                if let Expr::Ident(alias_name, _) = obj.as_ref() {
                    let is_local = self.resolve_local(alias_name).is_some() || {
                        let fn_idx = self.functions.len() - 1;
                        fn_idx > 0 && self.resolve_upvalue(fn_idx, alias_name).is_some()
                    };
                    let is_global = self.global_slots.contains_key(alias_name);
                    if !is_local && !is_global && self.module_aliases.contains_key(alias_name) {
                        if let Some(mangled) = self.resolve_alias_member(alias_name, field) {
                            return self.compile_expr(&Expr::Ident(mangled, span.clone()));
                        }
                    }
                }
                self.compile_expr(obj)?;
                let idx = self.add_constant_gc(|gc| Value::from_string(gc, field.clone()));
                self.emit_op(Op::GetField, span.line);
                self.emit_u16(idx, span.line);
            }
            Expr::Index(obj, idx_expr, span) => {
                // `local[idx]` fuses GetLocal+GetIndex into one dispatch and
                // skips the stack round-trip of the container. Reads are
                // alias-safe for any local; the MoveLocal tracking entry is
                // cleared so a later self-assign can't nil the slot out from
                // under this read.
                if let Expr::Ident(name, _) = obj.as_ref() {
                    if let Some(slot) = self.resolve_local(name) {
                        // both list and index are locals: one dispatch total
                        if let Expr::Ident(idx_name, _) = idx_expr.as_ref() {
                            if let Some(idx_slot) = self.resolve_local(idx_name) {
                                self.emit_op(Op::ListGetLL, span.line);
                                self.emit_u16(slot, span.line);
                                self.emit_u16(idx_slot, span.line);
                                self.current().last_get_local.remove(&slot);
                                self.current().last_get_local.remove(&idx_slot);
                                return Ok(());
                            }
                        }
                        self.compile_expr(idx_expr)?;
                        self.emit_op(Op::ListGetLocal, span.line);
                        self.emit_u16(slot, span.line);
                        self.current().last_get_local.remove(&slot);
                        return Ok(());
                    }
                }
                self.compile_expr(obj)?;
                self.compile_expr(idx_expr)?;
                self.emit_op(Op::GetIndex, span.line);
            }
            Expr::ComponentExpr(..) | Expr::StateRef(..) | Expr::VariantExpr(..) | Expr::EntityLiteral(..) => self.compile_construction_expr(expr)?,
            Expr::IfExpr(..) | Expr::MatchExpr(..) => self.compile_branching_expr(expr)?,
            Expr::QueryExpr(..) => self.compile_query_expr(expr)?,
            Expr::FnExpr(..) | Expr::SystemRef(..) => self.compile_closure_expr(expr)?,
            Expr::Error(_) => {}
        }
        Ok(())
    }

    fn compile_pipe_unfused(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::Pipe(left, right, span) => {
                self.compile_expr(left)?;
                match right.as_ref() {
                    Expr::Call(callee, args, _) => {
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        self.compile_expr(callee)?;
                        let total_argc = (args.len() + 1) as u8;
                        self.emit_op(Op::Call, span.line);
                        self.emit_byte(total_argc, span.line);
                    }
                    _ => {
                        self.compile_expr(right)?;
                        self.emit_op(Op::Call, span.line);
                        self.emit_byte(1, span.line);
                    }
                }
                Ok(())
            }
            _ => self.compile_expr(expr),
        }
    }

    fn try_classify_pipe_step(callee: &Expr, args: &[Expr]) -> Option<PipelineOp> {
        if args.len() != 1 {
            return None;
        }
        if let Expr::Ident(name, _) = callee {
            match name.as_str() {
                "map" => Some(PipelineOp::Map),
                "filter" => Some(PipelineOp::Filter),
                _ => None,
            }
        } else {
            None
        }
    }
}
