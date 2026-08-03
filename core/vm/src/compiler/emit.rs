use super::*;

impl Compiler {
    pub(crate) fn current(&mut self) -> &mut FnScope {
        self.functions.last_mut().unwrap()
    }

    pub(crate) fn current_chunk(&mut self) -> &mut Chunk {
        &mut self.functions.last_mut().unwrap().chunk
    }

    pub(crate) fn emit_op(&mut self, op: Op, line: u32) {
        if matches!(op, Op::Jump | Op::JumpIfFalse | Op::JumpBack) {
            self.current().last_get_local.clear();
        }
        let at = self.current_offset();
        self.current().prev_instr_start = at;
        self.current_chunk().write_op(op, line);
    }

    /// Record that `target` is (or will become) a jump destination, so no
    /// peephole fusion may rewrite or absorb the instruction starting there.
    pub(crate) fn mark_label(&mut self, target: usize) {
        let scope = self.current();
        if target > scope.label_high_water {
            scope.label_high_water = target;
        }
    }

    /// Mark the current emission point as a future jump destination (loop
    /// heads, continue targets — anything jumped to via raw offsets).
    pub(crate) fn mark_label_here(&mut self) {
        let at = self.current_offset();
        self.mark_label(at);
    }

    pub(crate) fn emit_byte(&mut self, byte: u8, line: u32) {
        self.current_chunk().write(byte, line);
    }

    pub(crate) fn emit_u16(&mut self, val: u16, line: u32) {
        self.current_chunk().write_u16(val, line);
    }

    pub(crate) fn emit_get_local(&mut self, slot: u16, line: u32) {
        // Peephole: GetLocal a; GetLocal b  =>  GetLocal2 a b (one dispatch).
        // Only when the previous instruction is provably a GetLocal starting
        // exactly 3 bytes back, and no label points into the fusion window
        // (a jump landing on the second GetLocal would otherwise land inside
        // GetLocal2's operands).
        let len = self.current_offset();
        let getlocal2_enabled = !std::env::var("RAD_NO_GETLOCAL2").is_ok_and(|v| v == "1");
        if getlocal2_enabled && len >= 3 {
            let prev = self.current().prev_instr_start;
            // A label on the first GetLocal is fine (landing there executes
            // the fused pair — same two pushes). A label at `len` (the
            // would-be second GetLocal) would land inside operands: block.
            let safe = self.current().label_high_water < len;
            if prev == len - 3 && safe {
                let chunk = self.current_chunk();
                if chunk.code[len - 3] == Op::GetLocal as u8 {
                    chunk.code[len - 3] = Op::GetLocal2 as u8;
                    self.emit_u16(slot, line);
                    // MoveLocal rewriting must never touch a fused pair.
                    let scope = self.current();
                    scope.last_get_local.retain(|_, ip| *ip != len - 3);
                    // The current slot is now the second operand of GetLocal2,
                    // so it has no independently rewritable instruction. A
                    // stale read from an earlier expression must not survive:
                    // assignment lowering could otherwise rewrite that older
                    // read to MoveLocal and clear the slot before this fused
                    // read executes.
                    scope.last_get_local.remove(&slot);
                    scope.prev_instr_start = len - 3;
                    return;
                }
            }
        }
        self.emit_op(Op::GetLocal, line);
        let ip = self.current_offset() - 1; // IP of Op::GetLocal
        self.emit_u16(slot, line);
        self.current().last_get_local.insert(slot, ip);
    }

    pub(crate) fn add_constant(&mut self, val: Value) -> u16 {
        self.current_chunk().add_constant(val)
    }

    pub(crate) fn checked_u16(val: usize, context: &str, line: u32) -> Result<u16, CompileError> {
        if val > u16::MAX as usize {
            return Err(CompileError {
                message: format!("{}: count {} exceeds u16 max", context, val),
                line,
                col: 0,
            });
        }
        Ok(val as u16)
    }

    pub(crate) fn emit_constant(&mut self, val: Value, line: u32) {
        self.current_chunk().write_const(val, line);
    }

    /// Build a heap-allocated constant via `gc` then emit — avoids E0499 when `emit_constant` would
    /// otherwise borrow `self` for both the outer call and `&mut self.gc` inside `Value::from_*`.
    pub(crate) fn emit_constant_gc(
        &mut self,
        line: u32,
        f: impl FnOnce(&mut crate::gc::GcHeap) -> Value,
    ) {
        let v = f(&mut self.gc);
        self.emit_constant(v, line);
    }

    pub(crate) fn add_constant_gc(
        &mut self,
        f: impl FnOnce(&mut crate::gc::GcHeap) -> Value,
    ) -> u16 {
        let v = f(&mut self.gc);
        self.add_constant(v)
    }

    pub(crate) fn current_offset(&self) -> usize {
        self.functions.last().unwrap().chunk.code.len()
    }

    pub(crate) fn emit_jump(&mut self, op: Op, line: u32) -> usize {
        // Peephole: Cmp; JumpIfFalse  =>  CmpJF (one dispatch, no bool).
        // A label on the compare is fine (landing there executes the fused
        // op — identical semantics); a label at the JumpIfFalse position
        // itself would land inside the fused operand: block.
        if op == Op::JumpIfFalse {
            let len = self.current_offset();
            if len >= 1
                && self.current().prev_instr_start == len - 1
                && self.current().label_high_water < len
            {
                let prev_byte = self.current_chunk().code[len - 1];
                let fused = match prev_byte {
                    b if b == Op::Eq as u8 => Some(Op::EqJF),
                    b if b == Op::Neq as u8 => Some(Op::NeqJF),
                    b if b == Op::Lt as u8 => Some(Op::LtJF),
                    b if b == Op::Lte as u8 => Some(Op::LteJF),
                    b if b == Op::Gt as u8 => Some(Op::GtJF),
                    b if b == Op::Gte as u8 => Some(Op::GteJF),
                    _ => None,
                };
                if let Some(f) = fused {
                    self.current_chunk().code[len - 1] = f as u8;
                    self.current().last_get_local.clear();
                    self.current().prev_instr_start = len - 1;
                    let hole = self.current_offset();
                    self.emit_u16(0xFFFF, line);
                    return hole;
                }
            }
            // const-compare (3-byte instruction) + JumpIfFalse
            if len >= 3
                && self.current().prev_instr_start == len - 3
                && self.current().label_high_water < len
            {
                let prev_byte = self.current_chunk().code[len - 3];
                let fused = match prev_byte {
                    b if b == Op::EqConst as u8 => Some(Op::EqConstJF),
                    b if b == Op::NeqConst as u8 => Some(Op::NeqConstJF),
                    _ => None,
                };
                if let Some(f) = fused {
                    self.current_chunk().code[len - 3] = f as u8;
                    self.current().last_get_local.clear();
                    self.current().prev_instr_start = len - 3;
                    let hole = self.current_offset();
                    self.emit_u16(0xFFFF, line);
                    return hole;
                }
            }
        }
        self.emit_op(op, line);
        let hole = self.current_offset();
        self.emit_u16(0xFFFF, line);
        hole
    }

    pub(crate) fn patch_jump(&mut self, hole: usize) {
        let target = self.current_offset();
        self.mark_label(target);
        let chunk = &mut self.functions.last_mut().unwrap().chunk;
        chunk.code[hole] = ((target as u16) >> 8) as u8;
        chunk.code[hole + 1] = (target & 0xff) as u8;
    }

    pub(crate) fn patch_jump_to(&mut self, hole: usize, target: usize) {
        self.mark_label(target);
        let chunk = &mut self.functions.last_mut().unwrap().chunk;
        chunk.code[hole] = ((target as u16) >> 8) as u8;
        chunk.code[hole + 1] = (target & 0xff) as u8;
    }

    pub(crate) fn resolve_local(&self, name: &str) -> Option<u16> {
        let scope = self.functions.last().unwrap();
        for (i, local) in scope.locals.iter().enumerate().rev() {
            if local.name == name {
                return Some(i as u16);
            }
        }
        None
    }

    pub(crate) fn add_local(&mut self, name: String, mutable: bool) {
        let depth = self.current().scope_depth;
        self.current().locals.push(Local {
            name,
            depth,
            mutable,
            is_captured: false,
        });
    }

    pub(crate) fn is_local_mutable(&self, name: &str) -> Option<bool> {
        let scope = self.functions.last().unwrap();
        for local in scope.locals.iter().rev() {
            if local.name == name {
                return Some(local.mutable);
            }
        }
        None
    }

    pub(crate) fn begin_scope(&mut self) {
        self.current().scope_depth += 1;
    }

    pub(crate) fn end_scope(&mut self, line: u32) {
        let depth = self.current().scope_depth;
        let mut count: usize = 0;
        while self
            .current()
            .locals
            .last()
            .is_some_and(|l| l.depth == depth)
        {
            self.current().locals.pop();
            count += 1;
        }
        self.emit_pops(count, line);
        self.current().scope_depth -= 1;
    }

    /// One Pop for one value, PopN for several — scope exits inside hot
    /// loops used to pay one dispatch per local.
    pub(crate) fn emit_pops(&mut self, count: usize, line: u32) {
        let mut remaining = count;
        while remaining > 0 {
            if remaining == 1 {
                self.emit_op(Op::Pop, line);
                remaining = 0;
            } else {
                let chunk = remaining.min(255);
                self.emit_op(Op::PopN, line);
                self.emit_byte(chunk as u8, line);
                remaining -= chunk;
            }
        }
    }

    pub(crate) fn end_scope_keep_top(&mut self, line: u32) {
        let depth = self.current().scope_depth;

        let mut count = 0;
        for local in self.current().locals.iter().rev() {
            if local.depth == depth {
                count += 1;
            } else {
                break;
            }
        }

        if count > 0 {
            let first_local_idx = (self.current().locals.len() - count) as u16;

            self.emit_op(Op::SetLocal, line);
            self.emit_u16(first_local_idx, line);

            for _ in 0..(count - 1) {
                self.emit_op(Op::Pop, line);
            }

            for _ in 0..count {
                self.current().locals.pop();
            }
        }

        self.current().scope_depth -= 1;
    }

    pub(crate) fn resolve_upvalue(&mut self, fn_idx: usize, name: &str) -> Option<u16> {
        if fn_idx == 0 {
            return None;
        }
        let parent_idx = fn_idx - 1;
        if let Some(local_idx) = self.resolve_local_at(parent_idx, name) {
            self.functions[parent_idx].locals[local_idx as usize].is_captured = true;
            return Some(self.add_upvalue(fn_idx, local_idx, true));
        }
        if let Some(upvalue_idx) = self.resolve_upvalue(parent_idx, name) {
            return Some(self.add_upvalue(fn_idx, upvalue_idx, false));
        }
        None
    }

    fn resolve_local_at(&self, fn_idx: usize, name: &str) -> Option<u16> {
        let scope = &self.functions[fn_idx];
        for (i, local) in scope.locals.iter().enumerate().rev() {
            if local.name == name {
                return Some(i as u16);
            }
        }
        None
    }

    fn resolve_local_mutability_at(&self, fn_idx: usize, name: &str) -> Option<bool> {
        let scope = &self.functions[fn_idx];
        for local in scope.locals.iter().rev() {
            if local.name == name {
                return Some(local.mutable);
            }
        }
        None
    }

    pub(crate) fn resolve_captured_mutability(&self, fn_idx: usize, name: &str) -> Option<bool> {
        if fn_idx == 0 {
            return None;
        }
        let parent_idx = fn_idx - 1;
        if let Some(mutable) = self.resolve_local_mutability_at(parent_idx, name) {
            return Some(mutable);
        }
        self.resolve_captured_mutability(parent_idx, name)
    }

    pub(crate) fn add_upvalue(&mut self, fn_idx: usize, index: u16, is_local: bool) -> u16 {
        let scope = &self.functions[fn_idx];
        for (i, uv) in scope.upvalues.iter().enumerate() {
            if uv.index == index && uv.is_local == is_local {
                return i as u16;
            }
        }
        let idx = scope.upvalues.len() as u16;
        self.functions[fn_idx]
            .upvalues
            .push(Upvalue { is_local, index });
        idx
    }
}
