//! Deterministic, linear-time expression simplification.
//!
//! The pass is intentionally closed over a side-effect-free AST subset. It
//! never saturates a search graph and never drops evaluation of calls, async
//! work, queries, allocations, or other potentially effectful expressions.

use crate::ast::*;

pub(crate) fn optimize_system_block(block: &Block) -> Block {
    optimize_block(block)
}

pub(crate) fn optimize_ecs_function_block(block: &Block) -> Block {
    optimize_block(block)
}

pub(crate) fn optimize_block(block: &Block) -> Block {
    Block {
        id: block.id,
        span: block.span.clone(),
        stmts: block.stmts.iter().map(optimize_stmt).collect(),
    }
}

fn optimize_stmt(stmt: &Stmt) -> Stmt {
    match stmt {
        Stmt::Let(l) => Stmt::Let(LetStmt {
            id: l.id,
            span: l.span.clone(),
            names: l.names.clone(),
            tuple_destructure: l.tuple_destructure,
            mutable: l.mutable,
            recursive: l.recursive,
            is_unique: l.is_unique,
            is_pub: l.is_pub,
            type_annotation: l.type_annotation.clone(),
            value: optimize_expr(&l.value),
        }),
        Stmt::LetElse(le) => Stmt::LetElse(LetElseStmt {
            id: le.id,
            span: le.span.clone(),
            mutable: le.mutable,
            type_annotation: le.type_annotation.clone(),
            variant_name: le.variant_name.clone(),
            bindings: le.bindings.clone(),
            pattern_bindings: le.pattern_bindings.clone(),
            has_rest: le.has_rest,
            subject: optimize_expr(&le.subject),
            else_block: optimize_block(&le.else_block),
        }),
        Stmt::Assign(a) => Stmt::Assign(optimize_assign_stmt(a)),
        Stmt::If(i) => Stmt::If(IfStmt {
            id: i.id,
            span: i.span.clone(),
            condition: optimize_expr(&i.condition),
            then_block: optimize_block(&i.then_block),
            else_block: i.else_block.as_ref().map(optimize_block),
        }),
        Stmt::While(w) => Stmt::While(WhileStmt {
            id: w.id,
            span: w.span.clone(),
            condition: optimize_expr(&w.condition),
            body: optimize_block(&w.body),
        }),
        Stmt::For(f) => Stmt::For(ForStmt {
            id: f.id,
            span: f.span.clone(),
            bindings: f.bindings.clone(),
            destructure_bindings: f.destructure_bindings.clone(),
            iterable: optimize_expr(&f.iterable),
            body: optimize_block(&f.body),
        }),
        Stmt::Return(r) => Stmt::Return(ReturnStmt {
            id: r.id,
            span: r.span.clone(),
            value: r.value.as_ref().map(optimize_expr),
        }),
        Stmt::Break(b) => Stmt::Break(BreakStmt {
            id: b.id,
            span: b.span.clone(),
        }),
        Stmt::Continue(c) => Stmt::Continue(ContinueStmt {
            id: c.id,
            span: c.span.clone(),
        }),
        Stmt::Emit(e) => Stmt::Emit(EmitStmt {
            id: e.id,
            span: e.span.clone(),
            event_name: e.event_name.clone(),
            fields: e
                .fields
                .iter()
                .map(|(name, expr)| (name.clone(), optimize_expr(expr)))
                .collect(),
            delay: e.delay.as_ref().map(optimize_expr),
            delivery: e.delivery.clone(),
        }),
        Stmt::Schedule(s) => Stmt::Schedule(ScheduleStmt {
            id: s.id,
            span: s.span.clone(),
            systems: s.systems.clone(),
            serial: s.serial,
        }),
        Stmt::Update(u) => Stmt::Update(UpdateStmt {
            id: u.id,
            span: u.span.clone(),
            entity_expr: u.entity_expr.as_ref().map(optimize_expr),
            comp_name: u.comp_name.clone(),
            field_updates: u
                .field_updates
                .iter()
                .map(|fu| FieldUpdate {
                    name: fu.name.clone(),
                    index: fu.index.as_ref().map(optimize_expr),
                    value: optimize_expr(&fu.value),
                })
                .collect(),
        }),
        Stmt::Transaction(t) => Stmt::Transaction(TransactionStmt {
            id: t.id,
            span: t.span.clone(),
            name: t.name.clone(),
            changes_only: t.changes_only.clone(),
            requires: t.requires.iter().map(optimize_expr).collect(),
            body: optimize_block(&t.body),
            ensures: t.ensures.iter().map(optimize_expr).collect(),
            post_commit: t.post_commit.as_ref().map(optimize_block),
        }),
        Stmt::Settle(s) => Stmt::Settle(SettleStmt {
            id: s.id,
            span: s.span.clone(),
            body: optimize_block(&s.body),
        }),
        Stmt::Propose(s) => Stmt::Propose(ProposeStmt {
            id: s.id,
            span: s.span.clone(),
            intent_name: s.intent_name.clone(),
            fields: s
                .fields
                .iter()
                .map(|(name, expr)| (name.clone(), optimize_expr(expr)))
                .collect(),
        }),
        Stmt::Next(s) => Stmt::Next(NextStmt {
            id: s.id,
            span: s.span.clone(),
            entity: optimize_expr(&s.entity),
            component_name: s.component_name.clone(),
            fields: s
                .fields
                .iter()
                .map(|(name, expr)| (name.clone(), optimize_expr(expr)))
                .collect(),
        }),
        Stmt::Require(s) => Stmt::Require(RequireStmt {
            id: s.id,
            span: s.span.clone(),
            condition: optimize_expr(&s.condition),
            code: s.code.clone(),
        }),
        Stmt::Match(m) => Stmt::Match(optimize_match_stmt(m)),
        Stmt::Expr(e) => Stmt::Expr(ExprStmt {
            id: e.id,
            span: e.span.clone(),
            expr: optimize_expr(&e.expr),
        }),
        Stmt::OnceGuardPass(span) => Stmt::OnceGuardPass(span.clone()),
        Stmt::Error(span) => Stmt::Error(span.clone()),
    }
}

fn optimize_assign_stmt(assign: &AssignStmt) -> AssignStmt {
    AssignStmt {
        id: assign.id,
        span: assign.span.clone(),
        target: optimize_expr(&assign.target),
        value: optimize_expr(&assign.value),
    }
}

fn optimize_match_stmt(stmt: &MatchStmt) -> MatchStmt {
    MatchStmt {
        id: stmt.id,
        span: stmt.span.clone(),
        subject: optimize_expr(&stmt.subject),
        cases: stmt.cases.iter().map(optimize_match_case).collect(),
    }
}

fn optimize_match_case(case: &MatchCase) -> MatchCase {
    MatchCase {
        id: case.id,
        span: case.span.clone(),
        pattern: optimize_pattern(&case.pattern),
        guard: case.guard.as_ref().map(optimize_expr),
        body: optimize_block(&case.body),
    }
}

fn optimize_pattern(pattern: &Pattern) -> Pattern {
    match pattern {
        Pattern::Wildcard => Pattern::Wildcard,
        Pattern::Literal(expr) => Pattern::Literal(optimize_expr(expr)),
        Pattern::Variant {
            path,
            bindings,
            pattern_bindings,
            has_rest,
            is_bare_variant,
        } => Pattern::Variant {
            path: path.clone(),
            bindings: bindings.clone(),
            pattern_bindings: pattern_bindings.clone(),
            has_rest: *has_rest,
            is_bare_variant: *is_bare_variant,
        },
        Pattern::HasComponent { component, binding } => Pattern::HasComponent {
            component: component.clone(),
            binding: binding.clone(),
        },
    }
}

pub(crate) fn optimize_expr(expr: &Expr) -> Expr {
    let transformed = match expr {
        Expr::IntLit(_, _)
        | Expr::BoolLit(_, _)
        | Expr::NilLit(_)
        | Expr::StrLit(_, _)
        | Expr::StateRef(_, _, _)
        | Expr::SystemRef(_, _)
        | Expr::Error(_)
        | Expr::Ident(_, _) => expr.clone(),
        Expr::FloatLit(_, _) => expr.clone(),
        Expr::ListLit(items, span) => {
            Expr::ListLit(items.iter().map(optimize_expr).collect(), span.clone())
        }
        Expr::MapLit(entries, span) => Expr::MapLit(
            entries
                .iter()
                .map(|(key, value)| (optimize_expr(key), optimize_expr(value)))
                .collect(),
            span.clone(),
        ),
        Expr::TupleLit(items, span) => {
            Expr::TupleLit(items.iter().map(optimize_expr).collect(), span.clone())
        }
        Expr::FStringExpr(parts, span) => Expr::FStringExpr(
            parts
                .iter()
                .map(|part| match part {
                    FStringPart::Lit(text) => FStringPart::Lit(text.clone()),
                    FStringPart::Expr(expr, suffix) => {
                        FStringPart::Expr(Box::new(optimize_expr(expr)), suffix.clone())
                    }
                })
                .collect(),
            span.clone(),
        ),
        Expr::Binary(left, op, right, span) => {
            let left = optimize_expr(left);
            let right = optimize_expr(right);
            Expr::Binary(Box::new(left), *op, Box::new(right), span.clone())
        }
        Expr::Unary(op, inner, span) => {
            let inner = optimize_expr(inner);
            Expr::Unary(*op, Box::new(inner), span.clone())
        }
        Expr::Pipe(left, right, span) => {
            let left = optimize_expr(left);
            let right = optimize_expr(right);
            Expr::Pipe(Box::new(left), Box::new(right), span.clone())
        }
        Expr::Call(callee, args, span) => Expr::Call(
            Box::new(optimize_expr(callee)),
            args.iter().map(optimize_expr).collect(),
            span.clone(),
        ),
        Expr::Field(inner, field, span) => {
            let inner = optimize_expr(inner);
            Expr::Field(Box::new(inner), field.clone(), span.clone())
        }
        Expr::Index(inner, idx, span) => {
            let inner = optimize_expr(inner);
            let idx = optimize_expr(idx);
            Expr::Index(Box::new(inner), Box::new(idx), span.clone())
        }
        Expr::ComponentExpr(name, fields, spread, span) => Expr::ComponentExpr(
            name.clone(),
            fields
                .iter()
                .map(|(field_name, expr)| (field_name.clone(), optimize_expr(expr)))
                .collect(),
            spread.as_ref().map(|expr| Box::new(optimize_expr(expr))),
            span.clone(),
        ),
        Expr::VariantExpr(name, variant, fields, span) => Expr::VariantExpr(
            name.clone(),
            variant.clone(),
            fields
                .iter()
                .map(|(field_name, expr)| (field_name.clone(), optimize_expr(expr)))
                .collect(),
            span.clone(),
        ),
        Expr::MatchExpr(m, span) => Expr::MatchExpr(Box::new(optimize_match_stmt(m)), span.clone()),
        Expr::IfExpr(c, t, e, span) => Expr::IfExpr(
            Box::new(optimize_expr(c)),
            Box::new(optimize_expr(t)),
            Box::new(optimize_expr(e)),
            span.clone(),
        ),
        Expr::FnExpr(params, param_muts, param_tys, param_defaults, ret, body, span) => {
            Expr::FnExpr(
                params.clone(),
                param_muts.clone(),
                param_tys.clone(),
                param_defaults.clone(),
                ret.clone(),
                optimize_block(body),
                span.clone(),
            )
        }
        Expr::QueryExpr(q, span) => Expr::QueryExpr(
            QueryExprNode {
                components: q.components.clone(),
                filter: q.filter.as_ref().map(|expr| Box::new(optimize_expr(expr))),
                select: q.select.clone(),
            },
            span.clone(),
        ),
        Expr::Await(inner, span) => Expr::Await(Box::new(optimize_expr(inner)), span.clone()),
        Expr::AsyncCall(callee, args, span) => Expr::AsyncCall(
            Box::new(optimize_expr(callee)),
            args.iter().map(optimize_expr).collect(),
            span.clone(),
        ),
        Expr::Try(inner, span) => Expr::Try(Box::new(optimize_expr(inner)), span.clone()),
        Expr::Spread(inner, span) => Expr::Spread(Box::new(optimize_expr(inner)), span.clone()),
        Expr::EntityLiteral(name, components, span) => Expr::EntityLiteral(
            name.as_ref().map(|expr| Box::new(optimize_expr(expr))),
            components
                .iter()
                .map(|entry| match entry {
                    ComponentEntry::Init(ci) => ComponentEntry::Init(ComponentInit {
                        id: ci.id,
                        span: ci.span.clone(),
                        comp_name: ci.comp_name.clone(),
                        fields: ci
                            .fields
                            .iter()
                            .map(|(name, expr)| (name.clone(), optimize_expr(expr)))
                            .collect(),
                    }),
                    ComponentEntry::Expr(expr) => ComponentEntry::Expr(optimize_expr(expr)),
                })
                .collect(),
            span.clone(),
        ),
    };

    optimize_value_expr(&transformed).unwrap_or(transformed)
}

fn optimize_value_expr(expr: &Expr) -> Option<Expr> {
    let span = expr.span().clone();
    match expr {
        Expr::Unary(UnaryOp::Neg, inner, _)
            if matches!(inner.as_ref(), Expr::Unary(UnaryOp::Neg, _, _)) =>
        {
            let Expr::Unary(_, nested, _) = inner.as_ref() else {
                unreachable!()
            };
            Some((**nested).clone())
        }
        Expr::Unary(UnaryOp::Not, inner, _)
            if matches!(inner.as_ref(), Expr::Unary(UnaryOp::Not, _, _)) =>
        {
            let Expr::Unary(_, nested, _) = inner.as_ref() else {
                unreachable!()
            };
            Some((**nested).clone())
        }
        Expr::Binary(left, BinOp::Add, right, _) if is_int(left, 0) => Some((**right).clone()),
        Expr::Binary(left, BinOp::Add, right, _) if is_int(right, 0) => Some((**left).clone()),
        Expr::Binary(left, BinOp::Add, right, _) if same_rewrite_value(left, right) => {
            Some(binary_expr(
                Expr::IntLit(2, span.clone()),
                BinOp::Mul,
                (**left).clone(),
                span,
            ))
        }
        Expr::Binary(left, BinOp::Add, right, _) => factor_common(left, right, &span),
        Expr::Binary(left, BinOp::Mul, right, _) if is_int(left, 1) => Some((**right).clone()),
        Expr::Binary(left, BinOp::Mul, right, _) if is_int(right, 1) => Some((**left).clone()),
        Expr::Binary(left, BinOp::Mul, right, _) if is_int(left, 0) && rewrite_safe(right) => {
            Some(Expr::IntLit(0, span))
        }
        Expr::Binary(left, BinOp::Mul, right, _) if is_int(right, 0) && rewrite_safe(left) => {
            Some(Expr::IntLit(0, span))
        }
        Expr::Binary(left, BinOp::Sub, right, _) if is_int(right, 0) => Some((**left).clone()),
        Expr::Binary(left, BinOp::Sub, right, _) if same_rewrite_value(left, right) => {
            Some(Expr::IntLit(0, span))
        }
        Expr::Binary(left, BinOp::Div, right, _) if is_int(right, 1) => Some((**left).clone()),
        Expr::Binary(left, BinOp::And, right, _) if is_bool(left, true) => Some((**right).clone()),
        Expr::Binary(left, BinOp::And, right, _) if is_bool(right, true) => Some((**left).clone()),
        Expr::Binary(left, BinOp::And, _, _) if is_bool(left, false) => {
            Some(Expr::BoolLit(false, span))
        }
        Expr::Binary(left, BinOp::And, right, _) if is_bool(right, false) && rewrite_safe(left) => {
            Some(Expr::BoolLit(false, span))
        }
        Expr::Binary(left, BinOp::Or, right, _) if is_bool(left, false) => Some((**right).clone()),
        Expr::Binary(left, BinOp::Or, right, _) if is_bool(right, false) => Some((**left).clone()),
        Expr::Binary(left, BinOp::Or, _, _) if is_bool(left, true) => {
            Some(Expr::BoolLit(true, span))
        }
        Expr::Binary(left, BinOp::Or, right, _) if is_bool(right, true) && rewrite_safe(left) => {
            Some(Expr::BoolLit(true, span))
        }
        _ => None,
    }
}

fn is_int(expr: &Expr, expected: i64) -> bool {
    matches!(expr, Expr::IntLit(value, _) if *value == expected)
}

fn is_bool(expr: &Expr, expected: bool) -> bool {
    matches!(expr, Expr::BoolLit(value, _) if *value == expected)
}

/// Whether eliminating or coalescing evaluation of this expression is safe.
///
/// This deliberately matches the old optimizer's closed algebraic subset.
/// Calls, allocations, async work, queries, and all other effectful or
/// potentially effectful expressions remain opaque.
fn rewrite_safe(expr: &Expr) -> bool {
    match expr {
        Expr::IntLit(_, _) | Expr::BoolLit(_, _) | Expr::NilLit(_) | Expr::Ident(_, _) => true,
        Expr::Unary(UnaryOp::Neg | UnaryOp::Not, inner, _) => rewrite_safe(inner),
        Expr::Unary(UnaryOp::BitNot, _, _) => false,
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
                    | BinOp::And
                    | BinOp::Or
            ) && rewrite_safe(left)
                && rewrite_safe(right)
        }
        Expr::Field(inner, _, _) => rewrite_safe(inner),
        Expr::Index(inner, index, _) => rewrite_safe(inner) && rewrite_safe(index),
        _ => false,
    }
}

fn same_rewrite_value(left: &Expr, right: &Expr) -> bool {
    if !rewrite_safe(left) || !rewrite_safe(right) {
        return false;
    }
    match (left, right) {
        (Expr::IntLit(a, _), Expr::IntLit(b, _)) => a == b,
        (Expr::BoolLit(a, _), Expr::BoolLit(b, _)) => a == b,
        (Expr::NilLit(_), Expr::NilLit(_)) => true,
        (Expr::Ident(a, _), Expr::Ident(b, _)) => a == b,
        (Expr::Unary(a_op, a, _), Expr::Unary(b_op, b, _)) => {
            a_op == b_op && same_rewrite_value(a, b)
        }
        (Expr::Binary(a_left, a_op, a_right, _), Expr::Binary(b_left, b_op, b_right, _)) => {
            a_op == b_op
                && same_rewrite_value(a_left, b_left)
                && same_rewrite_value(a_right, b_right)
        }
        (Expr::Field(a, a_field, _), Expr::Field(b, b_field, _)) => {
            a_field == b_field && same_rewrite_value(a, b)
        }
        (Expr::Index(a_value, a_index, _), Expr::Index(b_value, b_index, _)) => {
            same_rewrite_value(a_value, b_value) && same_rewrite_value(a_index, b_index)
        }
        _ => false,
    }
}

fn factor_common(left: &Expr, right: &Expr, span: &Span) -> Option<Expr> {
    let Expr::Binary(left_a, BinOp::Mul, left_b, _) = left else {
        return None;
    };
    let Expr::Binary(right_a, BinOp::Mul, right_b, _) = right else {
        return None;
    };
    if ![
        left_a.as_ref(),
        left_b.as_ref(),
        right_a.as_ref(),
        right_b.as_ref(),
    ]
    .into_iter()
    .all(rewrite_safe)
    {
        return None;
    }

    let candidates = [
        (
            left_a.as_ref(),
            left_b.as_ref(),
            right_a.as_ref(),
            right_b.as_ref(),
        ),
        (
            left_a.as_ref(),
            left_b.as_ref(),
            right_b.as_ref(),
            right_a.as_ref(),
        ),
        (
            left_b.as_ref(),
            left_a.as_ref(),
            right_a.as_ref(),
            right_b.as_ref(),
        ),
        (
            left_b.as_ref(),
            left_a.as_ref(),
            right_b.as_ref(),
            right_a.as_ref(),
        ),
    ];
    let (common, left_remainder, _, right_remainder) = candidates
        .into_iter()
        .find(|(left_common, _, right_common, _)| same_rewrite_value(left_common, right_common))?;
    let sum = binary_expr(
        left_remainder.clone(),
        BinOp::Add,
        right_remainder.clone(),
        span.clone(),
    );
    Some(binary_expr(common.clone(), BinOp::Mul, sum, span.clone()))
}

fn binary_expr(left: Expr, op: BinOp, right: Expr, span: Span) -> Expr {
    Expr::Binary(Box::new(left), op, Box::new(right), span)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span::default()
    }

    fn ident(name: &str) -> Expr {
        Expr::Ident(name.to_string(), span())
    }

    #[test]
    fn simplifies_arithmetic_identity() {
        let expr = Expr::Binary(
            Box::new(Expr::Binary(
                Box::new(ident("x")),
                BinOp::Add,
                Box::new(Expr::IntLit(0, span())),
                span(),
            )),
            BinOp::Mul,
            Box::new(Expr::IntLit(1, span())),
            span(),
        );

        let optimized = optimize_expr(&expr);
        match optimized {
            Expr::Ident(name, _) => assert_eq!(name, "x"),
            other => panic!("expected identifier, got {:?}", other),
        }
    }

    #[test]
    fn optimizes_field_access_through_logical_load() {
        let expr = Expr::Binary(
            Box::new(Expr::Field(
                Box::new(ident("player")),
                "health".to_string(),
                span(),
            )),
            BinOp::Add,
            Box::new(Expr::IntLit(0, span())),
            span(),
        );

        let optimized = optimize_expr(&expr);
        match optimized {
            Expr::Field(inner, field, _) => {
                assert_eq!(field, "health");
                match *inner {
                    Expr::Ident(name, _) => assert_eq!(name, "player"),
                    other => panic!("expected player identifier, got {:?}", other),
                }
            }
            other => panic!("expected field access, got {:?}", other),
        }
    }

    #[test]
    fn optimizes_assignment_value_and_location() {
        let stmt = AssignStmt {
            id: NodeId(1),
            span: span(),
            target: Expr::Field(Box::new(ident("player")), "score".to_string(), span()),
            value: Expr::Binary(
                Box::new(ident("x")),
                BinOp::Add,
                Box::new(Expr::IntLit(0, span())),
                span(),
            ),
        };

        let optimized = optimize_assign_stmt(&stmt);
        match optimized.target {
            Expr::Field(inner, field, _) => {
                assert_eq!(field, "score");
                match *inner {
                    Expr::Ident(name, _) => assert_eq!(name, "player"),
                    other => panic!("expected player identifier, got {:?}", other),
                }
            }
            other => panic!("expected field target, got {:?}", other),
        }
        match optimized.value {
            Expr::Ident(name, _) => assert_eq!(name, "x"),
            other => panic!("expected simplified value, got {:?}", other),
        }
    }

    #[test]
    fn never_drops_effectful_operands() {
        let call = Expr::Call(Box::new(ident("observe")), Vec::new(), span());
        let expr = Expr::Binary(
            Box::new(call),
            BinOp::Mul,
            Box::new(Expr::IntLit(0, span())),
            span(),
        );

        assert!(matches!(
            optimize_expr(&expr),
            Expr::Binary(_, BinOp::Mul, _, _)
        ));
    }

    #[test]
    fn factors_a_shared_pure_term() {
        let product = |left: &str, right: &str| {
            Expr::Binary(
                Box::new(ident(left)),
                BinOp::Mul,
                Box::new(ident(right)),
                span(),
            )
        };
        let expr = Expr::Binary(
            Box::new(product("rate", "base")),
            BinOp::Add,
            Box::new(product("bonus", "rate")),
            span(),
        );

        let optimized = optimize_expr(&expr);
        assert!(matches!(optimized, Expr::Binary(_, BinOp::Mul, _, _)));
    }
}
