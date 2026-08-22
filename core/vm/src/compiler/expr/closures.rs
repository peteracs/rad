// Lowering the expressions that produce callables: closures with their
// capture list, and references to declared systems.

impl Compiler {
    pub(crate) fn compile_closure_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::FnExpr(params, param_muts, _, param_destructures, _, body, span) => {
                let fn_name = format!("<anon@{}:{}>", span.line, span.col);
                let mut fn_scope = Self::new_fn_scope(&fn_name);
                fn_scope.unique_locals = super::escape::find_unique_locals(body);
                self.functions.push(fn_scope);

                for (i, param) in params.iter().enumerate() {
                    let is_mut = param_muts.get(i).copied().unwrap_or(false);
                    self.add_local(param.clone(), is_mut);
                }

                for (i, param) in params.iter().enumerate() {
                    if let Some(bindings) = param_destructures.get(i).and_then(|d| d.as_ref()) {
                        let is_mut = param_muts.get(i).copied().unwrap_or(false);
                        let param_slot = self.resolve_local(param).ok_or_else(|| CompileError {
                            message: "internal: closure destructure param local".to_string(),
                            line: span.line,
                            col: span.col,
                        })?;
                        for (j, name) in bindings.iter().enumerate() {
                            self.emit_get_local(param_slot, span.line);
                            self.emit_constant_gc(span.line, |gc| Value::from_int(gc, j as i64));
                            self.emit_op(Op::GetIndex, span.line);
                            self.add_local(name.clone(), is_mut);
                        }
                        self.emit_constant(Value::NIL, span.line);
                        self.emit_op(Op::SetLocal, span.line);
                        self.emit_u16(param_slot, span.line);
                    }
                }

                if let Some((last_stmt, prefix)) = body.stmts.split_last() {
                    match last_stmt {
                        Stmt::Expr(expr_stmt) => {
                            self.compile_body(prefix)?;
                            self.compile_expr(&expr_stmt.expr)?;
                            self.emit_op(Op::Return, expr_stmt.span.line);
                        }
                        _ => {
                            self.compile_body(&body.stmts)?;
                            self.emit_constant(Value::NIL, span.line);
                            self.emit_op(Op::Return, span.line);
                        }
                    }
                } else {
                    self.emit_constant(Value::NIL, span.line);
                    self.emit_op(Op::Return, span.line);
                }

                let scope = self.functions.pop().unwrap();
                let chunk_id = self.chunks.len() + 1;
                let upvalues = scope.upvalues;
                self.chunks.push(scope.chunk);

                if upvalues.is_empty() {
                    let fn_val = Value::from_fn(
                        &mut self.gc,
                        FnValue {
                            name: fn_name,
                            arity: params.len() as u8,
                            chunk_id,
                        },
                    );
                    self.emit_constant(fn_val, span.line);
                } else {
                    self.emit_op(Op::Closure, span.line);
                    self.emit_u16(chunk_id as u16, span.line);
                    self.emit_byte(params.len() as u8, span.line);
                    self.emit_byte(upvalues.len() as u8, span.line);
                    for uv in &upvalues {
                        self.emit_byte(if uv.is_local { 1 } else { 0 }, span.line);
                        self.emit_u16(uv.index, span.line);
                    }
                }
            }
            Expr::SystemRef(path, span) => {
                let q = crate::simulate_syntax::system_ref_qualified_string(path);
                let resolved = self.resolve_canonical_name(&q);
                if !self.is_system(&resolved) {
                    return Err(CompileError {
                        message: format!(
                            "Unknown system '{}' in system reference (each system must be compiled before use)",
                            q
                        ),
                        line: span.line,
                        col: span.col,
                    });
                }
                self.emit_constant_gc(span.line, |gc| Value::system_ref(gc, resolved));
            }
            other => unreachable!(
                "compile_closure_expr received {other:?}, which its dispatcher never routes here"
            ),
        }
        Ok(())
    }
}
