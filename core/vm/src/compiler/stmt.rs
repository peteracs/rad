use super::*;

impl Compiler {
    pub(crate) fn compile_body(&mut self, stmts: &[Stmt]) -> Result<(), CompileError> {
        for stmt in stmts {
            self.compile_stmt(stmt)?;
            if matches!(stmt, Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_)) {
                break;
            }
        }
        Ok(())
    }

    pub(crate) fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        match stmt {
            Stmt::Let(l) => self.compile_let(l),
            Stmt::LetElse(le) => self.compile_let_else(le),
            Stmt::Assign(a) => self.compile_assign(a),
            Stmt::If(i) => self.compile_if(i),
            Stmt::While(w) => self.compile_while(w),
            Stmt::For(f) => self.compile_for(f),
            Stmt::Return(r) => self.compile_return(r),
            Stmt::Break(b) => self.compile_break(b),
            Stmt::Continue(c) => self.compile_continue(c),
            Stmt::Emit(e) => self.compile_emit(e),
            Stmt::Schedule(s) => self.compile_schedule(s),
            Stmt::Update(u) => self.compile_update(u),
            Stmt::Settle(s) => self.compile_settle(s),
            Stmt::Propose(s) => self.compile_propose(s),
            Stmt::Next(s) => self.compile_next(s),
            Stmt::Match(m) => self.compile_match(m),
            Stmt::Expr(e) => {
                self.allow_pipe_fusion = true;
                self.compile_expr(&e.expr)?;
                self.emit_op(Op::Pop, e.span.line);
                Ok(())
            }
            Stmt::OnceGuardPass(span) => {
                self.emit_op(Op::OnceGuardPass, span.line);
                Ok(())
            }
            Stmt::Error(_) => Ok(()),
        }
    }

    fn compile_let_else(&mut self, le: &LetElseStmt) -> Result<(), CompileError> {
        let counterpart = match le.variant_name.as_str() {
            "Some" => "None",
            "Ok" => "Err",
            _ => {
                return Err(CompileError {
                    message: "let ... else: only Some or Ok variant patterns are supported".into(),
                    line: le.span.line,
                    col: le.span.col,
                });
            }
        };
        let Some(primary) = le.primary_binding_name() else {
            return Err(CompileError {
                message: "let ... else requires exactly one pattern binding".into(),
                line: le.span.line,
                col: le.span.col,
            });
        };

        let ident_span = le.span.clone();
        let some_body = Block {
            id: NodeId(0),
            span: ident_span.clone(),
            stmts: vec![Stmt::Expr(ExprStmt {
                id: NodeId(0),
                span: ident_span.clone(),
                expr: Expr::Ident(primary.clone(), ident_span),
            })],
        };
        let some_case = MatchCase {
            id: NodeId(0),
            span: le.span.clone(),
            pattern: Pattern::Variant {
                path: vec![le.variant_name.clone()],
                bindings: le.bindings.clone(),
                pattern_bindings: le.pattern_bindings.clone(),
                has_rest: le.has_rest,
                is_bare_variant: false,
            },
            guard: None,
            body: some_body,
        };
        let none_case = MatchCase {
            id: NodeId(0),
            span: le.span.clone(),
            pattern: Pattern::Variant {
                path: vec![counterpart.to_string()],
                bindings: vec![],
                pattern_bindings: vec![],
                has_rest: false,
                is_bare_variant: true,
            },
            guard: None,
            body: le.else_block.clone(),
        };
        let m = MatchStmt {
            id: le.id,
            span: le.span.clone(),
            subject: le.subject.clone(),
            cases: vec![some_case, none_case],
        };
        let let_stmt = LetStmt {
            id: le.id,
            span: le.span.clone(),
            names: vec![primary],
            tuple_destructure: false,
            mutable: le.mutable,
            recursive: false,
            is_unique: false,
            is_pub: false,
            type_annotation: le.type_annotation.clone(),
            value: Expr::MatchExpr(Box::new(m), le.span.clone()),
        };
        self.compile_let(&let_stmt)
    }

    fn compile_let(&mut self, l: &LetStmt) -> Result<(), CompileError> {
        let line = l.span.line;
        if l.recursive {
            return self.compile_let_rec(l);
        }
        if l.tuple_destructure {
            return self.compile_let_tuple_destructure(l);
        }
        debug_assert_eq!(l.names.len(), 1);
        let name = l.names[0].clone();
        self.allow_pipe_fusion = true;
        self.compile_expr(&l.value)?;
        if self.current().scope_depth > 0 {
            self.add_local(name, l.mutable);
            if l.is_unique {
                let last_local_name = self.current().locals.last().map(|local| local.name.clone());
                if let Some(local_name) = last_local_name {
                    self.current().unique_locals.insert(local_name);
                }
            }
        } else {
            self.global_mutability.insert(name.clone(), l.mutable);
            let slot = self.ensure_global_slot(&name);
            self.emit_op(Op::DefGlobal, line);
            self.emit_u16(slot, line);
        }
        Ok(())
    }

    /// `let (a, b, ...) = rhs` / `let (x) = rhs` — index each element (matches list and tuple).
    fn compile_let_tuple_destructure(&mut self, l: &LetStmt) -> Result<(), CompileError> {
        let line = l.span.line;
        if self.current().scope_depth > 0 {
            let tmp = format!("__td_{}", l.id.0);
            self.compile_expr(&l.value)?;
            self.add_local(tmp.clone(), false);
            let tmp_slot = self.resolve_local(&tmp).ok_or_else(|| CompileError {
                message: "internal: tuple destruct temp local".into(),
                line,
                col: l.span.col,
            })?;
            for (i, name) in l.names.iter().enumerate() {
                self.emit_get_local(tmp_slot, line);
                self.emit_constant_gc(line, |gc| Value::from_int(gc, i as i64));
                self.emit_op(Op::GetIndex, line);
                self.add_local(name.clone(), l.mutable);
            }
            // Overwrite the temporary local with NIL to allow the container to be garbage collected
            self.emit_constant(Value::NIL, line);
            self.emit_op(Op::SetLocal, line);
            self.emit_u16(tmp_slot, line);
        } else {
            self.compile_expr(&l.value)?;
            for (i, name) in l.names.iter().enumerate() {
                self.emit_op(Op::Dup, line);
                self.emit_constant_gc(line, |gc| Value::from_int(gc, i as i64));
                self.emit_op(Op::GetIndex, line);
                self.global_mutability.insert(name.clone(), l.mutable);
                let slot = self.ensure_global_slot(name);
                self.emit_op(Op::DefGlobal, line);
                self.emit_u16(slot, line);
            }
            self.emit_op(Op::Pop, line);
        }
        Ok(())
    }

    fn compile_let_rec(&mut self, l: &LetStmt) -> Result<(), CompileError> {
        let line = l.span.line;
        let name = l.names[0].clone();
        if self.current().scope_depth > 0 {
            // Local recursive binding:
            // 1. Push nil placeholder onto stack, register the local name
            self.emit_constant(Value::NIL, line);
            self.add_local(name.clone(), l.mutable);
            let slot = self.resolve_local(&name).unwrap();
            // 2. Compile the RHS — the closure body can now resolve the name
            //    as a local (and capture it as an upvalue / CaptureCell)
            self.compile_expr(&l.value)?;
            // 3. Overwrite the nil placeholder with the actual closure
            self.emit_op(Op::SetLocal, line);
            self.emit_u16(slot, line);
        } else {
            // Global recursive binding:
            // DefGlobal with nil first so the name exists during RHS compilation
            self.global_mutability.insert(name.clone(), l.mutable);
            let slot = self.ensure_global_slot(&name);
            self.emit_constant(Value::NIL, line);
            self.emit_op(Op::DefGlobal, line);
            self.emit_u16(slot, line);
            self.compile_expr(&l.value)?;
            self.emit_op(Op::SetGlobal, line);
            self.emit_u16(slot, line);
        }
        Ok(())
    }

    fn compile_assign(&mut self, a: &AssignStmt) -> Result<(), CompileError> {
        let line = a.span.line;
        match &a.target {
            Expr::Ident(name, _) => {
                self.ensure_assign_target_mutable(name, line, a.span.col)?;

                // Uniqueness typing / In-place mutation optimization
                let mut optimized = false;

                let check_expr = &a.value;
                // If it's a pipe, the right side is the call
                if let Expr::Pipe(left, right, _) = check_expr {
                    if let Expr::Ident(left_name, _) = left.as_ref() {
                        if left_name == name {
                            if let Expr::Call(callee, args, _) = right.as_ref() {
                                if let Expr::Ident(fn_name, _) = callee.as_ref() {
                                    let is_bytebuf_setter = fn_name == "bytebuf_set_u8"
                                        || fn_name == "bytebuf_set_u32_le"
                                        || fn_name == "bytebuf_set_i32_le";
                                    if ((fn_name == "bitset_set"
                                        || fn_name == "bitset_clear"
                                        || fn_name == "buffer_append")
                                        && args.len() == 1
                                        || is_bytebuf_setter && args.len() == 2)
                                        && self.current().unique_locals.contains(name)
                                    {
                                        self.compile_expr(left)?;
                                        for arg in args {
                                            self.compile_expr(arg)?;
                                        }
                                        if fn_name == "bitset_set" {
                                            self.emit_op(Op::BitsetSetInplace, line);
                                        } else if fn_name == "bitset_clear" {
                                            self.emit_op(Op::BitsetClearInplace, line);
                                        } else if fn_name == "buffer_append" {
                                            self.emit_op(Op::BufferAppendInplace, line);
                                        } else if fn_name == "bytebuf_set_u8" {
                                            self.emit_op(Op::ByteBufSetU8Inplace, line);
                                        } else if fn_name == "bytebuf_set_u32_le" {
                                            self.emit_op(Op::ByteBufSetU32LeInplace, line);
                                        } else if fn_name == "bytebuf_set_i32_le" {
                                            self.emit_op(Op::ByteBufSetI32LeInplace, line);
                                        }
                                        optimized = true;
                                    }
                                }
                            }
                        }
                    }
                }

                if !optimized {
                    if let Expr::Call(callee, args, _) = &a.value {
                        if let Expr::Ident(fn_name, _) = callee.as_ref() {
                            let is_bytebuf_setter = fn_name == "bytebuf_set_u8"
                                || fn_name == "bytebuf_set_u32_le"
                                || fn_name == "bytebuf_set_i32_le";
                            if ((fn_name == "bitset_set"
                                || fn_name == "bitset_clear"
                                || fn_name == "buffer_append"
                                || fn_name == "push")
                                && args.len() == 2)
                                || (is_bytebuf_setter && args.len() == 3)
                            {
                                if let Expr::Ident(arg_name, _) = &args[0] {
                                    if arg_name == name
                                        && self.current().unique_locals.contains(name)
                                    {
                                        if fn_name == "push" {
                                            if let Some(slot) = self.resolve_local(name) {
                                                self.compile_expr(&args[1])?;
                                                self.emit_op(Op::ListPushLocal, line);
                                                self.emit_u16(slot, line);
                                                return Ok(());
                                            }
                                        } else {
                                            // Compile the arguments
                                            self.compile_expr(&args[0])?;
                                            for arg in args.iter().skip(1) {
                                                self.compile_expr(arg)?;
                                            }
                                            // Emit the inplace opcode
                                            if fn_name == "bitset_set" {
                                                self.emit_op(Op::BitsetSetInplace, line);
                                            } else if fn_name == "bitset_clear" {
                                                self.emit_op(Op::BitsetClearInplace, line);
                                            } else if fn_name == "buffer_append" {
                                                self.emit_op(Op::BufferAppendInplace, line);
                                            } else if fn_name == "bytebuf_set_u8" {
                                                self.emit_op(Op::ByteBufSetU8Inplace, line);
                                            } else if fn_name == "bytebuf_set_u32_le" {
                                                self.emit_op(Op::ByteBufSetU32LeInplace, line);
                                            } else if fn_name == "bytebuf_set_i32_le" {
                                                self.emit_op(Op::ByteBufSetI32LeInplace, line);
                                            }
                                            optimized = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // `x = x + K` / `x = x - K` on a local: one IncLocal
                // dispatch instead of GetLocal+Const+Add+SetLocal — the
                // shape of every counter in hot code.
                if !optimized {
                    if let Expr::Binary(lhs, bop, rhs, _) = &a.value {
                        if matches!(bop, BinOp::Add | BinOp::Sub) {
                            if let (Expr::Ident(lname, _), Expr::IntLit(k, _)) = (&**lhs, &**rhs) {
                                if lname == name && *k != i64::MIN {
                                    if let Some(slot) = self.resolve_local(name) {
                                        let k = if *bop == BinOp::Sub { -*k } else { *k };
                                        let idx = self.add_constant_gc(|gc| Value::from_int(gc, k));
                                        self.emit_op(Op::IncLocal, line);
                                        self.emit_u16(slot, line);
                                        self.emit_u16(idx, line);
                                        self.current().last_get_local.remove(&slot);
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }
                }

                if !optimized {
                    // ident target: nothing on the stack before the value
                    self.allow_pipe_fusion = true;
                    self.compile_expr(&a.value)?;
                }

                if let Some(slot) = self.resolve_local(name) {
                    if let Some(ip) = self.current().last_get_local.get(&slot).cloned() {
                        if !self.current().locals[slot as usize].is_captured {
                            self.current_chunk().code[ip] = Op::MoveLocal as u8;
                        }
                    }
                    self.emit_op(Op::SetLocal, line);
                    self.emit_u16(slot, line);
                    self.current().last_get_local.remove(&slot);
                } else {
                    let fn_idx = self.functions.len() - 1;
                    if fn_idx > 0 {
                        if let Some(uv_idx) = self.resolve_upvalue(fn_idx, name) {
                            self.emit_op(Op::SetUpvalue, line);
                            self.emit_u16(uv_idx, line);
                            return Ok(());
                        }
                    }
                    let slot = self.ensure_global_slot(name);
                    self.emit_op(Op::SetGlobal, line);
                    self.emit_u16(slot, line);
                }
            }
            Expr::Field(obj, field, _) => {
                if let Some(root) = Self::root_ident(obj) {
                    self.ensure_assign_target_mutable(root, line, a.span.col)?;
                }
                self.compile_expr(obj)?;
                self.compile_expr(&a.value)?;
                let field_idx = self.add_constant_gc(|gc| Value::from_string(gc, field.clone()));
                self.emit_op(Op::SetField, line);
                self.emit_u16(field_idx, line);
                self.emit_container_assign_writeback(obj, line)?;
            }
            Expr::Index(obj, idx, _) => {
                if let Some(root) = Self::root_ident(obj) {
                    self.ensure_assign_target_mutable(root, line, a.span.col)?;
                }
                // `xs[i] = v` on a `let unique` local: mutate in the slot.
                // The generic path pushes the list to the stack (Arc count 2),
                // so SetIndex's copy-on-write cloned the whole list on every
                // single indexed write. Uniqueness guarantees no aliases, so
                // in-place is observationally identical — and O(1).
                if let Expr::Ident(name, _) = obj.as_ref() {
                    if self.current().unique_locals.contains(name) {
                        if let Some(slot) = self.resolve_local(name) {
                            self.compile_expr(idx)?;
                            self.compile_expr(&a.value)?;
                            self.emit_op(Op::ListSetLocal, line);
                            self.emit_u16(slot, line);
                            return Ok(());
                        }
                    }
                }
                self.compile_expr(obj)?;
                self.compile_expr(idx)?;
                self.compile_expr(&a.value)?;
                self.emit_op(Op::SetIndex, line);
                self.emit_container_assign_writeback(obj, line)?;
            }
            _ => {
                return Err(CompileError {
                    message: "Invalid assignment target".into(),
                    line,
                    col: a.span.col,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn emit_container_assign_writeback(
        &mut self,
        obj: &Expr,
        line: u32,
    ) -> Result<(), CompileError> {
        match obj {
            Expr::Ident(name, _) => self.emit_assign_back_to_name(name, line),
            Expr::Index(parent, idx, _) => {
                let temp_slot = self.bind_top_to_temp_local();
                self.compile_expr(parent)?;
                self.compile_expr(idx)?;
                self.emit_get_local(temp_slot, line);
                self.emit_op(Op::SetIndex, line);
                self.emit_op(Op::SetLocal, line);
                self.emit_u16(temp_slot, line);
                self.current().locals.pop();
                self.emit_container_assign_writeback(parent, line)?;
            }
            Expr::Field(parent, field, _) => {
                let temp_slot = self.bind_top_to_temp_local();
                self.compile_expr(parent)?;
                self.emit_get_local(temp_slot, line);
                let field_idx = self.add_constant_gc(|gc| Value::from_string(gc, field.clone()));
                self.emit_op(Op::SetField, line);
                self.emit_u16(field_idx, line);
                self.emit_op(Op::SetLocal, line);
                self.emit_u16(temp_slot, line);
                self.current().locals.pop();
                self.emit_container_assign_writeback(parent, line)?;
            }
            _ => {
                self.emit_op(Op::Pop, line);
            }
        }
        Ok(())
    }

    pub(crate) fn emit_assign_back_to_name(&mut self, name: &str, line: u32) {
        if let Some(slot) = self.resolve_local(name) {
            if let Some(ip) = self.current().last_get_local.get(&slot).cloned() {
                if !self.current().locals[slot as usize].is_captured {
                    self.current_chunk().code[ip] = Op::MoveLocal as u8;
                }
            }
            self.emit_op(Op::SetLocal, line);
            self.emit_u16(slot, line);
            self.current().last_get_local.remove(&slot);
        } else {
            let fn_idx = self.functions.len() - 1;
            if fn_idx > 0 {
                if let Some(uv_idx) = self.resolve_upvalue(fn_idx, name) {
                    self.emit_op(Op::SetUpvalue, line);
                    self.emit_u16(uv_idx, line);
                    return;
                }
            }
            let slot = self.ensure_global_slot(name);
            self.emit_op(Op::SetGlobal, line);
            self.emit_u16(slot, line);
        }
    }

    pub(crate) fn bind_top_to_temp_local(&mut self) -> u16 {
        let tmp = self.fresh_name("assign_tmp_");
        self.add_local(tmp.clone(), true);
        self.resolve_local(&tmp)
            .expect("temporary local must resolve")
    }

    pub(crate) fn ensure_assign_target_mutable(
        &mut self,
        name: &str,
        line: u32,
        col: u32,
    ) -> Result<(), CompileError> {
        if let Some(false) = self.is_local_mutable(name) {
            return Err(CompileError {
                message: format!(
                    "Cannot assign to immutable variable '{}' — change the binding to `let mut {}` (or avoid reassignment)",
                    name,
                    name
                ),
                line,
                col,
            });
        }
        if self.resolve_local(name).is_none() {
            let fn_idx = self.functions.len() - 1;
            if fn_idx > 0 && self.resolve_upvalue(fn_idx, name).is_some() {
                if matches!(self.resolve_captured_mutability(fn_idx, name), Some(false)) {
                    return Err(CompileError {
                        message: format!(
                            "Cannot assign to immutable variable '{}' — change the binding to `let mut {}` (or avoid reassignment)",
                            name,
                            name
                        ),
                        line,
                        col,
                    });
                }
            } else if let Some(false) = self.global_mutability.get(name).copied() {
                return Err(CompileError {
                    message: format!(
                        "Cannot assign to immutable variable '{}' — change the binding to `let mut {}` (or avoid reassignment)",
                        name,
                        name
                    ),
                    line,
                    col,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn root_ident(expr: &Expr) -> Option<&str> {
        match expr {
            Expr::Ident(name, _) => Some(name.as_str()),
            Expr::Field(obj, _, _) => Self::root_ident(obj),
            Expr::Index(obj, _, _) => Self::root_ident(obj),
            _ => None,
        }
    }

    fn compile_if(&mut self, i: &IfStmt) -> Result<(), CompileError> {
        let line = i.span.line;
        self.compile_expr(&i.condition)?;
        let else_jump = self.emit_jump(Op::JumpIfFalse, line);

        self.begin_scope();
        self.compile_body(&i.then_block.stmts)?;
        self.end_scope(line);

        if let Some(else_block) = &i.else_block {
            let end_jump = self.emit_jump(Op::Jump, line);
            self.patch_jump(else_jump);
            self.begin_scope();
            self.compile_body(&else_block.stmts)?;
            self.end_scope(line);
            self.patch_jump(end_jump);
        } else {
            self.patch_jump(else_jump);
        }
        Ok(())
    }

    fn compile_while(&mut self, w: &WhileStmt) -> Result<(), CompileError> {
        let line = w.span.line;
        let loop_start = self.current_offset();
        self.mark_label_here();

        let loop_depth = self.current().scope_depth;
        self.current().loop_contexts.push(LoopCtx {
            loop_depth,
            loop_start,
            break_holes: Vec::new(),
            continue_holes: Vec::new(),
            writebacks: Vec::new(),
        });

        self.compile_expr(&w.condition)?;
        let exit_jump = self.emit_jump(Op::JumpIfFalse, line);

        self.begin_scope();
        self.compile_body(&w.body.stmts)?;
        self.end_scope(line);

        let loop_end = self.current_offset();
        let delta = loop_end - loop_start + 3;
        self.emit_op(Op::JumpBack, line);
        self.emit_u16(delta as u16, line);

        self.patch_jump(exit_jump);
        let ctx = self.current().loop_contexts.pop().unwrap();
        for hole in ctx.break_holes {
            self.patch_jump(hole);
        }
        for hole in ctx.continue_holes {
            self.patch_jump_to(hole, ctx.loop_start);
        }
        Ok(())
    }

    fn compile_for(&mut self, f: &ForStmt) -> Result<(), CompileError> {
        if let Expr::QueryExpr(q, _) = &f.iterable {
            let has_mut = q.components.iter().any(|(_, is_mut)| *is_mut);
            let is_special_loop = has_mut || f.bindings.len() > 1;
            if is_special_loop {
                return self.compile_for_query_unpack(f, q);
            }
        }

        // `for i in range(lo, hi)` compiles as a counted loop: no list is
        // materialized and the per-iteration element load disappears. The
        // op profile of a bitboard solver showed range scaffolding (range
        // allocation + Len + GetIndex per iteration) as a top-3 cost.
        if f.bindings.len() == 1 && f.destructure_bindings.is_none() {
            if let Expr::Call(callee, args, _) = &f.iterable {
                if let Expr::Ident(fn_name, _) = callee.as_ref() {
                    if fn_name == "range"
                        && (args.len() == 1 || args.len() == 2)
                        && self.resolve_local("range").is_none()
                    {
                        return self.compile_for_counted_range(f, args);
                    }
                }
            }
        }

        let line = f.span.line;

        // `for (a, b, c) in rows` over a LIST is tuple destructure (the
        // checker typed it that way) — normalize to the single-binding +
        // destructure form the rest of this function already handles.
        let normalized;
        let f = if f.bindings.len() >= 2
            && f.destructure_bindings.is_none()
            && matches!(self.for_iter_kinds.get(&f.id), Some(ForIterKind::List))
        {
            let mut g = f.clone();
            g.destructure_bindings = Some(g.bindings.clone());
            g.bindings = vec![self.fresh_name("for_tuple")];
            normalized = g;
            &normalized
        } else {
            f
        };

        self.begin_scope();

        let list_name = self.fresh_name("list");
        let idx_name = self.fresh_name("idx");

        self.compile_expr(&f.iterable)?;
        self.add_local(list_name.clone(), false);
        let list_slot = self.resolve_local(&list_name).unwrap();

        let iter_hint = if f.bindings.len() == 2 {
            ForIterKind::Map // Force map iteration if 2 bindings
        } else {
            self.for_iter_kinds
                .get(&f.id)
                .copied()
                .unwrap_or(ForIterKind::Unknown)
        };

        match iter_hint {
            ForIterKind::Map => {
                if f.bindings.len() == 2 {
                    self.emit_get_local(list_slot, line);
                    self.emit_op(Op::GetIter, line);
                    self.emit_op(Op::SetLocal, line);
                    self.emit_u16(list_slot, line);
                } else {
                    self.emit_get_local(list_slot, line);
                    let keys_slot = self.ensure_global_slot("keys");
                    self.emit_op(Op::GetGlobal, line);
                    self.emit_u16(keys_slot, line);
                    self.emit_op(Op::Call, line);
                    self.emit_byte(1, line);
                    self.emit_op(Op::SetLocal, line);
                    self.emit_u16(list_slot, line);
                }
            }
            ForIterKind::List | ForIterKind::Str => {}
            ForIterKind::Unknown => {
                self.emit_get_local(list_slot, line);
                let typeof_slot = self.ensure_global_slot("typeof");
                self.emit_op(Op::GetGlobal, line);
                self.emit_u16(typeof_slot, line);
                self.emit_op(Op::Call, line);
                self.emit_byte(1, line);
                self.emit_constant_gc(line, |gc| Value::from_string(gc, "map".to_string()));
                self.emit_op(Op::Eq, line);
                let not_map = self.emit_jump(Op::JumpIfFalse, line);

                self.emit_get_local(list_slot, line);
                let keys_slot = self.ensure_global_slot("keys");
                self.emit_op(Op::GetGlobal, line);
                self.emit_u16(keys_slot, line);
                self.emit_op(Op::Call, line);
                self.emit_byte(1, line);
                self.emit_op(Op::SetLocal, line);
                self.emit_u16(list_slot, line);
                self.patch_jump(not_map);
            }
        }

        let idx_slot = if f.bindings.len() == 1 {
            self.emit_constant_gc(line, |gc| Value::from_int(gc, 0));
            self.add_local(idx_name.clone(), true);
            Some(self.resolve_local(&idx_name).unwrap())
        } else {
            None
        };

        let loop_start = self.current_offset();
        self.mark_label_here();
        let loop_depth = self.current().scope_depth;
        self.current().loop_contexts.push(LoopCtx {
            loop_depth,
            loop_start,
            break_holes: Vec::new(),
            continue_holes: Vec::new(),
            writebacks: Vec::new(),
        });

        let exit_jump = if f.bindings.len() == 2 {
            self.emit_get_local(list_slot, line);
            self.emit_op(Op::IterNext, line);
            self.emit_byte(2, line);
            self.emit_jump(Op::JumpIfFalse, line)
        } else {
            self.emit_get_local(idx_slot.unwrap(), line);
            self.emit_get_local(list_slot, line);
            self.emit_op(Op::Len, line);
            self.emit_op(Op::Lt, line);
            self.emit_jump(Op::JumpIfFalse, line)
        };

        self.begin_scope();

        if f.bindings.len() == 1 {
            self.emit_get_local(list_slot, line);
            self.emit_get_local(idx_slot.unwrap(), line);
            self.emit_op(Op::GetIndex, line);
            self.add_local(f.bindings[0].clone(), false);
            if let Some(names) = &f.destructure_bindings {
                let src_slot = self
                    .resolve_local(&f.bindings[0])
                    .ok_or_else(|| CompileError {
                        message: "internal: for destructure source local".to_string(),
                        line,
                        col: f.span.col,
                    })?;
                for (i, name) in names.iter().enumerate() {
                    self.emit_get_local(src_slot, line);
                    self.emit_constant_gc(line, |gc| Value::from_int(gc, i as i64));
                    self.emit_op(Op::GetIndex, line);
                    self.add_local(name.clone(), false);
                }
                self.emit_constant(Value::NIL, line);
                self.emit_op(Op::SetLocal, line);
                self.emit_u16(src_slot, line);
            }
        } else if f.bindings.len() == 2 {
            // IterNext pushed `key`, `value`, `has_next`.
            // JumpIfFalse popped `has_next`.
            // So `value` is on top, `key` is below it.
            // We need to add them as locals.
            // Since the stack grows upwards, `key` was pushed first, then `value`.
            // So `key` is at `stack_top - 2`, `value` is at `stack_top - 1`.
            self.add_local(f.bindings[0].clone(), false);
            self.add_local(f.bindings[1].clone(), false);
        }

        self.compile_body(&f.body.stmts)?;
        self.end_scope(line);

        let continue_target = self.current_offset();
        self.mark_label_here();
        if f.bindings.len() == 1 {
            self.emit_get_local(idx_slot.unwrap(), line);
            self.emit_constant_gc(line, |gc| Value::from_int(gc, 1));
            self.emit_op(Op::Add, line);
            self.emit_op(Op::SetLocal, line);
            self.emit_u16(idx_slot.unwrap(), line);
        }

        let loop_end = self.current_offset();
        let delta = loop_end - loop_start + 3;
        self.emit_op(Op::JumpBack, line);
        self.emit_u16(delta as u16, line);

        self.patch_jump(exit_jump);
        let ctx = self.current().loop_contexts.pop().unwrap();
        for hole in ctx.break_holes {
            self.patch_jump(hole);
        }
        for hole in ctx.continue_holes {
            self.patch_jump_to(hole, continue_target);
        }

        self.end_scope(line);
        Ok(())
    }

    /// `for i in range(...)` as a counted loop over two hidden int locals —
    /// no list allocation, no Len, no GetIndex. Semantics match the list
    /// form: bounds are evaluated once, the binding is fresh per iteration.
    fn compile_for_counted_range(
        &mut self,
        f: &ForStmt,
        args: &[Expr],
    ) -> Result<(), CompileError> {
        let line = f.span.line;
        self.begin_scope();

        let cur_name = self.fresh_name("range_cur");
        let end_name = self.fresh_name("range_end");

        if args.len() == 2 {
            self.compile_expr(&args[0])?;
        } else {
            self.emit_constant_gc(line, |gc| Value::from_int(gc, 0));
        }
        self.add_local(cur_name.clone(), true);
        let cur_slot = self.resolve_local(&cur_name).unwrap();

        self.compile_expr(args.last().unwrap())?;
        self.add_local(end_name.clone(), false);
        let end_slot = self.resolve_local(&end_name).unwrap();

        // entry guard: run the body only if cur < end to begin with
        self.emit_get_local(cur_slot, line);
        self.emit_get_local(end_slot, line);
        self.emit_op(Op::Lt, line);
        let exit_jump = self.emit_jump(Op::JumpIfFalse, line);

        // loop rotation: the back-edge test lives in ForRangeNext at the
        // bottom, so the body start is the jump-back target.
        let body_start = self.current_offset();
        self.mark_label_here();
        let loop_depth = self.current().scope_depth;
        self.current().loop_contexts.push(LoopCtx {
            loop_depth,
            loop_start: body_start,
            break_holes: Vec::new(),
            continue_holes: Vec::new(),
            writebacks: Vec::new(),
        });

        self.begin_scope();
        self.emit_get_local(cur_slot, line);
        self.add_local(f.bindings[0].clone(), false);
        self.compile_body(&f.body.stmts)?;
        self.end_scope(line);

        let continue_target = self.current_offset();
        self.mark_label_here();
        // ip after opcode byte + three u16 operands:
        let after_operands = self.current_offset() + 7;
        let delta = after_operands - body_start;
        self.emit_op(Op::ForRangeNext, line);
        self.emit_u16(cur_slot, line);
        self.emit_u16(end_slot, line);
        self.emit_u16(delta as u16, line);

        self.patch_jump(exit_jump);
        let ctx = self.current().loop_contexts.pop().unwrap();
        for hole in ctx.break_holes {
            self.patch_jump(hole);
        }
        for hole in ctx.continue_holes {
            self.patch_jump_to(hole, continue_target);
        }

        self.end_scope(line);
        Ok(())
    }

    fn compile_for_query_unpack(
        &mut self,
        f: &ForStmt,
        q: &crate::ast::QueryExprNode,
    ) -> Result<(), CompileError> {
        let line = f.span.line;
        self.begin_scope();

        let list_name = self.fresh_name("query_list");
        let idx_name = self.fresh_name("query_idx");

        let (without_types, remaining_filter) = if let Some(filter_expr) = &q.filter {
            Compiler::extract_query_negations(filter_expr)
        } else {
            (Vec::new(), None)
        };

        for (comp, _) in &q.components {
            let resolved = self.resolve_canonical_name(comp);
            self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
        }
        for comp in &without_types {
            let resolved = self.resolve_canonical_name(comp);
            self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
        }
        self.emit_op(Op::EcsQuery, line);
        self.emit_byte(q.components.len() as u8, line);
        self.emit_byte(without_types.len() as u8, line);

        if let Some(filter_expr) = &remaining_filter {
            let filter_scope = Compiler::new_fn_scope("__query_filter_mut");
            self.functions.push(filter_scope);
            self.add_local("__entity".to_string(), false);
            for (comp, _) in &q.components {
                self.add_local(comp.clone(), false);
            }
            self.compile_expr(filter_expr)?;
            self.emit_op(Op::Return, line);
            let filter_fn = self.functions.pop().unwrap();
            let filter_chunk_id = self.chunks.len() + 1;
            let upvalues = filter_fn.upvalues;
            self.chunks.push(filter_fn.chunk);

            for (comp, _) in &q.components {
                let resolved = self.resolve_canonical_name(comp);
                self.emit_constant_gc(line, |gc| Value::from_string(gc, resolved));
            }

            // QueryFilter pops a fn/closure value — mirror the non-mut
            // query path in expr.rs. (This used to push the chunk id as a
            // bare int constant, which died at runtime with "QueryFilter:
            // expected closure or function" for every `mut` + `where` query.)
            if upvalues.is_empty() {
                let fn_val = Value::from_fn(
                    &mut self.gc,
                    crate::value::FnValue {
                        name: "__query_filter_mut".to_string(),
                        arity: (q.components.len() + 1) as u8,
                        chunk_id: filter_chunk_id,
                    },
                );
                self.emit_constant(fn_val, line);
            } else {
                self.emit_op(Op::Closure, line);
                self.emit_u16(filter_chunk_id as u16, line);
                self.emit_byte((q.components.len() + 1) as u8, line);
                self.emit_byte(upvalues.len() as u8, line);
                for uv in &upvalues {
                    self.emit_byte(if uv.is_local { 1 } else { 0 }, line);
                    self.emit_u16(uv.index, line);
                }
            }

            self.emit_op(Op::QueryFilter, line);
            self.emit_byte(q.components.len() as u8, line);
        }

        self.add_local(list_name.clone(), false);
        let list_slot = self.resolve_local(&list_name).unwrap();

        self.emit_constant_gc(line, |gc| Value::from_int(gc, 0));
        self.add_local(idx_name.clone(), true);
        let idx_slot = self.resolve_local(&idx_name).unwrap();

        let loop_start = self.current_offset();
        self.mark_label_here();
        let loop_depth = self.current().scope_depth;
        self.current().loop_contexts.push(LoopCtx {
            loop_depth,
            loop_start,
            break_holes: Vec::new(),
            continue_holes: Vec::new(),
            writebacks: Vec::new(),
        });

        self.emit_get_local(idx_slot, line);
        self.emit_get_local(list_slot, line);
        self.emit_op(Op::Len, line);
        self.emit_op(Op::Lt, line);
        let exit_jump = self.emit_jump(Op::JumpIfFalse, line);

        self.begin_scope(); // Scope 1: entity_slot

        // Push entity id onto stack as a local
        self.emit_get_local(list_slot, line);
        self.emit_get_local(idx_slot, line);
        self.emit_op(Op::GetIndex, line);

        let mut writebacks = Vec::new();

        let (entity_slot, bind_offset) = if f.bindings.len() == q.components.len() + 1 {
            // User wants entity id as first binding
            self.add_local(f.bindings[0].clone(), false);
            let slot = self.resolve_local(&f.bindings[0]).unwrap();
            (slot, 1)
        } else {
            // Hidden entity id local
            let entity_id_name = self.fresh_name("entity_id");
            self.add_local(entity_id_name.clone(), false);
            let slot = self.resolve_local(&entity_id_name).unwrap();
            (slot, 0)
        };

        let mut skip_jumps = Vec::new();
        for (comp, _) in &q.components {
            self.emit_get_local(entity_slot, line);
            let resolved = self.resolve_canonical_name(comp);
            let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, resolved));
            self.emit_op(Op::EcsHas, line);
            self.emit_u16(type_idx, line);
            skip_jumps.push(self.emit_jump(Op::JumpIfFalse, line));
        }

        self.begin_scope(); // Scope 2: components

        for (i, (comp, is_mut)) in q.components.iter().enumerate() {
            let bind_name = &f.bindings[i + bind_offset];
            self.emit_get_local(entity_slot, line);
            let resolved = self.resolve_canonical_name(comp);
            let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, resolved));
            self.emit_op(Op::EcsGet, line);
            self.emit_u16(type_idx, line);
            self.add_local(bind_name.clone(), *is_mut);
            let comp_slot = self.resolve_local(bind_name).unwrap();
            if *is_mut {
                writebacks.push((entity_slot, comp_slot));
            }
        }

        self.current().loop_contexts.last_mut().unwrap().writebacks = writebacks.clone();

        self.compile_body(&f.body.stmts)?;

        // Perform writebacks for normal fallthrough
        for &(e_slot, c_slot) in &writebacks {
            self.emit_get_local(e_slot, line);
            self.emit_get_local(c_slot, line);
            self.emit_op(Op::EcsSet, line);
        }

        self.end_scope(line); // End Scope 2

        for jump in skip_jumps {
            self.patch_jump(jump);
        }

        self.end_scope(line); // End Scope 1

        let continue_target = self.current_offset();
        self.mark_label_here();

        self.emit_get_local(idx_slot, line);
        self.emit_constant_gc(line, |gc| Value::from_int(gc, 1));
        self.emit_op(Op::Add, line);
        self.emit_op(Op::SetLocal, line);
        self.emit_u16(idx_slot, line);

        let loop_end = self.current_offset();
        let delta = loop_end - loop_start + 3;
        self.emit_op(Op::JumpBack, line);
        self.emit_u16(delta as u16, line);

        self.patch_jump(exit_jump);
        let ctx = self.current().loop_contexts.pop().unwrap();
        for hole in ctx.break_holes {
            self.patch_jump(hole);
        }
        for hole in ctx.continue_holes {
            self.patch_jump_to(hole, continue_target);
        }

        self.end_scope(line);
        Ok(())
    }

    fn compile_return(&mut self, r: &ReturnStmt) -> Result<(), CompileError> {
        let line = r.span.line;
        if let Some(val) = &r.value {
            self.allow_pipe_fusion = true;
            self.compile_expr(val)?;
        } else {
            self.emit_constant(Value::NIL, line);
        }

        // Perform writebacks for all active loops before returning
        let mut all_writebacks = Vec::new();
        for ctx in &self.current().loop_contexts {
            all_writebacks.extend(ctx.writebacks.clone());
        }
        if !all_writebacks.is_empty() {
            // The return value is currently on top of the stack. We need to save it, do writebacks, and push it back.
            let ret_val_name = self.fresh_name("ret_val");
            self.add_local(ret_val_name.clone(), false);
            let ret_slot = self.resolve_local(&ret_val_name).unwrap();

            for (e_slot, c_slot) in all_writebacks {
                self.emit_get_local(e_slot, line);
                self.emit_get_local(c_slot, line);
                self.emit_op(Op::EcsSet, line);
            }

            self.emit_get_local(ret_slot, line);
        }

        self.emit_op(Op::Return, line);
        Ok(())
    }

    fn compile_break(&mut self, b: &BreakStmt) -> Result<(), CompileError> {
        let line = b.span.line;
        let loop_depth = self
            .current()
            .loop_contexts
            .last()
            .map(|ctx| ctx.loop_depth)
            .ok_or_else(|| CompileError {
                message: "'break' used outside of a loop".into(),
                line,
                col: b.span.col,
            })?;

        // Perform writebacks for the current loop before breaking
        let writebacks = self
            .current()
            .loop_contexts
            .last()
            .unwrap()
            .writebacks
            .clone();
        for (e_slot, c_slot) in writebacks {
            self.emit_get_local(e_slot, line);
            self.emit_get_local(c_slot, line);
            self.emit_op(Op::EcsSet, line);
        }

        let pop_count = self
            .current()
            .locals
            .iter()
            .rev()
            .take_while(|l| l.depth > loop_depth)
            .count();

        self.emit_pops(pop_count, line);

        let hole = self.emit_jump(Op::Jump, line);
        self.current()
            .loop_contexts
            .last_mut()
            .unwrap()
            .break_holes
            .push(hole);
        Ok(())
    }

    fn compile_continue(&mut self, c: &ContinueStmt) -> Result<(), CompileError> {
        let line = c.span.line;
        let loop_depth = self
            .current()
            .loop_contexts
            .last()
            .map(|ctx| ctx.loop_depth)
            .ok_or_else(|| CompileError {
                message: "'continue' used outside of a loop".into(),
                line,
                col: c.span.col,
            })?;

        // Perform writebacks for the current loop before continuing
        let writebacks = self
            .current()
            .loop_contexts
            .last()
            .unwrap()
            .writebacks
            .clone();
        for (e_slot, c_slot) in writebacks {
            self.emit_get_local(e_slot, line);
            self.emit_get_local(c_slot, line);
            self.emit_op(Op::EcsSet, line);
        }

        let pop_count = self
            .current()
            .locals
            .iter()
            .rev()
            .take_while(|l| l.depth > loop_depth)
            .count();

        self.emit_pops(pop_count, line);

        let hole = self.emit_jump(Op::Jump, line);
        self.current()
            .loop_contexts
            .last_mut()
            .unwrap()
            .continue_holes
            .push(hole);
        Ok(())
    }

    fn compile_emit(&mut self, e: &EmitStmt) -> Result<(), CompileError> {
        let line = e.span.line;
        let resolved_event = self.resolve_canonical_name(&e.event_name);
        let name_idx = self.add_constant_gc(|gc| Value::from_string(gc, resolved_event.clone()));
        let field_count = e.fields.len() as u16;
        if let Some(delay) = &e.delay {
            self.compile_expr(delay)?;
        }
        for (_, fexpr) in &e.fields {
            self.compile_expr(fexpr)?;
        }
        self.emit_op(Op::MakeCompSlot, line);
        self.emit_u16(name_idx, line);
        self.emit_u16(field_count, line);
        if e.delay.is_some() {
            self.emit_op(Op::EmitAfter, line);
        } else {
            self.emit_op(Op::Emit, line);
        }
        Ok(())
    }

    fn compile_schedule(&mut self, s: &ScheduleStmt) -> Result<(), CompileError> {
        let line = s.span.line;
        let mut expanded = Vec::new();
        for sys in &s.systems {
            if let Some(phase_systems) = self.phases.get(sys).cloned() {
                expanded.extend(phase_systems);
            } else {
                expanded.push(sys.clone());
            }
        }
        let count = Self::checked_u16(expanded.len(), "schedule", line)?;
        // `schedule serial [...]` shares RunSchedule's operand layout but
        // runs one system at a time in topological order (dogfood seq 83).
        self.emit_op(
            if s.serial {
                Op::RunScheduleSerial
            } else {
                Op::RunSchedule
            },
            line,
        );
        self.emit_u16(count, line);
        for sys in &expanded {
            let resolved_sys = self.resolve_canonical_name(sys);
            let name_idx = self.add_constant_gc(|gc| Value::system_ref(gc, resolved_sys));
            self.emit_u16(name_idx, line);
        }
        Ok(())
    }

    fn compile_update(&mut self, u: &UpdateStmt) -> Result<(), CompileError> {
        let span = &u.span;
        let line = span.line;

        self.begin_scope();

        let comp_name_expr = Expr::Ident(u.comp_name.clone(), span.clone());
        let mut ent_ref_for_set: Option<Expr> = None;
        let get_call = if let Some(entity_expr) = &u.entity_expr {
            self.compile_expr(entity_expr)?;
            let ent_tmp = self.fresh_name("update_ent_");
            self.add_local(ent_tmp.clone(), false);
            let ent_ref = Expr::Ident(ent_tmp, span.clone());
            ent_ref_for_set = Some(ent_ref.clone());
            Expr::Call(
                Box::new(Expr::Ident("get".to_string(), span.clone())),
                vec![ent_ref, comp_name_expr.clone()],
                span.clone(),
            )
        } else {
            Expr::Call(
                Box::new(Expr::Ident("get_resource".to_string(), span.clone())),
                vec![comp_name_expr.clone()],
                span.clone(),
            )
        };
        let unwrap_call = Expr::Call(
            Box::new(Expr::Ident("unwrap".to_string(), span.clone())),
            vec![get_call],
            span.clone(),
        );
        // Fold the update entries into one expression per field, in written
        // order. Indexed entries (`vals[i] = x`) become `set_at(base, i, x)`
        // where base is the previous entry for that field, or the current
        // component value when the indexed write is the first mention.
        let mut folded: Vec<(String, Expr)> = Vec::new();
        for fu in &u.field_updates {
            let next = match &fu.index {
                None => fu.value.clone(),
                Some(idx) => {
                    let base = match folded.iter().find(|(n, _)| *n == fu.name) {
                        Some((_, prev)) => prev.clone(),
                        None => Expr::Field(
                            Box::new(unwrap_call.clone()),
                            fu.name.clone(),
                            span.clone(),
                        ),
                    };
                    Expr::Call(
                        Box::new(Expr::Ident("set_at".to_string(), span.clone())),
                        vec![base, idx.clone(), fu.value.clone()],
                        span.clone(),
                    )
                }
            };
            match folded.iter_mut().find(|(n, _)| *n == fu.name) {
                Some((_, slot)) => *slot = next,
                None => folded.push((fu.name.clone(), next)),
            }
        }
        let comp_expr = Expr::ComponentExpr(
            u.comp_name.clone(),
            folded,
            Some(Box::new(unwrap_call)),
            span.clone(),
        );
        let set_call = if let Some(ent_ref) = ent_ref_for_set {
            Expr::Call(
                Box::new(Expr::Ident("set".to_string(), span.clone())),
                vec![ent_ref, comp_expr],
                span.clone(),
            )
        } else {
            Expr::Call(
                Box::new(Expr::Ident("set_resource".to_string(), span.clone())),
                vec![comp_name_expr, comp_expr],
                span.clone(),
            )
        };
        self.compile_expr(&set_call)?;
        self.emit_op(Op::Pop, line);

        self.end_scope(line);
        Ok(())
    }

    fn compile_match(&mut self, m: &MatchStmt) -> Result<(), CompileError> {
        let line = m.span.line;
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
                col: m.span.col,
            })?;

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
                    let variant_name = path.last().unwrap();
                    let pattern_idx =
                        self.add_constant_gc(|gc| Value::from_string(gc, variant_name.clone()));
                    self.emit_op(Op::MatchState, line);
                    self.emit_u16(pattern_idx, line);
                    let hole = self.current_offset();
                    self.emit_u16(0xFFFF, line);
                    Some(hole)
                }
                Pattern::HasComponent { component, .. } => {
                    self.emit_get_local(subject_slot, line);
                    let comp_idx =
                        self.add_constant_gc(|gc| Value::from_string(gc, component.clone()));
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
                let comp_idx = self.add_constant_gc(|gc| Value::from_string(gc, component.clone()));
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

            self.compile_body(&case.body.stmts)?;
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
        Ok(())
    }
}
