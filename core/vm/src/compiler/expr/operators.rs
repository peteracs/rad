// Lowering unary and binary operator expressions, including the fused
// compare-and-branch and constant-arithmetic forms.

impl Compiler {
    pub(crate) fn compile_operator_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::Binary(left, op, right, span) => {
                if let Some(folded) = Self::try_const_fold(&mut self.gc, left, op, right, span)? {
                    self.emit_constant(folded, span.line);
                    return Ok(());
                }
                match op {
                    BinOp::And => {
                        self.compile_expr(left)?;
                        let short = self.emit_jump(Op::JumpIfFalse, span.line);
                        self.compile_expr(right)?;
                        self.emit_op(Op::Not, span.line);
                        self.emit_op(Op::Not, span.line);
                        let end = self.emit_jump(Op::Jump, span.line);
                        self.patch_jump(short);
                        self.emit_constant(Value::from_bool(false), span.line);
                        self.patch_jump(end);
                        return Ok(());
                    }
                    BinOp::Or => {
                        self.compile_expr(left)?;
                        let short = self.emit_jump(Op::JumpIfFalse, span.line);
                        self.emit_constant(Value::from_bool(true), span.line);
                        let end = self.emit_jump(Op::Jump, span.line);
                        self.patch_jump(short);
                        self.compile_expr(right)?;
                        self.emit_op(Op::Not, span.line);
                        self.emit_op(Op::Not, span.line);
                        self.patch_jump(end);
                        return Ok(());
                    }
                    BinOp::Is => {
                        self.compile_expr(left)?;
                        if let Expr::Ident(name, _) = &**right {
                            let pattern_idx =
                                self.add_constant_gc(|gc| Value::from_string(gc, name.clone()));
                            self.emit_op(Op::IsVariant, span.line);
                            self.emit_u16(pattern_idx, span.line);
                        } else {
                            return Err(CompileError {
                                message: "Right side of 'is' must be an identifier".to_string(),
                                line: span.line,
                                col: span.col,
                            });
                        }
                        return Ok(());
                    }
                    _ => {}
                }
                // String-concat chain fusion (Tier-1 #2): `s + "ab" + f"{x}"`
                // used to compile to k-1 binary Adds, each re-copying the
                // growing prefix — O(chain²) bytes moved. A chain containing
                // a string literal or f-string can only succeed all-string
                // (rad has no implicit coercion: `+` between a string and
                // anything else is a type error either way), so it fuses
                // into one ConcatN: every operand copied exactly once.
                // Evaluation order is unchanged (left spine, then right).
                if matches!(op, BinOp::Add) {
                    let mut chain: Vec<&Expr> = Vec::new();
                    Self::flatten_add_chain(left, right, &mut chain);
                    let provably_string = chain
                        .iter()
                        .any(|e| matches!(e, Expr::StrLit(..) | Expr::FStringExpr(..)));
                    if provably_string && chain.len() >= 3 && chain.len() <= 255 {
                        for part in &chain {
                            self.compile_expr(part)?;
                        }
                        self.emit_op(Op::ConcatN, span.line);
                        self.emit_byte(chain.len() as u8, span.line);
                        return Ok(());
                    }
                }
                // Constant-rhs fusions: `x == K`, `x != K`, and
                // `x <arith/bit-op> K` skip the separate Const dispatch
                // (15% of all dispatches in bitboard workloads — every
                // `== 0`, `% 512`, `+ 1` pays it).
                if let Expr::IntLit(k, _) = &**right {
                    let fusable = matches!(
                        op,
                        BinOp::Eq
                            | BinOp::Ne
                            | BinOp::Add
                            | BinOp::Sub
                            | BinOp::Mul
                            | BinOp::Div
                            | BinOp::Mod
                            | BinOp::BitAnd
                            | BinOp::BitOr
                            | BinOp::BitXor
                            | BinOp::Shl
                            | BinOp::Shr
                    );
                    // Div/Mod by a zero literal must keep erroring at
                    // runtime through the normal path semantics — the
                    // fused helper preserves that, so no exclusion needed.
                    if fusable {
                        self.compile_expr(left)?;
                        let k = *k;
                        let idx = self.add_constant_gc(|gc| Value::from_int(gc, k));
                        match op {
                            BinOp::Eq => {
                                self.emit_op(Op::EqConst, span.line);
                                self.emit_u16(idx, span.line);
                            }
                            BinOp::Ne => {
                                self.emit_op(Op::NeqConst, span.line);
                                self.emit_u16(idx, span.line);
                            }
                            _ => {
                                let arith = match op {
                                    BinOp::Add => Op::Add,
                                    BinOp::Sub => Op::Sub,
                                    BinOp::Mul => Op::Mul,
                                    BinOp::Div => Op::Div,
                                    BinOp::Mod => Op::Mod,
                                    BinOp::BitAnd => Op::BitAnd,
                                    BinOp::BitOr => Op::BitOr,
                                    BinOp::BitXor => Op::BitXor,
                                    BinOp::Shl => Op::Shl,
                                    BinOp::Shr => Op::Shr,
                                    _ => unreachable!(),
                                };
                                self.emit_op(Op::ConstArith, span.line);
                                self.emit_u16(idx, span.line);
                                self.emit_byte(arith as u8, span.line);
                            }
                        }
                        return Ok(());
                    }
                }
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                let bytecode_op = match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Mod => Op::Mod,
                    BinOp::Eq => Op::Eq,
                    BinOp::Ne => Op::Neq,
                    BinOp::Lt => Op::Lt,
                    BinOp::Le => Op::Lte,
                    BinOp::Gt => Op::Gt,
                    BinOp::Ge => Op::Gte,
                    BinOp::BitAnd => Op::BitAnd,
                    BinOp::BitOr => Op::BitOr,
                    BinOp::BitXor => Op::BitXor,
                    BinOp::Shl => Op::Shl,
                    BinOp::Shr => Op::Shr,
                    BinOp::And | BinOp::Or | BinOp::Is => unreachable!(),
                };
                self.emit_op(bytecode_op, span.line);
            }
            Expr::Unary(op, operand, span) => {
                if let Some(folded) = Self::try_const_fold_unary(&mut self.gc, op, operand) {
                    self.emit_constant(folded, span.line);
                    return Ok(());
                }
                self.compile_expr(operand)?;
                match op {
                    UnaryOp::Neg => self.emit_op(Op::Neg, span.line),
                    UnaryOp::Not => self.emit_op(Op::Not, span.line),
                    UnaryOp::BitNot => self.emit_op(Op::BitNot, span.line),
                }
            }
            other => unreachable!(
                "compile_operator_expr received {other:?}, which its dispatcher never routes here"
            ),
        }
        Ok(())
    }
}
