

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
    /// Parenthesized right-nested chains stay as single operands — fusion is
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
    }}