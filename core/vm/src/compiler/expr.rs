use super::*;
use crate::gc::GcHeap;
use crate::value::{Builtin, PipelineOp};

pub enum VectorizableBody<'a> {
    Expr(&'a Expr),
    IfElse {
        cond: &'a Expr,
        then_expr: &'a Expr,
        else_expr: &'a Expr,
    },
}

impl Compiler {
    // A float-declared component/resource field must store a float even when
    // the supplied value's runtime tag is int. `session_emit` decodes event
    // payloads from JSON, where a whole number (`y: 0`) cannot encode
    // float-ness and arrives as an int; written verbatim it leaves a field like
    // `Position.y` holding an int. Strict float readers (the render buffer's
    // `component_float_field`) then silently drop the whole component (the local
    // champion vanishes on the first authoritative correction), and a client
    // snapshot (`0`) diverges from the authority's (`0.0`). Wrapping the value
    // in the `float` builtin coerces int -> float losslessly and is a no-op on
    // values already float, keeping component fields type-stable at every
    // construction/update site.
    fn compile_component_field(
        &mut self,
        comp_name: &str,
        field_name: &str,
        value: &Expr,
        span: &Span,
    ) -> Result<(), CompileError> {
        if self.field_declared_float(comp_name, field_name) {
            let coerced = Expr::Call(
                Box::new(Expr::Ident("float".to_string(), span.clone())),
                vec![value.clone()],
                span.clone(),
            );
            return self.compile_expr(&coerced);
        }
        self.compile_expr(value)
    }

    fn field_declared_float(&self, comp_name: &str, field_name: &str) -> bool {
        let is_float = |fields: &Vec<(String, Option<crate::ast::TypeExpr>, Expr)>| {
            fields
                .iter()
                .find(|(n, _, _)| n == field_name)
                .and_then(|(_, ty, _)| ty.as_ref())
                .map(|ty| matches!(ty, crate::ast::TypeExpr::Named(n) if n == "float"))
                .unwrap_or(false)
        };
        self.component_types
            .get(comp_name)
            .map(is_float)
            .unwrap_or(false)
            || self
                .resource_types
                .get(comp_name)
                .map(is_float)
                .unwrap_or(false)
    }

    fn try_const_fold(
        gc: &mut GcHeap,
        left: &Expr,
        op: &BinOp,
        right: &Expr,
        span: &Span,
    ) -> Result<Option<Value>, CompileError> {
        match (left, op, right) {
            (Expr::IntLit(a, _), BinOp::Add, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_int(gc, a + b)))
            }
            (Expr::IntLit(a, _), BinOp::Sub, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_int(gc, a - b)))
            }
            (Expr::IntLit(a, _), BinOp::Mul, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_int(gc, a * b)))
            }
            (Expr::IntLit(a, _), BinOp::Mod, Expr::IntLit(b, _)) => {
                if *b == 0 {
                    return Err(CompileError {
                        message: "Division by zero at compile time".to_string(),
                        line: span.line,
                        col: span.col,
                    });
                }
                Ok(Some(Value::from_int(gc, a % b)))
            }
            (Expr::IntLit(a, _), BinOp::Div, Expr::IntLit(b, _)) => {
                if *b == 0 {
                    return Err(CompileError {
                        message: "Division by zero at compile time".to_string(),
                        line: span.line,
                        col: span.col,
                    });
                }
                Ok(Some(Value::from_int(gc, a / b)))
            }
            (Expr::FloatLit(a, _), BinOp::Add, Expr::FloatLit(b, _)) => {
                Ok(Some(Value::from_float(a + b)))
            }
            (Expr::FloatLit(a, _), BinOp::Sub, Expr::FloatLit(b, _)) => {
                Ok(Some(Value::from_float(a - b)))
            }
            (Expr::FloatLit(a, _), BinOp::Mul, Expr::FloatLit(b, _)) => {
                Ok(Some(Value::from_float(a * b)))
            }
            (Expr::FloatLit(a, _), BinOp::Div, Expr::FloatLit(b, _)) => {
                if *b == 0.0 {
                    return Err(CompileError {
                        message: "Division by zero at compile time".to_string(),
                        line: span.line,
                        col: span.col,
                    });
                }
                Ok(Some(Value::from_float(a / b)))
            }
            (Expr::IntLit(a, _), BinOp::BitAnd, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_int(gc, a & b)))
            }
            (Expr::IntLit(a, _), BinOp::BitOr, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_int(gc, a | b)))
            }
            (Expr::IntLit(a, _), BinOp::BitXor, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_int(gc, a ^ b)))
            }
            // Shift folding mirrors the shl()/shr() builtins (and Op::Shl/Shr):
            // logical shifts, out-of-range count yields 0.
            (Expr::IntLit(a, _), BinOp::Shl, Expr::IntLit(b, _)) => {
                let v = if !(0..64).contains(b) {
                    0
                } else {
                    ((*a as u64) << (*b as u32)) as i64
                };
                Ok(Some(Value::from_int(gc, v)))
            }
            (Expr::IntLit(a, _), BinOp::Shr, Expr::IntLit(b, _)) => {
                let v = if !(0..64).contains(b) {
                    0
                } else {
                    ((*a as u64) >> (*b as u32)) as i64
                };
                Ok(Some(Value::from_int(gc, v)))
            }
            (Expr::IntLit(a, _), BinOp::Eq, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_bool(a == b)))
            }
            (Expr::IntLit(a, _), BinOp::Ne, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_bool(a != b)))
            }
            (Expr::IntLit(a, _), BinOp::Lt, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_bool(a < b)))
            }
            (Expr::IntLit(a, _), BinOp::Le, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_bool(a <= b)))
            }
            (Expr::IntLit(a, _), BinOp::Gt, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_bool(a > b)))
            }
            (Expr::IntLit(a, _), BinOp::Ge, Expr::IntLit(b, _)) => {
                Ok(Some(Value::from_bool(a >= b)))
            }
            (Expr::StrLit(a, _), BinOp::Add, Expr::StrLit(b, _)) => {
                Ok(Some(Value::from_string(gc, format!("{}{}", a, b))))
            }
            (Expr::StrLit(a, _), BinOp::Mul, Expr::IntLit(b, _)) if *b >= 0 => {
                Ok(Some(Value::from_string(gc, a.repeat(*b as usize))))
            }
            (Expr::IntLit(a, _), BinOp::Mul, Expr::StrLit(b, _)) if *a >= 0 => {
                Ok(Some(Value::from_string(gc, b.repeat(*a as usize))))
            }
            (Expr::BoolLit(a, _), BinOp::Eq, Expr::BoolLit(b, _)) => {
                Ok(Some(Value::from_bool(a == b)))
            }
            (Expr::BoolLit(a, _), BinOp::Ne, Expr::BoolLit(b, _)) => {
                Ok(Some(Value::from_bool(a != b)))
            }
            (Expr::Unary(UnaryOp::Neg, inner, _), _, _) => {
                if let Expr::IntLit(a, s) = inner.as_ref() {
                    Self::try_const_fold(gc, &Expr::IntLit(-a, s.clone()), op, right, span)
                } else {
                    Ok(None)
                }
            }
            (_, _, Expr::Unary(UnaryOp::Neg, inner, _)) => {
                if let Expr::IntLit(b, s) = inner.as_ref() {
                    Self::try_const_fold(gc, left, op, &Expr::IntLit(-b, s.clone()), span)
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    /// Flatten the left spine of a `+` chain: `((a+b)+c)+d` -> `[a,b,c,d]`.
    /// Parenthesized right-nested chains stay as single operands â€” fusion is
    /// conservative, never reassociating across explicit grouping.
    fn flatten_add_chain<'a>(left: &'a Expr, right: &'a Expr, out: &mut Vec<&'a Expr>) {
        if let Expr::Binary(l2, BinOp::Add, r2, _) = left {
            Self::flatten_add_chain(l2, r2, out);
        } else {
            out.push(left);
        }
        out.push(right);
    }

    fn try_const_fold_unary(gc: &mut GcHeap, op: &UnaryOp, operand: &Expr) -> Option<Value> {
        match (op, operand) {
            (UnaryOp::Neg, Expr::IntLit(n, _)) => Some(Value::from_int(gc, -n)),
            (UnaryOp::Neg, Expr::FloatLit(f, _)) => Some(Value::from_float(-f)),
            (UnaryOp::Not, Expr::BoolLit(b, _)) => Some(Value::from_bool(!b)),
            (UnaryOp::BitNot, Expr::IntLit(n, _)) => Some(Value::from_int(gc, !n)),
            _ => None,
        }
    }

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
                    // re-copying the growing prefix â€” O(partsÂ²) bytes moved
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
                // growing prefix â€” O(chainÂ²) bytes moved. A chain containing
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
                // (15% of all dispatches in bitboard workloads â€” every
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
                    // runtime through the normal path semantics â€” the
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
            Expr::Call(callee, args, span) => {
                if self.release && args.len() == 1 {
                    let mut is_debug_trace = false;
                    if let Expr::Ident(name, _) = callee.as_ref() {
                        let is_local = self.resolve_local(name).is_some() || {
                            let fn_idx = self.functions.len() - 1;
                            fn_idx > 0 && self.resolve_upvalue(fn_idx, name).is_some()
                        };
                        if !is_local {
                            if let Some(builtin) = Builtin::ALL.iter().find(|b| b.name() == name) {
                                if matches!(builtin, Builtin::DebugTrace) {
                                    is_debug_trace = true;
                                }
                            }
                        }
                    }
                    if is_debug_trace {
                        return self.compile_expr(&args[0]);
                    }
                }
                let resolved_system_name: Option<String> =
                    if let Expr::Field(obj, member, _) = callee.as_ref() {
                        if let Expr::Ident(alias, _) = obj.as_ref() {
                            self.resolve_alias_member(alias, member)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                let system_check_name: Option<&str> = if let Expr::Ident(name, _) = callee.as_ref()
                {
                    Some(name.as_str())
                } else {
                    resolved_system_name.as_deref()
                };
                if let Some(name) = system_check_name {
                    if self.is_system(name) {
                        if !args.is_empty() {
                            return Err(CompileError {
                                message: format!(
                                    "System '{}' takes no explicit arguments â€” its parameters come from the ECS world. Use `{}()` with no args.",
                                    name, name
                                ),
                                line: span.line,
                                col: span.col,
                            });
                        }
                        let name_idx =
                            self.add_constant_gc(|gc| Value::from_string(gc, name.to_string()));
                        self.emit_op(Op::RunSystem, span.line);
                        self.emit_u16(name_idx, span.line);
                        self.emit_constant(Value::NIL, span.line);
                        return Ok(());
                    }
                }
                let mut total_args = 0;
                for arg in args {
                    if let Expr::Spread(inner, s_span) = arg {
                        self.compile_expr(inner)?;
                        self.emit_op(Op::Unpack, span.line);
                        let len = self.spread_lengths.get(s_span).copied().unwrap_or(1);
                        total_args += len;
                    } else {
                        self.compile_expr(arg)?;
                        total_args += 1;
                    }
                }
                self.compile_expr(callee)?;
                self.emit_op(Op::Call, span.line);
                self.emit_byte(total_args as u8, span.line);
            }
            Expr::Await(inner, _) => {
                self.compile_expr(inner)?;
                self.emit_op(Op::Await, inner.span().line);
            }
            Expr::Try(inner, span) => {
                self.compile_expr(inner)?;
                self.emit_op(Op::Try, span.line);
            }
            Expr::AsyncCall(callee, args, span) => {
                let mut total_args = 0;
                for arg in args {
                    if let Expr::Spread(inner, s_span) = arg {
                        self.compile_expr(inner)?;
                        self.emit_op(Op::Unpack, span.line);
                        let len = self.spread_lengths.get(s_span).copied().unwrap_or(1);
                        total_args += len;
                    } else {
                        self.compile_expr(arg)?;
                        total_args += 1;
                    }
                }
                self.compile_expr(callee)?;
                self.emit_op(Op::AsyncCall, span.line);
                self.emit_byte(total_args as u8, span.line);
            }
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
            Expr::ComponentExpr(name, fields, rest, span) => {
                let name = &self.resolve_canonical_name(name);
                let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, name.clone()));
                // Declared defaults fill omitted fields; resources keep
                // theirs in a separate table.
                let defaults = self
                    .component_types
                    .get(name)
                    .or_else(|| self.resource_types.get(name))
                    .cloned()
                    .unwrap_or_default();

                if let Some(slot_order) = self.component_field_order(name) {
                    let field_count = slot_order.len();
                    for slot_name in &slot_order {
                        if let Some((_, fexpr)) = fields.iter().find(|(n, _)| n == slot_name) {
                            self.compile_component_field(name, slot_name, fexpr, span)?;
                        } else if let Some(base) = rest {
                            self.compile_expr(base)?;
                            let field_idx = self
                                .add_constant_gc(|gc| Value::from_string(gc, slot_name.clone()));
                            self.emit_op(Op::GetField, span.line);
                            self.emit_u16(field_idx, span.line);
                        } else if let Some((_, _, default_expr)) =
                            defaults.iter().find(|(n, _, _)| n == slot_name)
                        {
                            self.compile_expr(default_expr)?;
                        } else {
                            self.emit_constant(Value::NIL, span.line);
                        }
                    }
                    self.emit_op(Op::MakeCompSlot, span.line);
                    self.emit_u16(type_idx, span.line);
                    self.emit_u16(field_count as u16, span.line);
                } else {
                    let mut all_fields: Vec<(String, Option<&Expr>)> = Vec::new();
                    for (fname, _, fexpr) in &defaults {
                        all_fields.push((fname.clone(), Some(fexpr)));
                    }
                    for (fname, fexpr) in fields {
                        if let Some(existing) = all_fields.iter_mut().find(|(n, _)| n == fname) {
                            existing.1 = Some(fexpr);
                        } else {
                            all_fields.push((fname.clone(), Some(fexpr)));
                        }
                    }

                    let explicit_field_names: std::collections::HashSet<&str> =
                        fields.iter().map(|(n, _)| n.as_str()).collect();

                    let field_count = all_fields.len();
                    for (fname, fexpr) in &all_fields {
                        self.emit_constant_gc(span.line, |gc| {
                            Value::from_string(gc, fname.clone())
                        });
                        if explicit_field_names.contains(fname.as_str()) {
                            if let Some(expr) = fexpr {
                                self.compile_component_field(name, fname, expr, span)?;
                            } else {
                                self.emit_constant(Value::NIL, span.line);
                            }
                        } else if let Some(base) = rest {
                            self.compile_expr(base)?;
                            let field_idx =
                                self.add_constant_gc(|gc| Value::from_string(gc, fname.clone()));
                            self.emit_op(Op::GetField, span.line);
                            self.emit_u16(field_idx, span.line);
                        } else if let Some(expr) = fexpr {
                            self.compile_expr(expr)?;
                        } else {
                            self.emit_constant(Value::NIL, span.line);
                        }
                    }
                    self.emit_op(Op::MakeComp, span.line);
                    self.emit_u16(type_idx, span.line);
                    self.emit_u16(field_count as u16, span.line);
                }
            }
            Expr::StateRef(machine, state, span) => {
                let machine = &self.resolve_canonical_name(machine);
                if self
                    .variant_shorthand
                    .contains(&(machine.clone(), state.clone()))
                {
                    let type_idx =
                        self.add_constant_gc(|gc| Value::from_string(gc, machine.clone()));
                    let variant_idx =
                        self.add_constant_gc(|gc| Value::from_string(gc, state.clone()));
                    self.emit_op(Op::MakeVariant, span.line);
                    self.emit_u16(type_idx, span.line);
                    self.emit_u16(variant_idx, span.line);
                    self.emit_u16(0, span.line);
                } else {
                    let m_idx = self.add_constant_gc(|gc| Value::from_string(gc, machine.clone()));
                    let s_idx = self.add_constant_gc(|gc| Value::from_string(gc, state.clone()));
                    self.emit_op(Op::MakeState, span.line);
                    self.emit_u16(m_idx, span.line);
                    self.emit_u16(s_idx, span.line);
                }
            }
            Expr::VariantExpr(type_name, variant, fields, span) => {
                let type_name = &self.resolve_canonical_name(type_name);
                let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, type_name.clone()));
                let variant_idx =
                    self.add_constant_gc(|gc| Value::from_string(gc, variant.clone()));
                let field_count = fields.len();
                for (fname, fexpr) in fields {
                    self.emit_constant_gc(span.line, |gc| Value::from_string(gc, fname.clone()));
                    self.compile_expr(fexpr)?;
                }
                self.emit_op(Op::MakeVariant, span.line);
                self.emit_u16(type_idx, span.line);
                self.emit_u16(variant_idx, span.line);
                self.emit_u16(field_count as u16, span.line);
            }
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
                            // ("Free", not "Cc::Free") â€” same convention as
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
                            let comp_idx = self
                                .add_constant_gc(|gc| Value::from_string(gc, component.clone()));
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
                        let comp_idx =
                            self.add_constant_gc(|gc| Value::from_string(gc, component.clone()));
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
            Expr::EntityLiteral(name, components, span) => {
                if let Some(name_expr) = name {
                    self.compile_expr(name_expr)?;
                }
                self.compile_component_inits(components, span.line)?;
                let comp_count = components.len() as u8;
                self.emit_op(Op::EcsSpawn, span.line);
                self.emit_byte(comp_count, span.line);
                if name.is_some() {
                    self.emit_byte(1, span.line);
                    self.emit_u16(0, span.line);
                } else {
                    self.emit_byte(0, span.line);
                    let name_idx = self.add_constant_gc(|gc| Value::from_string(gc, String::new()));
                    self.emit_u16(name_idx, span.line);
                }
            }
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
        // operand stack is empty â€” in expression position (a call
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
