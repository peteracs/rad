use super::Checker;
use crate::ast::*;
use std::collections::{HashMap, HashSet};

impl Checker {
    pub(super) fn check_public_reachability(&mut self, program: &Program) {
        // Iterate over all declarations and check if pub declarations leak private types
        for decl in &program.declarations {
            match decl {
                Decl::Fn(f) if f.is_pub => {
                    for (idx, param_type) in f.param_types.iter().enumerate() {
                        if let Some(Some(te)) = Some(param_type) {
                            self.check_type_expr_visibility(
                                te,
                                &f.span,
                                &format!(
                                    "parameter '{}' of public function '{}'",
                                    f.params[idx], f.name
                                ),
                            );
                        }
                    }
                    if let Some(te) = &f.return_type {
                        self.check_type_expr_visibility(
                            te,
                            &f.span,
                            &format!("return type of public function '{}'", f.name),
                        );
                    }
                }
                Decl::Component(c) if c.is_pub => {
                    for field in &c.fields {
                        if let Some(te) = &field.type_annotation {
                            self.check_type_expr_visibility(
                                te,
                                &c.span,
                                &format!("field '{}' of public component '{}'", field.name, c.name),
                            );
                        }
                    }
                }
                Decl::Struct(s) if s.is_pub => {
                    for field in &s.fields {
                        if let Some(te) = &field.type_annotation {
                            self.check_type_expr_visibility(
                                te,
                                &s.span,
                                &format!("field '{}' of public struct '{}'", field.name, s.name),
                            );
                        }
                    }
                }
                Decl::Event(e) if e.is_pub => {
                    for (field_name, type_ann) in &e.fields {
                        if let Some(te) = type_ann {
                            self.check_type_expr_visibility(
                                te,
                                &e.span,
                                &format!("field '{}' of public event '{}'", field_name, e.name),
                            );
                        }
                    }
                }
                Decl::Type(t) if t.is_pub => {
                    for v in &t.variants {
                        for (field_name, expr) in &v.fields {
                            // Sum type fields are inferred from expressions.
                            // If they are explicitly typed via identifiers that resolve to types, we should check them.
                            // But since they use expressions, we check if the expression is an identifier referencing a type.
                            if let Expr::Ident(name, _) = expr {
                                let resolved = self.resolve_canonical_name(name);
                                if self.is_private_type(&resolved) {
                                    self.error(
                                        &t.span,
                                        format!("Public API leak: field '{}' of public sum type variant '{}::{}' exposes private type '{}'", field_name, t.name, v.name, name),
                                        Some(format!("Make '{}' public or do not expose it in a public API", name)),
                                    );
                                }
                            }
                        }
                    }
                }
                Decl::TypeAlias(a) if a.is_pub => {
                    self.check_type_expr_visibility(
                        &a.target,
                        &a.span,
                        &format!("target of public type alias '{}'", a.name),
                    );
                }
                _ => {}
            }
        }
    }

    fn check_type_expr_visibility(&mut self, te: &TypeExpr, span: &Span, context: &str) {
        match te {
            TypeExpr::Named(name) => {
                let resolved = self.resolve_canonical_name(name);
                if self.is_private_type(&resolved) {
                    self.error(
                        span,
                        format!(
                            "Public API leak: {} exposes private type '{}'",
                            context, name
                        ),
                        Some(format!(
                            "Make '{}' public or do not expose it in a public API",
                            name
                        )),
                    );
                }
            }
            TypeExpr::Generic(name, params) => {
                let resolved = self.resolve_canonical_name(name);
                if self.is_private_type(&resolved) {
                    self.error(
                        span,
                        format!(
                            "Public API leak: {} exposes private type '{}'",
                            context, name
                        ),
                        Some(format!(
                            "Make '{}' public or do not expose it in a public API",
                            name
                        )),
                    );
                }
                for param in params {
                    self.check_type_expr_visibility(param, span, context);
                }
            }
            TypeExpr::Tuple(types) => {
                for t in types {
                    self.check_type_expr_visibility(t, span, context);
                }
            }
            TypeExpr::FnType(params, ret, _) => {
                for param in params {
                    self.check_type_expr_visibility(param, span, context);
                }
                self.check_type_expr_visibility(ret, span, context);
            }
            TypeExpr::Union(types) => {
                for t in types {
                    self.check_type_expr_visibility(t, span, context);
                }
            }
        }
    }

    fn is_private_type(&self, name: &str) -> bool {
        // Language-provided sum types are everyone's vocabulary — a pub fn
        // returning Result/Option leaks nothing.
        if matches!(name, "Result" | "Option") {
            return false;
        }
        if let Some(c) = self.components.get(name) {
            return !c.is_pub;
        }
        if let Some(s) = self.structs.get(name) {
            return !s.is_pub;
        }
        if let Some(t) = self.sum_types.get(name) {
            return !t.is_pub;
        }
        if let Some(a) = self.type_aliases.get(name) {
            return !a.is_pub;
        }
        if let Some(e) = self.events.get(name) {
            return !e.is_pub;
        }
        if let Some(sm) = self.state_machines.get(name) {
            return !sm.is_pub;
        }
        // Builtin types or unknown types are not considered private leaks
        false
    }

    pub(super) fn check_reachability(&mut self, program: &Program) {
        let mut reachable_functions = HashSet::new();
        let mut reachable_components = HashSet::new();
        let mut reachable_events = HashSet::new();
        let mut reachable_structs = HashSet::new();

        let mut queue = Vec::new();

        let mut fn_bodies: HashMap<String, &FnDecl> = HashMap::new();
        let mut event_handlers: HashMap<String, Vec<&Block>> = HashMap::new();
        let mut component_decls: HashMap<String, &ComponentDecl> = HashMap::new();
        let mut struct_decls: HashMap<String, &StructDecl> = HashMap::new();

        for decl in &program.declarations {
            match decl {
                Decl::Fn(f) => {
                    fn_bodies.insert(f.name.clone(), f);
                    // `pub fn` is an EXTERNAL surface for the same reason
                    // `pub` events are (see the Decl::Event arm below): it is
                    // the module's published API, callable from files this
                    // analysis cannot see. A library module has no `main`, so
                    // without this every private helper reached only through
                    // its public API was reported unused — and since `pub` fns
                    // are already exempt from the report itself, the warning
                    // landed on the helper with the advice "consider removing
                    // it", which would break the module.
                    if f.name == "main" || f.is_pub {
                        reachable_functions.insert(f.name.clone());
                        queue.push(VisitItem::FnDecl(f));
                    }
                }
                Decl::Component(c) => {
                    component_decls.insert(c.name.clone(), c);
                }
                Decl::Struct(s) => {
                    struct_decls.insert(s.name.clone(), s);
                }
                Decl::OnHandler(h) => {
                    event_handlers
                        .entry(h.event_name.clone())
                        .or_default()
                        .push(&h.body);
                }
                Decl::Event(e) if e.is_pub => {
                    // pub events are an EXTERNAL surface: hosts inject them
                    // (session_emit), so their handlers — and everything
                    // those call — are live even if no rad code emits them.
                    // This was the GUI-app false-positive class: every
                    // `fn` called only from `on Click` flagged unused.
                    reachable_events.insert(e.name.clone());
                }
                Decl::System(s) => {
                    queue.push(VisitItem::Block(&s.body));
                    for (_, _, comp) in &s.params {
                        if reachable_components.insert(comp.clone()) {
                            if let Some(decl) = component_decls.get(comp) {
                                queue.push(VisitItem::ComponentDecl(decl));
                            }
                        }
                    }
                }
                Decl::Test(t) => {
                    queue.push(VisitItem::Block(&t.body));
                }
                Decl::Stmt(s) => {
                    queue.push(VisitItem::Stmt(s));
                }
                _ => {}
            }
        }

        // handlers of externally-injectable (pub) events are roots; the
        // decl scan above marked the events, now queue their bodies
        for name in reachable_events.iter() {
            if let Some(handlers) = event_handlers.get(name) {
                for handler in handlers {
                    queue.push(VisitItem::Block(handler));
                }
            }
        }

        for (alias_name, decls) in &self.alias_decls {
            let name_map = match self.module_aliases.get(alias_name) {
                Some(m) => m,
                None => continue,
            };
            for decl in decls {
                match decl {
                    Decl::Fn(f) => {
                        let mangled = name_map
                            .get(&f.name)
                            .cloned()
                            .unwrap_or_else(|| f.name.clone());
                        fn_bodies.insert(mangled, f);
                    }
                    Decl::Component(c) => {
                        let mangled = name_map
                            .get(&c.name)
                            .cloned()
                            .unwrap_or_else(|| c.name.clone());
                        component_decls.insert(mangled, c);
                    }
                    Decl::Struct(s) => {
                        let mangled = name_map
                            .get(&s.name)
                            .cloned()
                            .unwrap_or_else(|| s.name.clone());
                        struct_decls.insert(mangled, s);
                    }
                    Decl::System(s) => {
                        queue.push(VisitItem::Block(&s.body));
                        for (_, _, comp) in &s.params {
                            let resolved_comp = self.resolve_canonical_name(comp);
                            if reachable_components.insert(resolved_comp.clone()) {
                                if let Some(decl) = component_decls.get(&resolved_comp) {
                                    queue.push(VisitItem::ComponentDecl(decl));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        while let Some(item) = queue.pop() {
            match item {
                VisitItem::FnDecl(f) => {
                    queue.push(VisitItem::Block(&f.body));
                    for t in f.param_types.iter().flatten() {
                        queue.push(VisitItem::TypeExpr(t));
                    }
                    if let Some(t) = &f.return_type {
                        queue.push(VisitItem::TypeExpr(t));
                    }
                }
                VisitItem::ComponentDecl(c) => {
                    for field in &c.fields {
                        if let Some(t) = &field.type_annotation {
                            queue.push(VisitItem::TypeExpr(t));
                        }
                        queue.push(VisitItem::Expr(&field.default_value));
                    }
                }
                VisitItem::StructDecl(s) => {
                    for field in &s.fields {
                        if let Some(t) = &field.type_annotation {
                            queue.push(VisitItem::TypeExpr(t));
                        }
                        queue.push(VisitItem::Expr(&field.default_value));
                    }
                }
                VisitItem::Block(b) => {
                    for stmt in &b.stmts {
                        queue.push(VisitItem::Stmt(stmt));
                    }
                }
                VisitItem::Stmt(s) => match s {
                    Stmt::Let(l) => queue.push(VisitItem::Expr(&l.value)),
                    Stmt::LetElse(l) => {
                        queue.push(VisitItem::Expr(&l.subject));
                        queue.push(VisitItem::Block(&l.else_block));
                    }
                    Stmt::Assign(a) => {
                        queue.push(VisitItem::Expr(&a.target));
                        queue.push(VisitItem::Expr(&a.value));
                    }
                    Stmt::If(i) => {
                        queue.push(VisitItem::Expr(&i.condition));
                        queue.push(VisitItem::Block(&i.then_block));
                        if let Some(else_block) = &i.else_block {
                            queue.push(VisitItem::Block(else_block));
                        }
                    }
                    Stmt::While(w) => {
                        queue.push(VisitItem::Expr(&w.condition));
                        queue.push(VisitItem::Block(&w.body));
                    }
                    Stmt::For(f) => {
                        queue.push(VisitItem::Expr(&f.iterable));
                        queue.push(VisitItem::Block(&f.body));
                    }
                    Stmt::Return(r) => {
                        if let Some(expr) = &r.value {
                            queue.push(VisitItem::Expr(expr));
                        }
                    }
                    Stmt::Break(_) | Stmt::Continue(_) => {}
                    Stmt::Emit(e) => {
                        if reachable_events.insert(e.event_name.clone()) {
                            if let Some(handlers) = event_handlers.get(&e.event_name) {
                                for handler in handlers {
                                    queue.push(VisitItem::Block(handler));
                                }
                            }
                        }
                        if let Some(d) = &e.delay {
                            queue.push(VisitItem::Expr(d));
                        }
                        for (_, expr) in &e.fields {
                            queue.push(VisitItem::Expr(expr));
                        }
                    }
                    Stmt::Schedule(sched) => {
                        for sys_name in &sched.systems {
                            if fn_bodies.contains_key(sys_name)
                                && reachable_functions.insert(sys_name.clone())
                            {
                                if let Some(decl) = fn_bodies.get(sys_name) {
                                    queue.push(VisitItem::FnDecl(decl));
                                }
                            }
                        }
                    }
                    Stmt::Update(u) => {
                        if let Some(ref ent) = u.entity_expr {
                            queue.push(VisitItem::Expr(ent));
                        }
                        for fu in &u.field_updates {
                            if let Some(idx) = &fu.index {
                                queue.push(VisitItem::Expr(idx));
                            }
                            queue.push(VisitItem::Expr(&fu.value));
                        }
                    }
                    Stmt::Settle(settlement) => {
                        queue.push(VisitItem::Block(&settlement.body));
                    }
                    Stmt::Propose(proposal) => {
                        for (_, expr) in &proposal.fields {
                            queue.push(VisitItem::Expr(expr));
                        }
                    }
                    Stmt::Next(next) => {
                        reachable_components
                            .insert(self.resolve_canonical_name(&next.component_name));
                        queue.push(VisitItem::Expr(&next.entity));
                        for (_, expr) in &next.fields {
                            queue.push(VisitItem::Expr(expr));
                        }
                    }
                    Stmt::Require(requirement) => {
                        queue.push(VisitItem::Expr(&requirement.condition));
                    }
                    Stmt::Match(m) => {
                        queue.push(VisitItem::Expr(&m.subject));
                        for arm in &m.cases {
                            if let Some(guard) = &arm.guard {
                                queue.push(VisitItem::Expr(guard));
                            }
                            queue.push(VisitItem::Block(&arm.body));
                        }
                    }
                    Stmt::Expr(e) => {
                        queue.push(VisitItem::Expr(&e.expr));
                    }
                    Stmt::OnceGuardPass(_) | Stmt::Error(_) => {}
                },
                VisitItem::Expr(e) => match e {
                    Expr::ListLit(items, _) | Expr::TupleLit(items, _) => {
                        for item in items {
                            queue.push(VisitItem::Expr(item));
                        }
                    }
                    Expr::MapLit(items, _) => {
                        for (k, v) in items {
                            queue.push(VisitItem::Expr(k));
                            queue.push(VisitItem::Expr(v));
                        }
                    }
                    Expr::FStringExpr(parts, _) => {
                        for part in parts {
                            if let FStringPart::Expr(expr, _) = part {
                                queue.push(VisitItem::Expr(expr));
                            }
                        }
                    }
                    Expr::Ident(name, _) => {
                        if fn_bodies.contains_key(name) && reachable_functions.insert(name.clone())
                        {
                            if let Some(decl) = fn_bodies.get(name) {
                                queue.push(VisitItem::FnDecl(decl));
                            }
                        }
                        if self.components.contains_key(name)
                            && reachable_components.insert(name.clone())
                        {
                            if let Some(decl) = component_decls.get(name) {
                                queue.push(VisitItem::ComponentDecl(decl));
                            }
                        }
                        if self.events.contains_key(name) {
                            reachable_events.insert(name.clone());
                        }
                        if self.structs.contains_key(name) && reachable_structs.insert(name.clone())
                        {
                            if let Some(decl) = struct_decls.get(name) {
                                queue.push(VisitItem::StructDecl(decl));
                            }
                        }
                    }
                    Expr::Binary(lhs, _, rhs, _) => {
                        queue.push(VisitItem::Expr(lhs));
                        queue.push(VisitItem::Expr(rhs));
                    }
                    Expr::Unary(_, expr, _) => {
                        queue.push(VisitItem::Expr(expr));
                    }
                    Expr::Pipe(lhs, rhs, _) => {
                        queue.push(VisitItem::Expr(lhs));
                        queue.push(VisitItem::Expr(rhs));
                    }
                    Expr::Call(callee, args, _) => {
                        queue.push(VisitItem::Expr(callee));
                        for arg in args {
                            queue.push(VisitItem::Expr(arg));
                        }
                    }
                    Expr::Field(expr, field, _) => {
                        if let Expr::Ident(alias, _) = expr.as_ref() {
                            if let Some(mangled) = self.resolve_alias_member(alias, field) {
                                if fn_bodies.contains_key(&mangled)
                                    && reachable_functions.insert(mangled.clone())
                                {
                                    if let Some(decl) = fn_bodies.get(mangled.as_str()) {
                                        queue.push(VisitItem::FnDecl(decl));
                                    }
                                }
                                if self.components.contains_key(&mangled)
                                    && reachable_components.insert(mangled.clone())
                                {
                                    if let Some(decl) = component_decls.get(mangled.as_str()) {
                                        queue.push(VisitItem::ComponentDecl(decl));
                                    }
                                }
                                if self.structs.contains_key(&mangled)
                                    && reachable_structs.insert(mangled.clone())
                                {
                                    if let Some(decl) = struct_decls.get(mangled.as_str()) {
                                        queue.push(VisitItem::StructDecl(decl));
                                    }
                                }
                                if self.events.contains_key(&mangled) {
                                    reachable_events.insert(mangled);
                                }
                            }
                        }
                        queue.push(VisitItem::Expr(expr));
                    }
                    Expr::Index(expr, index, _) => {
                        queue.push(VisitItem::Expr(expr));
                        queue.push(VisitItem::Expr(index));
                    }
                    Expr::ComponentExpr(name, fields, base, _) => {
                        let resolved_name = self.resolve_canonical_name(name);
                        if self.components.contains_key(&resolved_name)
                            && reachable_components.insert(resolved_name.clone())
                        {
                            if let Some(decl) = component_decls.get(&resolved_name) {
                                queue.push(VisitItem::ComponentDecl(decl));
                            }
                        }
                        if self.structs.contains_key(&resolved_name)
                            && reachable_structs.insert(resolved_name.clone())
                        {
                            if let Some(decl) = struct_decls.get(&resolved_name) {
                                queue.push(VisitItem::StructDecl(decl));
                            }
                        }
                        for (_, expr) in fields {
                            queue.push(VisitItem::Expr(expr));
                        }
                        if let Some(base) = base {
                            queue.push(VisitItem::Expr(base));
                        }
                    }
                    Expr::StateRef(_, _, _) => {}
                    Expr::VariantExpr(_, _, fields, _) => {
                        for (_, expr) in fields {
                            queue.push(VisitItem::Expr(expr));
                        }
                    }
                    Expr::MatchExpr(m, _) => {
                        queue.push(VisitItem::Expr(&m.subject));
                        for arm in &m.cases {
                            if let Some(guard) = &arm.guard {
                                queue.push(VisitItem::Expr(guard));
                            }
                            queue.push(VisitItem::Block(&arm.body));
                        }
                    }
                    Expr::IfExpr(c, t, e, _) => {
                        queue.push(VisitItem::Expr(c));
                        queue.push(VisitItem::Expr(t));
                        queue.push(VisitItem::Expr(e));
                    }
                    // `entity { Comp { … } }` uses Comp — without this
                    // arm, components used only in entity literals were
                    // flagged unused
                    Expr::EntityLiteral(name, entries, _) => {
                        if let Some(n) = name {
                            queue.push(VisitItem::Expr(n));
                        }
                        for entry in entries {
                            match entry {
                                crate::ast::ComponentEntry::Init(ci) => {
                                    let resolved = self.resolve_canonical_name(&ci.comp_name);
                                    if reachable_components.insert(resolved.clone()) {
                                        if let Some(decl) = component_decls.get(&resolved) {
                                            queue.push(VisitItem::ComponentDecl(decl));
                                        }
                                    }
                                    for (_, fexpr) in &ci.fields {
                                        queue.push(VisitItem::Expr(fexpr));
                                    }
                                }
                                crate::ast::ComponentEntry::Expr(e) => {
                                    queue.push(VisitItem::Expr(e));
                                }
                            }
                        }
                    }
                    Expr::FnExpr(_, _, _, _, _, body, _) => {
                        queue.push(VisitItem::Block(body));
                    }
                    Expr::QueryExpr(q, _) => {
                        for (comp, _) in &q.components {
                            if reachable_components.insert(comp.clone()) {
                                if let Some(decl) = component_decls.get(comp) {
                                    queue.push(VisitItem::ComponentDecl(decl));
                                }
                            }
                        }
                        for comp in &q.select {
                            if reachable_components.insert(comp.clone()) {
                                if let Some(decl) = component_decls.get(comp) {
                                    queue.push(VisitItem::ComponentDecl(decl));
                                }
                            }
                        }
                        if let Some(filter) = &q.filter {
                            queue.push(VisitItem::Expr(filter));
                        }
                    }
                    Expr::Await(expr, _) => {
                        queue.push(VisitItem::Expr(expr));
                    }
                    Expr::AsyncCall(callee, args, _) => {
                        queue.push(VisitItem::Expr(callee));
                        for arg in args {
                            queue.push(VisitItem::Expr(arg));
                        }
                    }
                    Expr::Try(expr, _) => {
                        queue.push(VisitItem::Expr(expr));
                    }
                    Expr::Spread(expr, _) => {
                        queue.push(VisitItem::Expr(expr));
                    }
                    _ => {}
                },
                VisitItem::TypeExpr(t) => match t {
                    TypeExpr::Named(name) => {
                        let resolved = self.resolve_canonical_name(name);
                        if self.components.contains_key(&resolved)
                            && reachable_components.insert(resolved.clone())
                        {
                            if let Some(decl) = component_decls.get(&resolved) {
                                queue.push(VisitItem::ComponentDecl(decl));
                            }
                        }
                        if self.structs.contains_key(&resolved)
                            && reachable_structs.insert(resolved.clone())
                        {
                            if let Some(decl) = struct_decls.get(&resolved) {
                                queue.push(VisitItem::StructDecl(decl));
                            }
                        }
                    }
                    TypeExpr::Generic(name, params) => {
                        let resolved = self.resolve_canonical_name(name);
                        if self.components.contains_key(&resolved)
                            && reachable_components.insert(resolved.clone())
                        {
                            if let Some(decl) = component_decls.get(&resolved) {
                                queue.push(VisitItem::ComponentDecl(decl));
                            }
                        }
                        if self.structs.contains_key(&resolved)
                            && reachable_structs.insert(resolved.clone())
                        {
                            if let Some(decl) = struct_decls.get(&resolved) {
                                queue.push(VisitItem::StructDecl(decl));
                            }
                        }
                        for param in params {
                            queue.push(VisitItem::TypeExpr(param));
                        }
                    }
                    TypeExpr::Tuple(types) => {
                        for t in types {
                            queue.push(VisitItem::TypeExpr(t));
                        }
                    }
                    TypeExpr::FnType(params, ret, _) => {
                        for param in params {
                            queue.push(VisitItem::TypeExpr(param));
                        }
                        queue.push(VisitItem::TypeExpr(ret));
                    }
                    TypeExpr::Union(types) => {
                        for t in types {
                            queue.push(VisitItem::TypeExpr(t));
                        }
                    }
                },
            }
        }

        // `pub` declarations are EXPORTS: a library module's pub items are
        // consumed by importers (or by a host reading the world), so
        // "unused in this file" is not a defect. This false-positive class
        // was reported by two dogfood cycles (lib_combat, lib_sight,
        // lib_gui) before getting fixed here.
        for decl in &program.declarations {
            match decl {
                Decl::Fn(f) => {
                    if !f.is_pub && !reachable_functions.contains(&f.name) {
                        self.warning(
                            &f.span,
                            format!("Unused function '{}'", f.name),
                            Some("If this is intentional, consider removing it".to_string()),
                        );
                    }
                }
                Decl::Event(e) => {
                    if !e.is_pub && !reachable_events.contains(&e.name) {
                        self.warning(
                            &e.span,
                            format!("Unused event '{}'", e.name),
                            Some("If this is intentional, consider removing it".to_string()),
                        );
                    }
                }
                Decl::Component(c) => {
                    if !c.is_pub && !reachable_components.contains(&c.name) {
                        self.warning(
                            &c.span,
                            format!("Unused component '{}'", c.name),
                            Some("If this is intentional, consider removing it".to_string()),
                        );
                    }
                }
                Decl::Struct(s) if !s.is_pub && !reachable_structs.contains(&s.name) => {
                    self.warning(
                        &s.span,
                        format!("Unused struct '{}'", s.name),
                        Some("If this is intentional, consider removing it".to_string()),
                    );
                }
                _ => {}
            }
        }
    }
}

enum VisitItem<'a> {
    Block(&'a Block),
    Stmt(&'a Stmt),
    Expr(&'a Expr),
    TypeExpr(&'a TypeExpr),
    FnDecl(&'a FnDecl),
    ComponentDecl(&'a ComponentDecl),
    StructDecl(&'a StructDecl),
}
