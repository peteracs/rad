use crate::ast::*;
use std::collections::HashSet;

pub fn find_unique_locals(body: &Block) -> HashSet<String> {
    let mut analyzer = EscapeAnalyzer {
        unique_candidates: HashSet::new(),
        escaped: HashSet::new(),
    };
    analyzer.visit_block(body);

    let mut result = HashSet::new();
    for cand in analyzer.unique_candidates {
        if !analyzer.escaped.contains(&cand) {
            result.insert(cand);
        }
    }
    result
}

struct EscapeAnalyzer {
    unique_candidates: HashSet<String>,
    escaped: HashSet<String>,
}

impl EscapeAnalyzer {
    fn visit_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.visit_stmt(stmt);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let(s) => {
                // If initialized with a uniquely-owned container constructor.
                if s.names.len() == 1 {
                    if let Expr::Call(callee, _, _) = &s.value {
                        if let Expr::Ident(name, _) = callee.as_ref() {
                            if name == "bitset_new" || name == "buffer_new" || name == "bytebuf_new"
                            {
                                self.unique_candidates.insert(s.names[0].clone());
                            }
                        }
                    }
                }
                self.visit_expr(&s.value);
            }
            Stmt::LetElse(s) => {
                self.visit_expr(&s.subject);
                self.visit_block(&s.else_block);
            }
            Stmt::Assign(s) => {
                // If it's x = bitset_set(x, ...) or x = x |> bitset_set(...)
                let mut is_inplace = false;
                if let Expr::Ident(target_name, _) = &s.target {
                    if let Expr::Call(callee, args, _) = &s.value {
                        if let Expr::Ident(fn_name, _) = callee.as_ref() {
                            let is_bytebuf_setter = fn_name == "bytebuf_set_u8"
                                || fn_name == "bytebuf_set_u32_le"
                                || fn_name == "bytebuf_set_i32_le";
                            if ((fn_name == "bitset_set"
                                || fn_name == "bitset_clear"
                                || fn_name == "buffer_append")
                                && args.len() == 2)
                                || (is_bytebuf_setter && args.len() == 3)
                            {
                                if let Expr::Ident(arg_name, _) = &args[0] {
                                    if arg_name == target_name {
                                        is_inplace = true;
                                        for arg in args.iter().skip(1) {
                                            self.visit_expr(arg);
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Expr::Pipe(left, right, _) = &s.value {
                        if let Expr::Ident(left_name, _) = left.as_ref() {
                            if left_name == target_name {
                                if let Expr::Call(callee, args, _) = right.as_ref() {
                                    if let Expr::Ident(fn_name, _) = callee.as_ref() {
                                        let is_bytebuf_setter = fn_name == "bytebuf_set_u8"
                                            || fn_name == "bytebuf_set_u32_le"
                                            || fn_name == "bytebuf_set_i32_le";
                                        if ((fn_name == "bitset_set"
                                            || fn_name == "bitset_clear"
                                            || fn_name == "buffer_append")
                                            && args.len() == 1)
                                            || (is_bytebuf_setter && args.len() == 2)
                                        {
                                            is_inplace = true;
                                            for arg in args {
                                                self.visit_expr(arg);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if !is_inplace {
                    self.visit_expr(&s.target);
                    self.visit_expr(&s.value);
                }
            }
            Stmt::If(s) => {
                self.visit_expr(&s.condition);
                self.visit_block(&s.then_block);
                if let Some(b) = &s.else_block {
                    self.visit_block(b);
                }
            }
            Stmt::While(s) => {
                self.visit_expr(&s.condition);
                self.visit_block(&s.body);
            }
            Stmt::For(s) => {
                self.visit_expr(&s.iterable);
                self.visit_block(&s.body);
            }
            Stmt::Return(s) => {
                if let Some(e) = &s.value {
                    if let Expr::Ident(_, _) = e {
                        // Direct return transfers ownership and terminates control flow.
                        // It does not cause aliasing that could be observed by subsequent mutations.
                    } else {
                        self.visit_expr(e);
                    }
                }
            }
            Stmt::Match(s) => {
                self.visit_expr(&s.subject);
                for case in &s.cases {
                    if let Some(g) = &case.guard {
                        self.visit_expr(g);
                    }
                    self.visit_block(&case.body);
                }
            }
            Stmt::Expr(s) => {
                self.visit_expr(&s.expr);
            }
            Stmt::Emit(s) => {
                for (_, e) in &s.fields {
                    self.visit_expr(e);
                }
                if let Some(d) = &s.delay {
                    self.visit_expr(d);
                }
            }
            Stmt::Schedule(_) => {}
            Stmt::Update(s) => {
                if let Some(ref ent) = s.entity_expr {
                    self.visit_expr(ent);
                }
                for fu in &s.field_updates {
                    if let Some(idx) = &fu.index {
                        self.visit_expr(idx);
                    }
                    self.visit_expr(&fu.value);
                }
            }
            Stmt::Settle(s) => self.visit_block(&s.body),
            Stmt::Propose(s) => {
                for (_, expr) in &s.fields {
                    self.visit_expr(expr);
                }
            }
            Stmt::Next(s) => {
                self.visit_expr(&s.entity);
                for (_, expr) in &s.fields {
                    self.visit_expr(expr);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::OnceGuardPass(_) | Stmt::Error(_) => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name, _) => {
                self.escaped.insert(name.clone());
            }
            Expr::Call(callee, args, _) => {
                if let Expr::Ident(fn_name, _) = callee.as_ref() {
                    if (fn_name == "bitset_has"
                        || fn_name == "buffer_to_str"
                        || fn_name == "bytebuf_len"
                        || fn_name == "bytebuf_get"
                        || fn_name == "bytebuf_get_u32_le"
                        || fn_name == "bytebuf_get_i32_le")
                        && !args.is_empty()
                    {
                        // First arg doesn't escape
                        if let Expr::Ident(_, _) = &args[0] {
                            // It's fine, don't mark as escaped
                        } else {
                            self.visit_expr(&args[0]);
                        }
                        for arg in args.iter().skip(1) {
                            self.visit_expr(arg);
                        }
                        return;
                    }
                }
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            Expr::TupleLit(items, _) | Expr::ListLit(items, _) => {
                for item in items {
                    self.visit_expr(item);
                }
            }
            Expr::MapLit(entries, _) => {
                for (k, v) in entries {
                    self.visit_expr(k);
                    self.visit_expr(v);
                }
            }
            Expr::Binary(l, _, r, _) => {
                self.visit_expr(l);
                self.visit_expr(r);
            }
            Expr::Unary(_, e, _) => self.visit_expr(e),
            Expr::Pipe(l, r, _) => {
                let mut is_safe_pipe = false;
                if let Expr::Call(callee, args, _) = r.as_ref() {
                    if let Expr::Ident(fn_name, _) = callee.as_ref() {
                        if fn_name == "bitset_has"
                            || fn_name == "buffer_to_str"
                            || fn_name == "bytebuf_len"
                            || fn_name == "bytebuf_get"
                            || fn_name == "bytebuf_get_u32_le"
                            || fn_name == "bytebuf_get_i32_le"
                        {
                            is_safe_pipe = true;
                            for arg in args {
                                self.visit_expr(arg);
                            }
                        }
                    }
                }
                if is_safe_pipe {
                    if let Expr::Ident(_, _) = l.as_ref() {
                        // safe, do nothing
                    } else {
                        self.visit_expr(l);
                    }
                } else {
                    self.visit_expr(l);
                    self.visit_expr(r);
                }
            }
            Expr::Field(obj, _, _) => self.visit_expr(obj),
            Expr::Index(obj, idx, _) => {
                self.visit_expr(obj);
                self.visit_expr(idx);
            }
            Expr::ComponentExpr(_, fields, rest, _) => {
                for (_, e) in fields {
                    self.visit_expr(e);
                }
                if let Some(e) = rest {
                    self.visit_expr(e);
                }
            }
            Expr::VariantExpr(_, _, fields, _) => {
                for (_, e) in fields {
                    self.visit_expr(e);
                }
            }
            Expr::MatchExpr(m, _) => {
                self.visit_expr(&m.subject);
                for case in &m.cases {
                    if let Some(g) = &case.guard {
                        self.visit_expr(g);
                    }
                    self.visit_block(&case.body);
                }
            }
            Expr::IfExpr(c, t, e, _) => {
                self.visit_expr(c);
                self.visit_expr(t);
                self.visit_expr(e);
            }
            Expr::FnExpr(_, _, _, _, _, body, _) => {
                // Everything captured escapes
                // For simplicity, we just don't traverse into closures,
                // but any captured variable would be marked as escaped if we did.
                // Wait, if we don't traverse, we might miss an escape!
                // Let's traverse.
                self.visit_block(body);
            }
            Expr::Try(e, _) => self.visit_expr(e),
            Expr::Spread(e, _) => self.visit_expr(e),
            Expr::FStringExpr(parts, _) => {
                for part in parts {
                    if let FStringPart::Expr(e, _) = part {
                        self.visit_expr(e);
                    }
                }
            }
            Expr::Await(e, _) => self.visit_expr(e),
            Expr::AsyncCall(callee, args, _) => {
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            Expr::EntityLiteral(name, components, _) => {
                if let Some(name_expr) = name {
                    self.visit_expr(name_expr);
                }
                for entry in components {
                    match entry {
                        ComponentEntry::Init(ci) => {
                            for (_, fexpr) in &ci.fields {
                                self.visit_expr(fexpr);
                            }
                        }
                        ComponentEntry::Expr(ex) => self.visit_expr(ex),
                    }
                }
            }
            _ => {}
        }
    }
}
