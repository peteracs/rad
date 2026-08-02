//! Unified AST traversal (syn-style [`visit`](https://docs.rs/syn/latest/syn/visit/trait.Visit.html)).
//!
//! Default `visit_*` implementations delegate to `walk_*` so passes only override the nodes they care about
//! and still recurse into children via `walk_*` where needed.

use crate::ast::*;

pub trait AstVisitor {
    fn visit_program(&mut self, program: &Program) {
        walk_program(self, program);
    }

    fn visit_decl(&mut self, decl: &Decl) {
        walk_decl(self, decl);
    }

    fn visit_block(&mut self, block: &Block) {
        walk_block(self, block);
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        walk_stmt(self, stmt);
    }

    fn visit_schedule_stmt(&mut self, stmt: &ScheduleStmt) {
        walk_schedule_stmt(self, stmt);
    }

    fn visit_match_stmt(&mut self, stmt: &MatchStmt) {
        walk_match_stmt(self, stmt);
    }

    fn visit_pattern(&mut self, pattern: &Pattern) {
        walk_pattern(self, pattern);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        walk_expr(self, expr);
    }

    fn visit_call_expr(&mut self, callee: &Expr, args: &[Expr], _span: &Span) {
        walk_call_expr(self, callee, args);
    }

    fn visit_type_expr(&mut self, te: &TypeExpr) {
        walk_type_expr(self, te);
    }

    fn visit_fn_decl(&mut self, decl: &FnDecl) {
        walk_fn_decl(self, decl);
    }
}

pub fn walk_program<V: AstVisitor + ?Sized>(v: &mut V, program: &Program) {
    for d in &program.declarations {
        v.visit_decl(d);
    }
}

pub fn walk_decl<V: AstVisitor + ?Sized>(v: &mut V, decl: &Decl) {
    match decl {
        Decl::Component(c) => {
            for field in &c.fields {
                if let Some(te) = &field.type_annotation {
                    v.visit_type_expr(te);
                }
                v.visit_expr(&field.default_value);
            }
        }
        Decl::Resource(r) => {
            for field in &r.fields {
                if let Some(te) = &field.type_annotation {
                    v.visit_type_expr(te);
                }
                v.visit_expr(&field.default_value);
            }
        }
        Decl::Struct(s) => {
            for field in &s.fields {
                if let Some(te) = &field.type_annotation {
                    v.visit_type_expr(te);
                }
                v.visit_expr(&field.default_value);
            }
        }
        Decl::Intent(i) => {
            for field in &i.fields {
                v.visit_type_expr(&field.type_annotation);
            }
        }
        Decl::Law(l) => {
            for ty in &l.param_types {
                v.visit_type_expr(ty);
            }
            v.visit_block(&l.body);
        }
        Decl::Resolver(r) => v.visit_block(&r.body),
        Decl::Entity(e) => {
            for entry in &e.components {
                match entry {
                    ComponentEntry::Init(init) => {
                        for (_, ex) in &init.fields {
                            v.visit_expr(ex);
                        }
                    }
                    ComponentEntry::Expr(ex) => v.visit_expr(ex),
                }
            }
        }
        Decl::State(s) => {
            for st in &s.states {
                for (_, _, guard) in &st.transitions {
                    if let Some(g) = guard {
                        v.visit_expr(g);
                    }
                }
            }
        }
        Decl::System(s) => v.visit_block(&s.body),
        Decl::Event(e) => {
            for (_, te) in &e.fields {
                if let Some(te) = te {
                    v.visit_type_expr(te);
                }
            }
        }
        Decl::OnHandler(h) => v.visit_block(&h.body),
        Decl::Migration(m) => v.visit_block(&m.body),
        Decl::Phase(_) => {}
        Decl::Fn(f) => v.visit_fn_decl(f),
        Decl::Type(t) => {
            for var in &t.variants {
                for (_, ex) in &var.fields {
                    v.visit_expr(ex);
                }
            }
        }
        Decl::TypeAlias(a) => v.visit_type_expr(&a.target),
        Decl::Test(t) => {
            v.visit_block(&t.body);
            for (_, ex) in &t.generators {
                v.visit_expr(ex);
            }
        }
        Decl::Stmt(s) => v.visit_stmt(s),
        Decl::Use(_) | Decl::Error => {}
    }
}

pub fn walk_fn_decl<V: AstVisitor + ?Sized>(v: &mut V, decl: &FnDecl) {
    for te in decl.param_types.iter().flatten() {
        v.visit_type_expr(te);
    }
    if let Some(rt) = &decl.return_type {
        v.visit_type_expr(rt);
    }
    v.visit_block(&decl.body);
}

pub fn walk_block<V: AstVisitor + ?Sized>(v: &mut V, block: &Block) {
    for s in &block.stmts {
        v.visit_stmt(s);
    }
}

pub fn walk_stmt<V: AstVisitor + ?Sized>(v: &mut V, stmt: &Stmt) {
    match stmt {
        Stmt::Let(s) => {
            if let Some(te) = &s.type_annotation {
                v.visit_type_expr(te);
            }
            v.visit_expr(&s.value);
        }
        Stmt::LetElse(le) => {
            if let Some(te) = &le.type_annotation {
                v.visit_type_expr(te);
            }
            v.visit_expr(&le.subject);
            v.visit_block(&le.else_block);
        }
        Stmt::Assign(s) => {
            v.visit_expr(&s.target);
            v.visit_expr(&s.value);
        }
        Stmt::If(s) => {
            v.visit_expr(&s.condition);
            v.visit_block(&s.then_block);
            if let Some(b) = &s.else_block {
                v.visit_block(b);
            }
        }
        Stmt::While(s) => {
            v.visit_expr(&s.condition);
            v.visit_block(&s.body);
        }
        Stmt::For(s) => {
            v.visit_expr(&s.iterable);
            v.visit_block(&s.body);
        }
        Stmt::Return(s) => {
            if let Some(e) = &s.value {
                v.visit_expr(e);
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Update(s) => {
            if let Some(ref ent) = s.entity_expr {
                v.visit_expr(ent);
            }
            for fu in &s.field_updates {
                if let Some(idx) = &fu.index {
                    v.visit_expr(idx);
                }
                v.visit_expr(&fu.value);
            }
        }
        Stmt::Settle(s) => v.visit_block(&s.body),
        Stmt::Propose(s) => {
            for (_, expr) in &s.fields {
                v.visit_expr(expr);
            }
        }
        Stmt::Next(s) => {
            v.visit_expr(&s.entity);
            for (_, expr) in &s.fields {
                v.visit_expr(expr);
            }
        }
        Stmt::Emit(s) => {
            for (_, ex) in &s.fields {
                v.visit_expr(ex);
            }
            if let Some(d) = &s.delay {
                v.visit_expr(d);
            }
        }
        Stmt::Schedule(s) => v.visit_schedule_stmt(s),
        Stmt::Match(s) => v.visit_match_stmt(s),
        Stmt::Expr(s) => v.visit_expr(&s.expr),
        Stmt::OnceGuardPass(_) | Stmt::Error(_) => {}
    }
}

pub fn walk_schedule_stmt<V: AstVisitor + ?Sized>(_v: &mut V, _stmt: &ScheduleStmt) {}

pub fn walk_match_stmt<V: AstVisitor + ?Sized>(v: &mut V, stmt: &MatchStmt) {
    v.visit_expr(&stmt.subject);
    for case in &stmt.cases {
        v.visit_pattern(&case.pattern);
        if let Some(g) = &case.guard {
            v.visit_expr(g);
        }
        v.visit_block(&case.body);
    }
}

pub fn walk_pattern<V: AstVisitor + ?Sized>(v: &mut V, pattern: &Pattern) {
    match pattern {
        Pattern::Wildcard | Pattern::HasComponent { .. } => {}
        Pattern::Literal(ex) => v.visit_expr(ex),
        Pattern::Variant { .. } => {}
    }
}

pub fn walk_call_expr<V: AstVisitor + ?Sized>(v: &mut V, callee: &Expr, args: &[Expr]) {
    v.visit_expr(callee);
    for a in args {
        v.visit_expr(a);
    }
}

pub fn walk_type_expr<V: AstVisitor + ?Sized>(v: &mut V, te: &TypeExpr) {
    match te {
        TypeExpr::Named(_) => {}
        TypeExpr::Union(xs) => {
            for x in xs {
                v.visit_type_expr(x);
            }
        }
        TypeExpr::Generic(_, args) => {
            for x in args {
                v.visit_type_expr(x);
            }
        }
        TypeExpr::Tuple(xs) => {
            for x in xs {
                v.visit_type_expr(x);
            }
        }
        TypeExpr::FnType(args, ret, _) => {
            for x in args {
                v.visit_type_expr(x);
            }
            v.visit_type_expr(ret);
        }
    }
}

pub fn walk_expr<V: AstVisitor + ?Sized>(v: &mut V, expr: &Expr) {
    match expr {
        Expr::IntLit(_, _)
        | Expr::FloatLit(_, _)
        | Expr::StrLit(_, _)
        | Expr::BoolLit(_, _)
        | Expr::NilLit(_)
        | Expr::Ident(_, _)
        | Expr::StateRef(_, _, _)
        | Expr::SystemRef(_, _)
        | Expr::Error(_) => {}
        Expr::TupleLit(items, _) => {
            for item in items {
                v.visit_expr(item);
            }
        }
        Expr::Spread(inner, _) => v.visit_expr(inner),
        Expr::ListLit(items, _) => {
            for item in items {
                v.visit_expr(item);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, val) in entries {
                v.visit_expr(k);
                v.visit_expr(val);
            }
        }
        Expr::FStringExpr(parts, _) => {
            for part in parts {
                if let FStringPart::Expr(e, _) = part {
                    v.visit_expr(e);
                }
            }
        }
        Expr::Binary(a, _, b, _) => {
            v.visit_expr(a);
            v.visit_expr(b);
        }
        Expr::Unary(_, inner, _) => v.visit_expr(inner),
        Expr::Pipe(a, b, _) => {
            v.visit_expr(a);
            v.visit_expr(b);
        }
        Expr::Call(callee, args, span) => v.visit_call_expr(callee, args.as_slice(), span),
        Expr::Field(inner, _, _) => v.visit_expr(inner),
        Expr::Index(a, b, _) => {
            v.visit_expr(a);
            v.visit_expr(b);
        }
        Expr::ComponentExpr(_, fields, spread, _) => {
            for (_, e) in fields {
                v.visit_expr(e);
            }
            if let Some(s) = spread {
                v.visit_expr(s);
            }
        }
        Expr::VariantExpr(_, _, fields, _) => {
            for (_, e) in fields {
                v.visit_expr(e);
            }
        }
        Expr::MatchExpr(m, _) => v.visit_match_stmt(m),
        Expr::IfExpr(c, t, e, _) => {
            v.visit_expr(c);
            v.visit_expr(t);
            v.visit_expr(e);
        }
        Expr::FnExpr(_, _, param_tys, _, ret, body, _) => {
            for pt in param_tys.iter().flatten() {
                v.visit_type_expr(pt);
            }
            if let Some(rt) = ret {
                v.visit_type_expr(rt);
            }
            v.visit_block(body);
        }
        Expr::QueryExpr(q, _) => {
            if let Some(f) = &q.filter {
                v.visit_expr(f);
            }
        }
        Expr::Await(inner, _) => v.visit_expr(inner),
        Expr::AsyncCall(callee, args, _) => {
            v.visit_expr(callee);
            for a in args {
                v.visit_expr(a);
            }
        }
        Expr::Try(inner, _) => v.visit_expr(inner),
        Expr::EntityLiteral(name, components, _) => {
            if let Some(name_expr) = name {
                v.visit_expr(name_expr);
            }
            for entry in components {
                match entry {
                    ComponentEntry::Init(ci) => {
                        for (_name, fexpr) in &ci.fields {
                            v.visit_expr(fexpr);
                        }
                    }
                    ComponentEntry::Expr(ex) => v.visit_expr(ex),
                }
            }
        }
    }
}
