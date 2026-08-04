impl Compiler {

    pub(crate) fn extract_query_negations(expr: &Expr) -> (Vec<String>, Option<Expr>) {
        match expr {
            Expr::Binary(left, BinOp::And, right, span) => {
                let (mut l_neg, l_rem) = Self::extract_query_negations(left);
                let (mut r_neg, r_rem) = Self::extract_query_negations(right);
                l_neg.append(&mut r_neg);

                let rem = match (l_rem, r_rem) {
                    (Some(l), Some(r)) => Some(Expr::Binary(
                        Box::new(l),
                        BinOp::And,
                        Box::new(r),
                        span.clone(),
                    )),
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                    (None, None) => None,
                };
                (l_neg, rem)
            }
            Expr::Unary(UnaryOp::Not, operand, _) => {
                if let Expr::Ident(name, _) = operand.as_ref() {
                    (vec![name.clone()], None)
                } else {
                    (Vec::new(), Some(expr.clone()))
                }
            }
            _ => (Vec::new(), Some(expr.clone())),
        }
    }

    fn try_collect_fusable_pipe(expr: &Expr) -> Option<(&Expr, Vec<(&Expr, PipelineOp)>)> {
        let mut steps: Vec<(&Expr, PipelineOp)> = Vec::new();
        let mut current = expr;

        while let Expr::Pipe(left, right, _) = current {
            if let Expr::Call(callee, args, _) = right.as_ref() {
                if let Some(op) = Self::try_classify_pipe_step(callee, args) {
                    steps.push((&args[0], op));
                    current = left;
                    continue;
                }
            }
            return None;
        }

        if steps.is_empty() {
            return None;
        }

        steps.reverse();
        Some((current, steps))
    }

    fn extract_closure_param_and_body(expr: &Expr) -> Option<(String, VectorizableBody<'_>)> {
        if let Expr::FnExpr(params, _, _, param_destructures, _, body, _) = expr {
            if param_destructures.iter().any(|d| d.is_some()) {
                return None;
            }
            if params.len() == 1 && body.stmts.len() == 1 {
                match &body.stmts[0] {
                    Stmt::Expr(ExprStmt {
                        expr: body_expr, ..
                    }) => {
                        return Some((params[0].clone(), VectorizableBody::Expr(body_expr)));
                    }
                    Stmt::Return(crate::ast::ReturnStmt {
                        value: Some(body_expr),
                        ..
                    }) => {
                        return Some((params[0].clone(), VectorizableBody::Expr(body_expr)));
                    }
                    Stmt::If(crate::ast::IfStmt {
                        condition,
                        then_block,
                        else_block: Some(else_block),
                        ..
                    }) if then_block.stmts.len() == 1 && else_block.stmts.len() == 1 => {
                        let then_expr = match &then_block.stmts[0] {
                            Stmt::Return(crate::ast::ReturnStmt { value: Some(e), .. }) => Some(e),
                            Stmt::Expr(ExprStmt { expr: e, .. }) => Some(e),
                            _ => None,
                        };
                        let else_expr = match &else_block.stmts[0] {
                            Stmt::Return(crate::ast::ReturnStmt { value: Some(e), .. }) => Some(e),
                            Stmt::Expr(ExprStmt { expr: e, .. }) => Some(e),
                            _ => None,
                        };
                        if let (Some(t), Some(e)) = (then_expr, else_expr) {
                            return Some((
                                params[0].clone(),
                                VectorizableBody::IfElse {
                                    cond: condition,
                                    then_expr: t,
                                    else_expr: e,
                                },
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    #[allow(clippy::only_used_in_recursion)]
    fn is_vectorizable_expr(body: &Expr, param_name: &str) -> bool {
        match body {
            Expr::IntLit(..) | Expr::FloatLit(..) | Expr::BoolLit(..) | Expr::NilLit(..) => true,
            Expr::StrLit(..) => true,
            Expr::Ident(..) => true,
            Expr::Binary(left, op, right, _) => {
                matches!(
                    op,
                    BinOp::Add
                        | BinOp::Sub
                        | BinOp::Mul
                        | BinOp::Div
                        | BinOp::Mod
                        | BinOp::Eq
                        | BinOp::Ne
                        | BinOp::Lt
                        | BinOp::Le
                        | BinOp::Gt
                        | BinOp::Ge
                ) && Self::is_vectorizable_expr(left, param_name)
                    && Self::is_vectorizable_expr(right, param_name)
            }
            Expr::Unary(op, operand, _) => {
                matches!(op, UnaryOp::Neg | UnaryOp::Not)
                    && Self::is_vectorizable_expr(operand, param_name)
            }
            _ => false,
        }
    }

    fn is_vectorizable(body: &VectorizableBody, param_name: &str) -> bool {
        match body {
            VectorizableBody::Expr(e) => Self::is_vectorizable_expr(e, param_name),
            VectorizableBody::IfElse {
                cond,
                then_expr,
                else_expr,
            } => {
                Self::is_vectorizable_expr(cond, param_name)
                    && Self::is_vectorizable_expr(then_expr, param_name)
                    && Self::is_vectorizable_expr(else_expr, param_name)
            }
        }
    }

    fn can_vectorize_pipeline(steps: &[(&Expr, PipelineOp)]) -> bool {
        steps.iter().all(|(func_expr, _)| {
            Self::extract_closure_param_and_body(func_expr)
                .is_some_and(|(param, body)| Self::is_vectorizable(&body, &param))
        })
    }

    fn compile_vectorized_pipeline(
        &mut self,
        source: &Expr,
        steps: &[(&Expr, PipelineOp)],
        line: u32,
    ) -> Result<(), CompileError> {
        // The accumulator lives in a GLOBAL scratch slot, not a local:
        // local slots are frame-relative and only correct when the
        // operand stack is empty — in expression position (a call
        // argument, an f-string part) values already sit on the stack
        // and a local here would alias them. Same pattern as the match
        // expression's result slot.
        self.compile_expr(source)?;
        let vec_name = self.fresh_name("vec_in");
        let vec_slot = self.ensure_global_slot(&vec_name);
        self.emit_op(Op::SetGlobal, line);
        self.emit_u16(vec_slot, line);

        for (func_expr, op) in steps {
            let (param, body) = Self::extract_closure_param_and_body(func_expr).unwrap();

            match op {
                PipelineOp::Map => {
                    self.compile_vec_body(&body, &param, vec_slot, line)?;
                    self.emit_op(Op::SetGlobal, line);
                    self.emit_u16(vec_slot, line);
                }
                PipelineOp::Filter => {
                    self.emit_get_vec_global(vec_slot, line);
                    self.compile_vec_body(&body, &param, vec_slot, line)?;
                    self.emit_op(Op::VecFilter, line);
                    self.emit_op(Op::SetGlobal, line);
                    self.emit_u16(vec_slot, line);
                }
            }
        }

        self.emit_get_vec_global(vec_slot, line);
        Ok(())
    }

    fn emit_get_vec_global(&mut self, slot: u16, line: u32) {
        self.emit_op(Op::GetGlobal, line);
        self.emit_u16(slot, line);
    }

    fn compile_vec_body(
        &mut self,
        body: &VectorizableBody,
        param_name: &str,
        vec_slot: u16,
        line: u32,
    ) -> Result<(), CompileError> {
        match body {
            VectorizableBody::Expr(e) => self.compile_vec_expr(e, param_name, vec_slot, line),
            VectorizableBody::IfElse {
                cond,
                then_expr,
                else_expr,
            } => {
                self.compile_vec_expr(cond, param_name, vec_slot, line)?;
                self.compile_vec_expr(then_expr, param_name, vec_slot, line)?;
                self.compile_vec_expr(else_expr, param_name, vec_slot, line)?;
                self.emit_op(Op::VecSelect, line);
                Ok(())
            }
        }
    }

    fn compile_vec_expr(
        &mut self,
        expr: &Expr,
        param_name: &str,
        vec_slot: u16,
        line: u32,
    ) -> Result<(), CompileError> {
        match expr {
            Expr::IntLit(n, span) => {
                let val = Value::from_int(&mut self.gc, *n);
                self.emit_constant(val, span.line);
                self.emit_get_vec_global(vec_slot, line);
                self.emit_op(Op::VecBroadcast, span.line);
            }
            Expr::FloatLit(f, span) => {
                self.emit_constant(Value::from_float(*f), span.line);
                self.emit_get_vec_global(vec_slot, line);
                self.emit_op(Op::VecBroadcast, span.line);
            }
            Expr::BoolLit(b, span) => {
                self.emit_constant(Value::from_bool(*b), span.line);
                self.emit_get_vec_global(vec_slot, line);
                self.emit_op(Op::VecBroadcast, span.line);
            }
            Expr::NilLit(span) => {
                self.emit_constant(Value::NIL, span.line);
                self.emit_get_vec_global(vec_slot, line);
                self.emit_op(Op::VecBroadcast, span.line);
            }
            Expr::StrLit(s, span) => {
                self.emit_constant_gc(span.line, |gc| Value::from_string(gc, s.clone()));
                self.emit_get_vec_global(vec_slot, line);
                self.emit_op(Op::VecBroadcast, span.line);
            }
            Expr::Ident(name, _) => {
                if name == param_name {
                    self.emit_get_vec_global(vec_slot, line);
                } else {
                    self.compile_expr(expr)?;
                }
            }
            Expr::Binary(left, op, right, _) => {
                self.compile_vec_expr(left, param_name, vec_slot, line)?;
                self.compile_vec_expr(right, param_name, vec_slot, line)?;
                let vec_op = match op {
                    BinOp::Add => Op::VecAdd,
                    BinOp::Sub => Op::VecSub,
                    BinOp::Mul => Op::VecMul,
                    BinOp::Div => Op::VecDiv,
                    BinOp::Mod => Op::VecMod,
                    BinOp::Eq => Op::VecEq,
                    BinOp::Ne => Op::VecNeq,
                    BinOp::Lt => Op::VecLt,
                    BinOp::Le => Op::VecLte,
                    BinOp::Gt => Op::VecGt,
                    BinOp::Ge => Op::VecGte,
                    _ => unreachable!(),
                };
                self.emit_op(vec_op, line);
            }
            Expr::Unary(op, operand, _) => {
                self.compile_vec_expr(operand, param_name, vec_slot, line)?;
                match op {
                    UnaryOp::Neg => self.emit_op(Op::VecNeg, line),
                    UnaryOp::Not => self.emit_op(Op::VecNot, line),
                    // excluded by is_vectorizable_expr
                    UnaryOp::BitNot => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}