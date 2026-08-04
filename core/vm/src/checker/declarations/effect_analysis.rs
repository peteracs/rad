impl Checker {

    fn stmt_is_conservatively_readonly(
        &self,
        stmt: &Stmt,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> bool {
        match stmt {
            Stmt::Let(s) => {
                if s.mutable {
                    for n in &s.names {
                        local_muts.insert(n.clone());
                    }
                }
                self.expr_is_conservatively_readonly(&s.value)
            }
            Stmt::LetElse(le) => {
                if le.mutable {
                    if let Some(name) = le.primary_binding_name() {
                        local_muts.insert(name);
                    }
                }
                self.expr_is_conservatively_readonly(&le.subject)
                    && self.block_is_conservatively_readonly_with_locals(&le.else_block, local_muts)
            }
            Stmt::Assign(s) => {
                if let Expr::Ident(name, _) = &s.target {
                    if local_muts.contains(name) {
                        return self.expr_is_conservatively_readonly(&s.value);
                    }
                }
                false
            }
            Stmt::If(s) => {
                self.expr_is_conservatively_readonly(&s.condition)
                    && self.block_is_conservatively_readonly_with_locals(&s.then_block, local_muts)
                    && s.else_block
                        .as_ref()
                        .map(|b| self.block_is_conservatively_readonly_with_locals(b, local_muts))
                        .unwrap_or(true)
            }
            Stmt::While(s) => {
                self.expr_is_conservatively_readonly(&s.condition)
                    && self.block_is_conservatively_readonly_with_locals(&s.body, local_muts)
            }
            Stmt::For(s) => {
                for binding in &s.bindings {
                    local_muts.insert(binding.clone());
                }
                self.expr_is_conservatively_readonly(&s.iterable)
                    && self.block_is_conservatively_readonly_with_locals(&s.body, local_muts)
            }
            Stmt::Return(s) => s
                .value
                .as_ref()
                .map(|e| self.expr_is_conservatively_readonly(e))
                .unwrap_or(true),
            Stmt::Break(_) | Stmt::Continue(_) => true,
            Stmt::Emit(_) | Stmt::Schedule(_) | Stmt::Update(_) | Stmt::Settle(_) => false,
            Stmt::Propose(s) => s
                .fields
                .iter()
                .all(|(_, expr)| self.expr_is_conservatively_readonly(expr)),
            Stmt::Next(s) => {
                self.expr_is_conservatively_readonly(&s.entity)
                    && s.fields
                        .iter()
                        .all(|(_, expr)| self.expr_is_conservatively_readonly(expr))
            }
            Stmt::Require(s) => self.expr_is_conservatively_readonly(&s.condition),
            Stmt::Match(m) => {
                self.expr_is_conservatively_readonly(&m.subject)
                    && m.cases.iter().all(|case| {
                        (match &case.pattern {
                            Pattern::Literal(e) => self.expr_is_conservatively_readonly(e),
                            _ => true,
                        }) && case
                            .guard
                            .as_ref()
                            .map(|e| self.expr_is_conservatively_readonly(e))
                            .unwrap_or(true)
                            && self.block_is_conservatively_readonly_with_locals(
                                &case.body, local_muts,
                            )
                    })
            }
            Stmt::Expr(s) => self.expr_is_conservatively_readonly(&s.expr),
            Stmt::OnceGuardPass(_) | Stmt::Error(_) => true,
        }
    }

    fn expr_is_conservatively_pure(&self, expr: &Expr) -> bool {
        match expr {
            Expr::IntLit(_, _)
            | Expr::FloatLit(_, _)
            | Expr::StrLit(_, _)
            | Expr::BoolLit(_, _)
            | Expr::NilLit(_)
            | Expr::Ident(_, _)
            | Expr::StateRef(_, _, _)
            | Expr::SystemRef(_, _)
            | Expr::QueryExpr(_, _) => true,
            Expr::TupleLit(items, _) => items.iter().all(|e| self.expr_is_conservatively_pure(e)),
            Expr::Spread(expr, _) => self.expr_is_conservatively_pure(expr),
            Expr::ListLit(items, _) => items.iter().all(|e| self.expr_is_conservatively_pure(e)),
            Expr::MapLit(entries, _) => entries.iter().all(|(k, v)| {
                self.expr_is_conservatively_pure(k) && self.expr_is_conservatively_pure(v)
            }),
            Expr::FStringExpr(parts, _) => parts.iter().all(|part| match part {
                FStringPart::Lit(_) => true,
                FStringPart::Expr(e, _) => self.expr_is_conservatively_pure(e),
            }),
            Expr::Binary(l, _, r, _) => {
                self.expr_is_conservatively_pure(l) && self.expr_is_conservatively_pure(r)
            }
            Expr::Unary(_, e, _) => self.expr_is_conservatively_pure(e),
            Expr::Pipe(l, r, _) => {
                self.expr_is_conservatively_pure(l) && self.expr_is_conservatively_pure(r)
            }
            Expr::Call(callee, args, _) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if super::diagnostics::is_impure_builtin(name) {
                        return false;
                    }
                    if crate::builtins::builtin_type_scheme(name)
                        .map(|sig| !sig.is_pure)
                        .unwrap_or(false)
                    {
                        return false;
                    }
                    if let Some(sig) = self.functions.get(name) {
                        if !sig.is_pure {
                            return false;
                        }
                    } else if !crate::builtins::is_builtin(name) {
                        return false;
                    }
                } else {
                    return false;
                }
                self.expr_is_conservatively_pure(callee)
                    && args.iter().all(|e| self.expr_is_conservatively_pure(e))
            }
            Expr::Field(obj, _, _) => self.expr_is_conservatively_pure(obj),
            Expr::Index(obj, idx, _) => {
                self.expr_is_conservatively_pure(obj) && self.expr_is_conservatively_pure(idx)
            }
            Expr::ComponentExpr(_, fields, rest, _) => {
                fields
                    .iter()
                    .all(|(_, e)| self.expr_is_conservatively_pure(e))
                    && rest
                        .as_ref()
                        .map(|e| self.expr_is_conservatively_pure(e))
                        .unwrap_or(true)
            }
            Expr::VariantExpr(_, _, fields, _) => fields
                .iter()
                .all(|(_, e)| self.expr_is_conservatively_pure(e)),
            Expr::MatchExpr(m, _) => {
                self.expr_is_conservatively_pure(&m.subject)
                    && m.cases.iter().all(|case| {
                        (match &case.pattern {
                            Pattern::Literal(e) => self.expr_is_conservatively_pure(e),
                            _ => true,
                        }) && case
                            .guard
                            .as_ref()
                            .map(|e| self.expr_is_conservatively_pure(e))
                            .unwrap_or(true)
                            && self.block_is_conservatively_pure(&case.body)
                    })
            }
            Expr::IfExpr(c, t, e, _) => {
                self.expr_is_conservatively_pure(c)
                    && self.expr_is_conservatively_pure(t)
                    && self.expr_is_conservatively_pure(e)
            }
            Expr::FnExpr(_, _, _, _, _, body, _) => self.block_is_conservatively_pure(body),
            Expr::Await(_, _) | Expr::AsyncCall(_, _, _) => false,
            Expr::Try(e, _) => self.expr_is_conservatively_pure(e),
            Expr::EntityLiteral(_, _, _) => false,
            Expr::Error(_) => true,
        }
    }

    fn expr_is_conservatively_readonly(&self, expr: &Expr) -> bool {
        match expr {
            Expr::IntLit(_, _)
            | Expr::FloatLit(_, _)
            | Expr::StrLit(_, _)
            | Expr::BoolLit(_, _)
            | Expr::NilLit(_)
            | Expr::Ident(_, _)
            | Expr::StateRef(_, _, _)
            | Expr::SystemRef(_, _)
            | Expr::QueryExpr(_, _) => true,
            Expr::TupleLit(items, _) => items
                .iter()
                .all(|e| self.expr_is_conservatively_readonly(e)),
            Expr::Spread(expr, _) => self.expr_is_conservatively_readonly(expr),
            Expr::ListLit(items, _) => items
                .iter()
                .all(|e| self.expr_is_conservatively_readonly(e)),
            Expr::MapLit(entries, _) => entries.iter().all(|(k, v)| {
                self.expr_is_conservatively_readonly(k) && self.expr_is_conservatively_readonly(v)
            }),
            Expr::FStringExpr(parts, _) => parts.iter().all(|part| match part {
                FStringPart::Lit(_) => true,
                FStringPart::Expr(e, _) => self.expr_is_conservatively_readonly(e),
            }),
            Expr::Binary(l, _, r, _) => {
                self.expr_is_conservatively_readonly(l) && self.expr_is_conservatively_readonly(r)
            }
            Expr::Unary(_, e, _) => self.expr_is_conservatively_readonly(e),
            Expr::Pipe(l, r, _) => {
                self.expr_is_conservatively_readonly(l) && self.expr_is_conservatively_readonly(r)
            }
            Expr::Call(callee, args, _) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if super::diagnostics::is_impure_builtin(name) {
                        return false;
                    }
                    if !super::diagnostics::is_readonly_builtin(name)
                        && crate::builtins::builtin_type_scheme(name)
                            .map(|sig| !sig.is_pure)
                            .unwrap_or(false)
                    {
                        return false;
                    }
                    if let Some(sig) = self.functions.get(name) {
                        if !(sig.is_pure || sig.effects.is_readonly()) {
                            return false;
                        }
                    } else if !crate::builtins::is_builtin(name) {
                        return false;
                    }
                } else {
                    return false;
                }
                self.expr_is_conservatively_readonly(callee)
                    && args.iter().all(|e| self.expr_is_conservatively_readonly(e))
            }
            Expr::Field(obj, _, _) => self.expr_is_conservatively_readonly(obj),
            Expr::Index(obj, idx, _) => {
                self.expr_is_conservatively_readonly(obj)
                    && self.expr_is_conservatively_readonly(idx)
            }
            Expr::ComponentExpr(_, fields, rest, _) => {
                fields
                    .iter()
                    .all(|(_, e)| self.expr_is_conservatively_readonly(e))
                    && rest
                        .as_ref()
                        .map(|e| self.expr_is_conservatively_readonly(e))
                        .unwrap_or(true)
            }
            Expr::VariantExpr(_, _, fields, _) => fields
                .iter()
                .all(|(_, e)| self.expr_is_conservatively_readonly(e)),
            Expr::MatchExpr(m, _) => {
                self.expr_is_conservatively_readonly(&m.subject)
                    && m.cases.iter().all(|case| {
                        (match &case.pattern {
                            Pattern::Literal(e) => self.expr_is_conservatively_readonly(e),
                            _ => true,
                        }) && case
                            .guard
                            .as_ref()
                            .map(|e| self.expr_is_conservatively_readonly(e))
                            .unwrap_or(true)
                            && self.block_is_conservatively_readonly(&case.body)
                    })
            }
            Expr::IfExpr(c, t, e, _) => {
                self.expr_is_conservatively_readonly(c)
                    && self.expr_is_conservatively_readonly(t)
                    && self.expr_is_conservatively_readonly(e)
            }
            Expr::FnExpr(_, _, _, _, _, body, _) => self.block_is_conservatively_readonly(body),
            Expr::Await(_, _) | Expr::AsyncCall(_, _, _) => false,
            Expr::Try(e, _) => self.expr_is_conservatively_readonly(e),
            Expr::EntityLiteral(_, _, _) => false,
            Expr::Error(_) => true,
        }
    }

    /// Like `block_is_conservatively_pure`, but returns a human-readable
    /// description of the *first* purity breach found (or `None` if pure).
    pub(super) fn find_block_purity_breach(&self, block: &Block) -> Option<String> {
        let mut local_muts = std::collections::HashSet::new();
        self.find_block_purity_breach_with_locals(block, &mut local_muts)
    }

    /// Like [`find_block_purity_breach`], but read-only ECS builtins and
    /// `readonly fn` calls are NOT breaches. This is the `query_where`
    /// predicate contract: world READS during iteration are safe (the
    /// entity list is snapshotted before the predicate runs) — only writes,
    /// IO, events, and unverifiable calls are rejected.
    pub(super) fn find_block_readonly_breach(&self, block: &Block) -> Option<String> {
        self.purity_allow_read_ecs.set(true);
        let breach = self.find_block_purity_breach(block);
        self.purity_allow_read_ecs.set(false);
        breach
    }

    /// Does this effect set stay within reads? (`Unrestricted` does not —
    /// it allows everything.)
    pub(super) fn effect_set_is_read_only(effects: &crate::types::EffectSet) -> bool {
        use crate::types::Effect;
        !(effects.allows(Effect::IO)
            || effects.allows(Effect::ECS)
            || effects.allows(Effect::Event)
            || effects.allows(Effect::Async))
    }

    fn find_block_purity_breach_with_locals(
        &self,
        block: &Block,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> Option<String> {
        for stmt in &block.stmts {
            if let Some(reason) = self.find_stmt_purity_breach(stmt, local_muts) {
                return Some(reason);
            }
        }
        None
    }

    fn find_stmt_purity_breach(
        &self,
        stmt: &Stmt,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> Option<String> {
        match stmt {
            Stmt::Let(s) => {
                if s.mutable {
                    for n in &s.names {
                        local_muts.insert(n.clone());
                    }
                }
                self.find_expr_purity_breach(&s.value)
            }
            Stmt::LetElse(le) => {
                if le.mutable {
                    if let Some(name) = le.primary_binding_name() {
                        local_muts.insert(name);
                    }
                }
                self.find_expr_purity_breach(&le.subject).or_else(|| {
                    self.find_block_purity_breach_with_locals(&le.else_block, local_muts)
                })
            }
            Stmt::Assign(s) => {
                if let Expr::Ident(name, _) = &s.target {
                    if local_muts.contains(name) {
                        return self.find_expr_purity_breach(&s.value);
                    }
                }
                Some("mutates a non-local variable".to_string())
            }
            Stmt::If(s) => self
                .find_expr_purity_breach(&s.condition)
                .or_else(|| self.find_block_purity_breach_with_locals(&s.then_block, local_muts))
                .or_else(|| {
                    s.else_block
                        .as_ref()
                        .and_then(|b| self.find_block_purity_breach_with_locals(b, local_muts))
                }),
            Stmt::While(s) => self
                .find_expr_purity_breach(&s.condition)
                .or_else(|| self.find_block_purity_breach_with_locals(&s.body, local_muts)),
            Stmt::For(s) => {
                for binding in &s.bindings {
                    local_muts.insert(binding.clone());
                }
                self.find_expr_purity_breach(&s.iterable)
                    .or_else(|| self.find_block_purity_breach_with_locals(&s.body, local_muts))
            }
            Stmt::Return(s) => s
                .value
                .as_ref()
                .and_then(|e| self.find_expr_purity_breach(e)),
            Stmt::Break(_) | Stmt::Continue(_) => None,
            Stmt::Emit(_) => Some("emits an event".to_string()),
            Stmt::Schedule(_) => Some("runs a system schedule".to_string()),
            Stmt::Update(_) => Some("calls impure builtin 'set'".to_string()),
            Stmt::Settle(_) => Some("opens a causal settlement".to_string()),
            Stmt::Propose(s) => s
                .fields
                .iter()
                .find_map(|(_, expr)| self.find_expr_purity_breach(expr)),
            Stmt::Next(s) => self.find_expr_purity_breach(&s.entity).or_else(|| {
                s.fields
                    .iter()
                    .find_map(|(_, expr)| self.find_expr_purity_breach(expr))
            }),
            Stmt::Require(s) => self.find_expr_purity_breach(&s.condition),
            Stmt::Match(m) => self.find_expr_purity_breach(&m.subject).or_else(|| {
                m.cases.iter().find_map(|case| {
                    case.guard
                        .as_ref()
                        .and_then(|e| self.find_expr_purity_breach(e))
                        .or_else(|| {
                            self.find_block_purity_breach_with_locals(&case.body, local_muts)
                        })
                })
            }),
            Stmt::Expr(s) => self.find_expr_purity_breach(&s.expr),
            Stmt::OnceGuardPass(_) | Stmt::Error(_) => None,
        }
    }

    pub(super) fn find_expr_purity_breach(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::IntLit(_, _)
            | Expr::FloatLit(_, _)
            | Expr::StrLit(_, _)
            | Expr::BoolLit(_, _)
            | Expr::NilLit(_)
            | Expr::Ident(_, _)
            | Expr::StateRef(_, _, _)
            | Expr::SystemRef(_, _)
            | Expr::QueryExpr(_, _) => None,
            Expr::TupleLit(items, _) => items.iter().find_map(|e| self.find_expr_purity_breach(e)),
            Expr::Spread(expr, _) => self.find_expr_purity_breach(expr),
            Expr::ListLit(items, _) => items.iter().find_map(|e| self.find_expr_purity_breach(e)),
            Expr::MapLit(entries, _) => entries.iter().find_map(|(k, v)| {
                self.find_expr_purity_breach(k)
                    .or_else(|| self.find_expr_purity_breach(v))
            }),
            Expr::FStringExpr(parts, _) => parts.iter().find_map(|part| match part {
                FStringPart::Lit(_) => None,
                FStringPart::Expr(e, _) => self.find_expr_purity_breach(e),
            }),
            Expr::Binary(l, _, r, _) => self
                .find_expr_purity_breach(l)
                .or_else(|| self.find_expr_purity_breach(r)),
            Expr::Unary(_, e, _) => self.find_expr_purity_breach(e),
            Expr::Pipe(l, r, _) => self
                .find_expr_purity_breach(l)
                .or_else(|| self.find_expr_purity_breach(r)),
            Expr::Call(callee, args, _) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if self.purity_allow_read_ecs.get()
                        && matches!(name.as_str(), "base" | "candidate")
                    {
                        return args
                            .first()
                            .and_then(|argument| self.find_expr_purity_breach(argument));
                    }
                    // Read-tolerant mode (`find_block_readonly_breach`):
                    // world reads are not breaches. The read set is the
                    // curated `is_readonly_builtin` list, which shares no
                    // member with `is_impure_builtin` (rand_*, IO, writes,
                    // fork/commit all stay breaches).
                    let read_ok = self.purity_allow_read_ecs.get()
                        && super::diagnostics::is_readonly_builtin(name);
                    if !read_ok {
                        if super::diagnostics::is_impure_builtin(name) {
                            return Some(format!("calls impure builtin '{}'", name));
                        }
                        if crate::builtins::builtin_type_scheme(name)
                            .map(|sig| !sig.is_pure)
                            .unwrap_or(false)
                        {
                            return Some(format!("calls impure builtin '{}'", name));
                        }
                    }
                    if let Some(sig) = self.functions.get(name) {
                        let readonly_fn_ok = self.purity_allow_read_ecs.get()
                            && Self::effect_set_is_read_only(&sig.effects);
                        if !sig.is_pure && !readonly_fn_ok {
                            if let Some(inner) = self.purity_breach_reasons.get(name) {
                                return Some(format!("calls '{}' which {}", name, inner));
                            }
                            return Some(format!(
                                "calls '{}' which is not declared `pure fn`",
                                name
                            ));
                        }
                    } else if !crate::builtins::is_builtin(name) {
                        return Some(format!(
                            "calls '{}' whose purity could not be verified",
                            name
                        ));
                    }
                } else {
                    return Some("calls a dynamic (non-named) function".to_string());
                }
                args.iter().find_map(|e| self.find_expr_purity_breach(e))
            }
            Expr::Field(obj, _, _) => self.find_expr_purity_breach(obj),
            Expr::Index(obj, idx, _) => self
                .find_expr_purity_breach(obj)
                .or_else(|| self.find_expr_purity_breach(idx)),
            Expr::ComponentExpr(_, fields, rest, _) => fields
                .iter()
                .find_map(|(_, e)| self.find_expr_purity_breach(e))
                .or_else(|| rest.as_ref().and_then(|e| self.find_expr_purity_breach(e))),
            Expr::VariantExpr(_, _, fields, _) => fields
                .iter()
                .find_map(|(_, e)| self.find_expr_purity_breach(e)),
            Expr::MatchExpr(m, _) => self.find_expr_purity_breach(&m.subject).or_else(|| {
                m.cases.iter().find_map(|case| {
                    case.guard
                        .as_ref()
                        .and_then(|e| self.find_expr_purity_breach(e))
                        .or_else(|| self.find_block_purity_breach(&case.body))
                })
            }),
            Expr::IfExpr(c, t, e, _) => self
                .find_expr_purity_breach(c)
                .or_else(|| self.find_expr_purity_breach(t))
                .or_else(|| self.find_expr_purity_breach(e)),
            Expr::FnExpr(_, _, _, _, _, body, _) => self.find_block_purity_breach(body),
            Expr::Await(_, _) | Expr::AsyncCall(_, _, _) => Some("uses async/await".to_string()),
            Expr::Try(e, _) => self.find_expr_purity_breach(e),
            Expr::EntityLiteral(_, _, _) => Some("spawns an entity".to_string()),
            Expr::Error(_) => None,
        }
    }

    /// Like `find_block_purity_breach`, but permits ECS mutations (set, spawn,
    /// remove, despawn). Returns `Some(reason)` only when the block performs IO
    /// or Event effects — the operations forbidden inside `simulate()`.
    fn assign_target_root_ident(expr: &Expr) -> Option<&str> {
        match expr {
            Expr::Ident(name, _) => Some(name),
            Expr::Field(inner, _, _) | Expr::Index(inner, _, _) => {
                Self::assign_target_root_ident(inner)
            }
            _ => None,
        }
    }

    /// Simulation safety for a SYSTEM body: the body itself must be
    /// breach-free, and so must every handler transitively reachable
    /// through the events it emits (handlers may emit further events).
    pub(super) fn find_system_sim_breach(
        &self,
        block: &Block,
        sys_mut_params: std::collections::HashSet<String>,
    ) -> Option<String> {
        if let Some(reason) = self.find_block_simulation_breach(block, sys_mut_params) {
            return Some(reason);
        }
        let mut queue = Vec::new();
        Self::collect_emits_in_block(block, &mut queue);
        let mut seen = std::collections::HashSet::new();
        while let Some(ev) = queue.pop() {
            if !seen.insert(ev.clone()) {
                continue;
            }
            // raw name, canonical resolution, and last segment all match
            // (cross-module emits may use any of the three spellings)
            let mut names = vec![ev.clone(), self.resolve_canonical_name(&ev)];
            if let Some(last) = ev.rsplit('.').next() {
                names.push(last.to_string());
            }
            names.dedup();
            for key in names {
                if let Some(blocks) = self.event_handler_blocks.get(&key) {
                    for hb in blocks {
                        if let Some(reason) =
                            self.find_block_simulation_breach(hb, std::collections::HashSet::new())
                        {
                            return Some(format!("handler `on {}` {}", key, reason));
                        }
                        Self::collect_emits_in_block(hb, &mut queue);
                    }
                }
            }
        }
        None
    }

    fn collect_emits_in_block(block: &Block, out: &mut Vec<String>) {
        use crate::visitor::AstVisitor;
        struct EmitCollector<'a> {
            out: &'a mut Vec<String>,
        }
        impl<'a> AstVisitor for EmitCollector<'a> {
            fn visit_stmt(&mut self, stmt: &Stmt) {
                if let Stmt::Emit(e) = stmt {
                    self.out.push(e.event_name.clone());
                }
                crate::visitor::walk_stmt(self, stmt);
            }
        }
        let mut c = EmitCollector { out };
        c.visit_block(block);
    }

    pub(super) fn find_block_simulation_breach(
        &self,
        block: &Block,
        initial_muts: std::collections::HashSet<String>,
    ) -> Option<String> {
        let mut local_muts = initial_muts;
        self.find_block_sim_breach_with_locals(block, &mut local_muts)
    }

    fn find_block_sim_breach_with_locals(
        &self,
        block: &Block,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> Option<String> {
        for stmt in &block.stmts {
            if let Some(reason) = self.find_stmt_sim_breach(stmt, local_muts) {
                return Some(reason);
            }
        }
        None
    }

    fn find_stmt_sim_breach(
        &self,
        stmt: &Stmt,
        local_muts: &mut std::collections::HashSet<String>,
    ) -> Option<String> {
        match stmt {
            Stmt::Let(s) => {
                if s.mutable {
                    for n in &s.names {
                        local_muts.insert(n.clone());
                    }
                }
                self.find_expr_sim_breach(&s.value)
            }
            Stmt::LetElse(le) => {
                if le.mutable {
                    if let Some(name) = le.primary_binding_name() {
                        local_muts.insert(name);
                    }
                }
                self.find_expr_sim_breach(&le.subject)
                    .or_else(|| self.find_block_sim_breach_with_locals(&le.else_block, local_muts))
            }
            Stmt::Assign(s) => {
                let root = Self::assign_target_root_ident(&s.target);
                if let Some(name) = root {
                    if local_muts.contains(name) {
                        return self.find_expr_sim_breach(&s.value);
                    }
                }
                Some("mutates a non-local variable".to_string())
            }
            Stmt::If(s) => self
                .find_expr_sim_breach(&s.condition)
                .or_else(|| self.find_block_sim_breach_with_locals(&s.then_block, local_muts))
                .or_else(|| {
                    s.else_block
                        .as_ref()
                        .and_then(|b| self.find_block_sim_breach_with_locals(b, local_muts))
                }),
            Stmt::While(s) => self
                .find_expr_sim_breach(&s.condition)
                .or_else(|| self.find_block_sim_breach_with_locals(&s.body, local_muts)),
            Stmt::For(s) => {
                for binding in &s.bindings {
                    local_muts.insert(binding.clone());
                }
                self.find_expr_sim_breach(&s.iterable)
                    .or_else(|| self.find_block_sim_breach_with_locals(&s.body, local_muts))
            }
            Stmt::Return(s) => s.value.as_ref().and_then(|e| self.find_expr_sim_breach(e)),
            Stmt::Break(_) | Stmt::Continue(_) => None,
            // `emit` is legal in simulation: events dispatch inside the
            // fork (the VM flushes per simulated tick). Safety moves to
            // the handlers — find_system_sim_breach walks them.
            Stmt::Emit(e) => e
                .fields
                .iter()
                .find_map(|(_, expr)| self.find_expr_sim_breach(expr))
                .or_else(|| e.delay.as_ref().and_then(|d| self.find_expr_sim_breach(d))),
            Stmt::Schedule(_) | Stmt::Update(_) => None,
            Stmt::Settle(s) => self.find_block_sim_breach_with_locals(&s.body, local_muts),
            Stmt::Propose(s) => s
                .fields
                .iter()
                .find_map(|(_, expr)| self.find_expr_sim_breach(expr)),
            Stmt::Next(s) => self.find_expr_sim_breach(&s.entity).or_else(|| {
                s.fields
                    .iter()
                    .find_map(|(_, expr)| self.find_expr_sim_breach(expr))
            }),
            Stmt::Require(s) => self.find_expr_sim_breach(&s.condition),
            Stmt::Match(m) => self.find_expr_sim_breach(&m.subject).or_else(|| {
                m.cases.iter().find_map(|case| {
                    case.guard
                        .as_ref()
                        .and_then(|e| self.find_expr_sim_breach(e))
                        .or_else(|| self.find_block_sim_breach_with_locals(&case.body, local_muts))
                })
            }),
            Stmt::Expr(s) => self.find_expr_sim_breach(&s.expr),
            Stmt::OnceGuardPass(_) | Stmt::Error(_) => None,
        }
    }

    fn is_io_builtin(name: &str) -> bool {
        matches!(
            name,
            "print"
                | "eprint"
                | "write_stdout"
                | "write_stderr"
                | "flush_stdout"
                | "input"
                | "readline"
                | "read_stdin_all"
                | "read_file"
                | "write_file"
                | "append_file"
                | "file_exists"
                | "remove_file"
                | "list_dir"
                | "create_dir"
                | "remove_dir"
                | "read_file_bytes"
                | "write_file_bytes"
                | "http_get"
                | "http_post"
                | "http_post_json"
                | "http_request"
                | "tcp_connect"
                | "tcp_listen"
                | "tcp_accept"
                | "tcp_accept_timeout"
                | "tcp_read"
                | "tcp_write"
                | "tcp_close"
                | "udp_bind"
                | "udp_recv_from"
                | "udp_recv_from_timeout"
                | "udp_recv_from_bytes"
                | "udp_recv_from_bytes_timeout"
                | "udp_recv_bytebuf"
                | "udp_recv_bytebuf_timeout"
                | "udp_send_to"
                | "udp_send_to_bytes"
                | "udp_send_bytebuf"
                | "udp_close"
                | "now_unix_s"
                | "now_unix_ms"
                | "clock"
                | "rand_int"
                | "rand_float"
                | "rand_bool"
                | "rand_seed"
                | "load_extension"
                | "log"
                | "metric"
        )
    }

    /// Draw-from-the-stream randomness only. `rand_seed` stays banned even
    /// under the lenient walk: re-seeding inside a speculated system would
    /// collapse `simulate_par`'s per-fork divergence.
    fn is_rand_builtin(name: &str) -> bool {
        matches!(name, "rand_int" | "rand_float" | "rand_bool")
    }

    fn is_event_builtin(name: &str) -> bool {
        matches!(name, "emit" | "transition")
    }

    fn is_simulation_unsafe_builtin(name: &str) -> bool {
        matches!(name, "commit")
    }

    fn find_expr_sim_breach(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::IntLit(_, _)
            | Expr::FloatLit(_, _)
            | Expr::StrLit(_, _)
            | Expr::BoolLit(_, _)
            | Expr::NilLit(_)
            | Expr::Ident(_, _)
            | Expr::StateRef(_, _, _)
            | Expr::SystemRef(_, _)
            | Expr::QueryExpr(_, _) => None,
            Expr::TupleLit(items, _) => items.iter().find_map(|e| self.find_expr_sim_breach(e)),
            Expr::Spread(expr, _) => self.find_expr_sim_breach(expr),
            Expr::ListLit(items, _) => items.iter().find_map(|e| self.find_expr_sim_breach(e)),
            Expr::MapLit(entries, _) => entries.iter().find_map(|(k, v)| {
                self.find_expr_sim_breach(k)
                    .or_else(|| self.find_expr_sim_breach(v))
            }),
            Expr::FStringExpr(parts, _) => parts.iter().find_map(|part| match part {
                FStringPart::Lit(_) => None,
                FStringPart::Expr(e, _) => self.find_expr_sim_breach(e),
            }),
            Expr::Binary(l, _, r, _) => self
                .find_expr_sim_breach(l)
                .or_else(|| self.find_expr_sim_breach(r)),
            Expr::Unary(_, e, _) => self.find_expr_sim_breach(e),
            Expr::Pipe(l, r, _) => self
                .find_expr_sim_breach(l)
                .or_else(|| self.find_expr_sim_breach(r)),
            Expr::Call(callee, args, _) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if Self::is_io_builtin(name)
                        && !(self.sim_breach_allow_rand && Self::is_rand_builtin(name))
                    {
                        return Some(format!(
                            "calls IO builtin '{}' (forbidden in simulation)",
                            name
                        ));
                    }
                    if Self::is_event_builtin(name) {
                        return Some(format!(
                            "calls event builtin '{}' (forbidden in simulation)",
                            name
                        ));
                    }
                    if Self::is_simulation_unsafe_builtin(name) {
                        return Some(format!(
                            "calls '{}' (forbidden in simulation — would corrupt the forked world)",
                            name
                        ));
                    }
                    if let Some(sig) = self.functions.get(name) {
                        if sig.effects.allows(crate::types::Effect::IO) && !sig.effects.is_pure() {
                            return Some(format!("calls '{}' which has IO effects", name));
                        }
                        if sig.effects.allows(crate::types::Effect::Event) && !sig.effects.is_pure()
                        {
                            return Some(format!("calls '{}' which has Event effects", name));
                        }
                    }
                }
                args.iter().find_map(|e| self.find_expr_sim_breach(e))
            }
            Expr::Field(obj, _, _) => self.find_expr_sim_breach(obj),
            Expr::Index(obj, idx, _) => self
                .find_expr_sim_breach(obj)
                .or_else(|| self.find_expr_sim_breach(idx)),
            Expr::ComponentExpr(_, fields, rest, _) => fields
                .iter()
                .find_map(|(_, e)| self.find_expr_sim_breach(e))
                .or_else(|| rest.as_ref().and_then(|e| self.find_expr_sim_breach(e))),
            Expr::VariantExpr(_, _, fields, _) => fields
                .iter()
                .find_map(|(_, e)| self.find_expr_sim_breach(e)),
            Expr::MatchExpr(m, _) => self.find_expr_sim_breach(&m.subject).or_else(|| {
                m.cases.iter().find_map(|case| {
                    case.guard
                        .as_ref()
                        .and_then(|e| self.find_expr_sim_breach(e))
                        .or_else(|| {
                            self.find_block_simulation_breach(
                                &case.body,
                                std::collections::HashSet::new(),
                            )
                        })
                })
            }),
            Expr::IfExpr(c, t, e, _) => self
                .find_expr_sim_breach(c)
                .or_else(|| self.find_expr_sim_breach(t))
                .or_else(|| self.find_expr_sim_breach(e)),
            Expr::FnExpr(_, _, _, _, _, body, _) => {
                self.find_block_simulation_breach(body, std::collections::HashSet::new())
            }
            Expr::Await(_, _) | Expr::AsyncCall(_, _, _) => {
                Some("uses async/await (forbidden in simulation)".to_string())
            }
            Expr::Try(e, _) => self.find_expr_sim_breach(e),
            Expr::EntityLiteral(_, _, _) => None,
            Expr::Error(_) => None,
        }
    }

    fn check_system_cycles(&mut self) {
        let names: Vec<String> = self.systems.keys().cloned().collect();
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        for name in &names {
            graph.insert(name.clone(), Vec::new());
        }
        let mut system_spans: HashMap<String, Span> = HashMap::new();
        for (name, (after, before, span)) in &self.system_deps {
            system_spans.insert(name.clone(), span.clone());
            for dep in after {
                if self.systems.contains_key(dep) {
                    graph.entry(name.clone()).or_default().push(dep.clone());
                }
            }
            for dep in before {
                if self.systems.contains_key(dep) {
                    graph.entry(dep.clone()).or_default().push(name.clone());
                }
            }
        }
        let mut visited: HashMap<String, u8> = HashMap::new();
        for name in &names {
            if self.dfs_cycle(name, &graph, &mut visited) {
                let span = system_spans.get(name).cloned().unwrap_or_default();
                self.errors.push(TypeError {
                    line: span.line,
                    col: span.col,
                    file: span.file,
                    message: format!("Circular system dependency detected involving '{}'", name),
                    hint: Some("Check after/before declarations for cycles".to_string()),
                });
                break;
            }
        }
    }

    fn dfs_cycle(
        &self,
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        visited: &mut HashMap<String, u8>,
    ) -> bool {
        match visited.get(node) {
            Some(1) => return true,
            Some(2) => return false,
            _ => {}
        }
        visited.insert(node.to_string(), 1);
        if let Some(deps) = graph.get(node) {
            for dep in deps {
                if self.dfs_cycle(dep, graph, visited) {
                    return true;
                }
            }
        }
        visited.insert(node.to_string(), 2);
        false
    }
}
