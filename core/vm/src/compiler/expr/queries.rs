// Lowering query expressions into their filter/project opcode pipeline.

impl Compiler {
    pub(crate) fn compile_query_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::QueryExpr(query, span) => {
                let line = span.line;

                let (without_types, remaining_filter) = if let Some(filter_expr) = &query.filter {
                    Self::extract_query_negations(filter_expr)
                } else {
                    (Vec::new(), None)
                };

                for (comp, _) in &query.components {
                    let resolved = self.resolve_canonical_name(comp);
                    self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
                }
                for comp in &without_types {
                    let resolved = self.resolve_canonical_name(comp);
                    self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
                }

                self.emit_op(Op::EcsQuery, line);
                self.emit_byte(query.components.len() as u8, line);
                self.emit_byte(without_types.len() as u8, line);

                if let Some(filter_expr) = &remaining_filter {
                    let filter_scope = Compiler::new_fn_scope("__query_filter");
                    self.functions.push(filter_scope);
                    self.add_local("__entity".to_string(), false);
                    for (comp, _) in &query.components {
                        self.add_local(comp.clone(), false);
                    }
                    self.compile_expr(filter_expr)?;
                    self.emit_op(Op::Return, line);
                    let filter_fn = self.functions.pop().unwrap();
                    let filter_chunk_id = self.chunks.len() + 1;
                    let upvalues = filter_fn.upvalues;
                    self.chunks.push(filter_fn.chunk);

                    for (comp, _) in &query.components {
                        let resolved = self.resolve_canonical_name(comp);
                        self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
                    }

                    if upvalues.is_empty() {
                        let fn_val = Value::from_fn(
                            &mut self.gc,
                            crate::value::FnValue {
                                name: "__query_filter".to_string(),
                                arity: (query.components.len() + 1) as u8,
                                chunk_id: filter_chunk_id,
                            },
                        );
                        self.emit_constant(fn_val, line);
                    } else {
                        self.emit_op(Op::Closure, line);
                        self.emit_u16(filter_chunk_id as u16, line);
                        self.emit_byte((query.components.len() + 1) as u8, line);
                        self.emit_byte(upvalues.len() as u8, line);
                        for uv in &upvalues {
                            self.emit_byte(if uv.is_local { 1 } else { 0 }, line);
                            self.emit_u16(uv.index, line);
                        }
                    }

                    self.emit_op(Op::QueryFilter, line);
                    self.emit_byte(query.components.len() as u8, line);
                }

                if !query.select.is_empty() {
                    for sel in &query.select {
                        let resolved = self.resolve_canonical_name(sel);
                        self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
                    }
                    self.emit_op(Op::QueryProject, line);
                    self.emit_byte(query.select.len() as u8, line);
                }
            }
            other => unreachable!(
                "compile_query_expr received {other:?}, which its dispatcher never routes here"
            ),
        }
        Ok(())
    }
}
