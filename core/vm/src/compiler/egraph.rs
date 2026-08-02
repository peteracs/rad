use egg::{Extractor, Id, RecExpr, Rewrite, Runner, Symbol};

use crate::ast::*;

egg::define_language! {
    pub enum RadIr {
        Num(i64),
        Bool(bool),
        Symbol(Symbol),
        FieldName(Symbol),
        "nil" = Nil,
        "+" = Add([Id; 2]),
        "-" = Sub([Id; 2]),
        "*" = Mul([Id; 2]),
        "/" = Div([Id; 2]),
        "%" = Mod([Id; 2]),
        "neg" = Neg(Id),
        "not" = Not(Id),
        "==" = Eq([Id; 2]),
        "!=" = Neq([Id; 2]),
        "<" = Lt([Id; 2]),
        "<=" = Lte([Id; 2]),
        ">" = Gt([Id; 2]),
        ">=" = Gte([Id; 2]),
        "&&" = And([Id; 2]),
        "||" = Or([Id; 2]),
        "logical_load" = LogicalLoad([Id; 2]),
        "logical_store" = LogicalStore([Id; 2]),
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RadCost;

impl egg::CostFunction<RadIr> for RadCost {
    type Cost = usize;

    fn cost<C>(&mut self, enode: &RadIr, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        let base = match enode {
            RadIr::Num(_)
            | RadIr::Bool(_)
            | RadIr::Symbol(_)
            | RadIr::FieldName(_)
            | RadIr::Nil => 1,
            RadIr::Neg(_)
            | RadIr::Not(_)
            | RadIr::Add(_)
            | RadIr::Sub(_)
            | RadIr::Mul(_)
            | RadIr::Div(_)
            | RadIr::Mod(_)
            | RadIr::Eq(_)
            | RadIr::Neq(_)
            | RadIr::Lt(_)
            | RadIr::Lte(_)
            | RadIr::Gt(_)
            | RadIr::Gte(_)
            | RadIr::And(_)
            | RadIr::Or(_) => 1,
            RadIr::LogicalLoad(_) => 2,
            RadIr::LogicalStore(_) => 3,
        };

        let children_cost = match enode {
            RadIr::Num(_)
            | RadIr::Bool(_)
            | RadIr::Symbol(_)
            | RadIr::FieldName(_)
            | RadIr::Nil => 0,
            RadIr::Neg(a) | RadIr::Not(a) => costs(*a),
            RadIr::Add([a, b])
            | RadIr::Sub([a, b])
            | RadIr::Mul([a, b])
            | RadIr::Div([a, b])
            | RadIr::Mod([a, b])
            | RadIr::Eq([a, b])
            | RadIr::Neq([a, b])
            | RadIr::Lt([a, b])
            | RadIr::Lte([a, b])
            | RadIr::Gt([a, b])
            | RadIr::Gte([a, b])
            | RadIr::And([a, b])
            | RadIr::Or([a, b])
            | RadIr::LogicalLoad([a, b])
            | RadIr::LogicalStore([a, b]) => costs(*a) + costs(*b),
        };

        base + children_cost
    }
}

const EGRAPH_ITER_LIMIT: usize = 8;

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
    let target = optimize_expr(&assign.target);
    let value = optimize_expr(&assign.value);

    if let Some((target, value)) = optimize_store_stmt(&target, &value, &assign.span) {
        AssignStmt {
            id: assign.id,
            span: assign.span.clone(),
            target,
            value,
        }
    } else {
        AssignStmt {
            id: assign.id,
            span: assign.span.clone(),
            target,
            value,
        }
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
    let mut rec = RecExpr::default();
    let _root = lower_value_expr(expr, &mut rec)?;
    let best = saturate(rec)?;
    raise_value_expr(&best, best.root(), expr.span())
}

fn optimize_store_stmt(target: &Expr, value: &Expr, span: &Span) -> Option<(Expr, Expr)> {
    let mut rec = RecExpr::default();
    let _target_root = lower_location_expr(target, &mut rec)?;
    let _value_root = lower_value_expr(value, &mut rec)?;
    let _store_root = rec.add(RadIr::LogicalStore([_target_root, _value_root]));
    let best = saturate(rec)?;
    raise_store_expr(&best, best.root(), span)
}

fn saturate(rec: RecExpr<RadIr>) -> Option<RecExpr<RadIr>> {
    let runner = Runner::default()
        .with_expr(&rec)
        .with_iter_limit(EGRAPH_ITER_LIMIT)
        .run(&rewrites());
    let root = runner.roots[0];
    let extractor = Extractor::new(&runner.egraph, RadCost);
    let (_, best) = extractor.find_best(root);
    Some(best)
}

fn rewrites() -> Vec<Rewrite<RadIr, ()>> {
    let mut rules = Vec::new();
    rules.extend(egg::rewrite!("add-comm"; "(+ ?a ?b)" <=> "(+ ?b ?a)"));
    rules.extend(egg::rewrite!("mul-comm"; "(* ?a ?b)" <=> "(* ?b ?a)"));
    rules.extend(egg::rewrite!("and-comm"; "(&& ?a ?b)" <=> "(&& ?b ?a)"));
    rules.extend(egg::rewrite!("or-comm"; "(|| ?a ?b)" <=> "(|| ?b ?a)"));
    rules.extend(egg::rewrite!("eq-comm"; "(== ?a ?b)" <=> "(== ?b ?a)"));
    rules.push(egg::rewrite!("add-assoc-l"; "(+ ?a (+ ?b ?c))" => "(+ (+ ?a ?b) ?c)"));
    rules.push(egg::rewrite!("add-assoc-r"; "(+ (+ ?a ?b) ?c)" => "(+ ?a (+ ?b ?c))"));
    rules.push(egg::rewrite!("mul-assoc-l"; "(* ?a (* ?b ?c))" => "(* (* ?a ?b) ?c)"));
    rules.push(egg::rewrite!("mul-assoc-r"; "(* (* ?a ?b) ?c)" => "(* ?a (* ?b ?c))"));
    rules.push(egg::rewrite!("add-zero"; "(+ ?a 0)" => "?a"));
    rules.push(egg::rewrite!("add-zero-comm"; "(+ 0 ?a)" => "?a"));
    rules.push(egg::rewrite!("mul-one"; "(* ?a 1)" => "?a"));
    rules.push(egg::rewrite!("mul-one-comm"; "(* 1 ?a)" => "?a"));
    rules.push(egg::rewrite!("mul-zero"; "(* ?a 0)" => "0"));
    rules.push(egg::rewrite!("mul-zero-comm"; "(* 0 ?a)" => "0"));
    rules.push(egg::rewrite!("sub-zero"; "(- ?a 0)" => "?a"));
    rules.push(egg::rewrite!("sub-self"; "(- ?a ?a)" => "0"));
    rules.push(egg::rewrite!("div-one"; "(/ ?a 1)" => "?a"));
    rules.push(egg::rewrite!("neg-neg"; "(neg (neg ?a))" => "?a"));
    rules.push(egg::rewrite!("not-not"; "(not (not ?a))" => "?a"));
    rules.push(egg::rewrite!("and-true"; "(&& true ?a)" => "?a"));
    rules.push(egg::rewrite!("and-true-comm"; "(&& ?a true)" => "?a"));
    rules.push(egg::rewrite!("and-false"; "(&& false ?a)" => "false"));
    rules.push(egg::rewrite!("and-false-comm"; "(&& ?a false)" => "false"));
    rules.push(egg::rewrite!("or-false"; "(|| false ?a)" => "?a"));
    rules.push(egg::rewrite!("or-false-comm"; "(|| ?a false)" => "?a"));
    rules.push(egg::rewrite!("or-true"; "(|| true ?a)" => "true"));
    rules.push(egg::rewrite!("or-true-comm"; "(|| ?a true)" => "true"));
    rules.push(egg::rewrite!("dup-add"; "(+ ?a ?a)" => "(* 2 ?a)"));
    rules.push(egg::rewrite!("factor-common"; "(+ (* ?a ?b) (* ?a ?c))" => "(* ?a (+ ?b ?c))"));
    rules
}

fn lower_value_expr(expr: &Expr, out: &mut RecExpr<RadIr>) -> Option<Id> {
    match expr {
        Expr::IntLit(value, _) => Some(out.add(RadIr::Num(*value))),
        Expr::FloatLit(_, _) => None,
        Expr::BoolLit(value, _) => Some(out.add(RadIr::Bool(*value))),
        Expr::NilLit(_) => Some(out.add(RadIr::Nil)),
        Expr::StrLit(_, _) => None,
        Expr::Ident(name, _) => Some(out.add(RadIr::Symbol(name.clone().into()))),
        Expr::Unary(op, inner, _) => {
            let inner = lower_value_expr(inner, out)?;
            let node = match op {
                UnaryOp::Neg => RadIr::Neg(inner),
                UnaryOp::Not => RadIr::Not(inner),
                // opaque to algebraic rewrites, like the binary bit ops
                UnaryOp::BitNot => return None,
            };
            Some(out.add(node))
        }
        Expr::Binary(left, op, right, _) => {
            let left = lower_value_expr(left, out)?;
            let right = lower_value_expr(right, out)?;
            let node = match op {
                BinOp::Add => RadIr::Add([left, right]),
                BinOp::Sub => RadIr::Sub([left, right]),
                BinOp::Mul => RadIr::Mul([left, right]),
                BinOp::Div => RadIr::Div([left, right]),
                BinOp::Mod => RadIr::Mod([left, right]),
                BinOp::Eq => RadIr::Eq([left, right]),
                BinOp::Ne => RadIr::Neq([left, right]),
                BinOp::Lt => RadIr::Lt([left, right]),
                BinOp::Le => RadIr::Lte([left, right]),
                BinOp::Gt => RadIr::Gt([left, right]),
                BinOp::Ge => RadIr::Gte([left, right]),
                BinOp::And => RadIr::And([left, right]),
                BinOp::Or => RadIr::Or([left, right]),
                // Bitwise ops are opaque to the e-graph (no rewrite rules
                // would fire on them anyway); they fall back to direct
                // bytecode emission, which is already a single opcode.
                BinOp::Is
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr => return None,
            };
            Some(out.add(node))
        }
        Expr::Field(inner, field, _) => {
            let inner = lower_value_expr(inner, out)?;
            let field = out.add(RadIr::FieldName(field.clone().into()));
            Some(out.add(RadIr::LogicalLoad([inner, field])))
        }
        Expr::Index(inner, index, _) => {
            let inner = lower_value_expr(inner, out)?;
            let index = lower_value_expr(index, out)?;
            Some(out.add(RadIr::LogicalLoad([inner, index])))
        }
        _ => None,
    }
}

fn lower_location_expr(expr: &Expr, out: &mut RecExpr<RadIr>) -> Option<Id> {
    match expr {
        Expr::Ident(name, _) => Some(out.add(RadIr::Symbol(name.clone().into()))),
        Expr::Field(inner, field, _) => {
            let inner = lower_location_expr(inner, out)?;
            let field = out.add(RadIr::FieldName(field.clone().into()));
            Some(out.add(RadIr::LogicalLoad([inner, field])))
        }
        Expr::Index(inner, index, _) => {
            let inner = lower_location_expr(inner, out)?;
            let index = lower_value_expr(index, out)?;
            Some(out.add(RadIr::LogicalLoad([inner, index])))
        }
        _ => None,
    }
}

fn raise_value_expr(expr: &RecExpr<RadIr>, id: Id, span: &Span) -> Option<Expr> {
    match &expr[id] {
        RadIr::Num(value) => Some(Expr::IntLit(*value, span.clone())),
        RadIr::Bool(value) => Some(Expr::BoolLit(*value, span.clone())),
        RadIr::Nil => Some(Expr::NilLit(span.clone())),
        RadIr::Symbol(name) => Some(Expr::Ident(name.to_string(), span.clone())),
        RadIr::FieldName(name) => Some(Expr::Ident(name.to_string(), span.clone())),
        RadIr::Neg(inner) => Some(Expr::Unary(
            UnaryOp::Neg,
            Box::new(raise_value_expr(expr, *inner, span)?),
            span.clone(),
        )),
        RadIr::Not(inner) => Some(Expr::Unary(
            UnaryOp::Not,
            Box::new(raise_value_expr(expr, *inner, span)?),
            span.clone(),
        )),
        RadIr::Add([a, b]) => Some(binary(expr, *a, *b, BinOp::Add, span)?),
        RadIr::Sub([a, b]) => Some(binary(expr, *a, *b, BinOp::Sub, span)?),
        RadIr::Mul([a, b]) => Some(binary(expr, *a, *b, BinOp::Mul, span)?),
        RadIr::Div([a, b]) => Some(binary(expr, *a, *b, BinOp::Div, span)?),
        RadIr::Mod([a, b]) => Some(binary(expr, *a, *b, BinOp::Mod, span)?),
        RadIr::Eq([a, b]) => Some(binary(expr, *a, *b, BinOp::Eq, span)?),
        RadIr::Neq([a, b]) => Some(binary(expr, *a, *b, BinOp::Ne, span)?),
        RadIr::Lt([a, b]) => Some(binary(expr, *a, *b, BinOp::Lt, span)?),
        RadIr::Lte([a, b]) => Some(binary(expr, *a, *b, BinOp::Le, span)?),
        RadIr::Gt([a, b]) => Some(binary(expr, *a, *b, BinOp::Gt, span)?),
        RadIr::Gte([a, b]) => Some(binary(expr, *a, *b, BinOp::Ge, span)?),
        RadIr::And([a, b]) => Some(binary(expr, *a, *b, BinOp::And, span)?),
        RadIr::Or([a, b]) => Some(binary(expr, *a, *b, BinOp::Or, span)?),
        RadIr::LogicalLoad([loc, sel]) => {
            let location = raise_location_expr(expr, *loc, span)?;
            match &expr[*sel] {
                RadIr::FieldName(field) => Some(Expr::Field(
                    Box::new(location),
                    field.to_string(),
                    span.clone(),
                )),
                _ => Some(Expr::Index(
                    Box::new(location),
                    Box::new(raise_value_expr(expr, *sel, span)?),
                    span.clone(),
                )),
            }
        }
        RadIr::LogicalStore(_) => None,
    }
}

fn raise_location_expr(expr: &RecExpr<RadIr>, id: Id, span: &Span) -> Option<Expr> {
    match &expr[id] {
        RadIr::Symbol(name) => Some(Expr::Ident(name.to_string(), span.clone())),
        RadIr::FieldName(name) => Some(Expr::Ident(name.to_string(), span.clone())),
        RadIr::LogicalLoad([loc, sel]) => {
            let location = raise_location_expr(expr, *loc, span)?;
            match &expr[*sel] {
                RadIr::FieldName(field) => Some(Expr::Field(
                    Box::new(location),
                    field.to_string(),
                    span.clone(),
                )),
                _ => Some(Expr::Index(
                    Box::new(location),
                    Box::new(raise_value_expr(expr, *sel, span)?),
                    span.clone(),
                )),
            }
        }
        _ => None,
    }
}

fn raise_store_expr(expr: &RecExpr<RadIr>, id: Id, span: &Span) -> Option<(Expr, Expr)> {
    match &expr[id] {
        RadIr::LogicalStore([loc, val]) => Some((
            raise_location_expr(expr, *loc, span)?,
            raise_value_expr(expr, *val, span)?,
        )),
        _ => None,
    }
}

fn binary(expr: &RecExpr<RadIr>, left: Id, right: Id, op: BinOp, span: &Span) -> Option<Expr> {
    Some(Expr::Binary(
        Box::new(raise_value_expr(expr, left, span)?),
        op,
        Box::new(raise_value_expr(expr, right, span)?),
        span.clone(),
    ))
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
}
