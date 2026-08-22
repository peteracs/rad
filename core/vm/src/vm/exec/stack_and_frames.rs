// Interpreter primitives: operand-stack access, call frames, bytecode reads,
// runtime error construction, and re-entrant calls into a value.

impl VM {

    #[inline(always)]
    pub(crate) fn peek(&self) -> Result<&Value, String> {
        self.stack
            .last()
            .ok_or_else(|| "stack underflow".to_string())
    }
    #[inline(always)]
    pub(crate) fn pop(&mut self) -> Result<Value, String> {
        self.stack
            .pop()
            .ok_or_else(|| "stack underflow".to_string())
    }
    #[inline(always)]
    pub(crate) fn push(&mut self, val: Value) {
        self.stack.push(val);
    }

    /// Build a list value on the GC heap and push it (avoids overlapping `&mut self` borrows).
    #[inline(always)]
    pub(crate) fn push_list_vec(&mut self, items: Vec<Value>) {
        let v = Value::list(&mut self.gc, items);
        self.push(v);
    }

    #[inline(always)]
    pub(crate) fn current_frame(&self) -> &CallFrame {
        self.frames
            .last()
            .expect("VM invariant violated: current_frame called with no frames")
    }
    #[inline(always)]
    pub(crate) fn current_frame_mut(&mut self) -> &mut CallFrame {
        self.frames
            .last_mut()
            .expect("VM invariant violated: current_frame_mut called with no frames")
    }
    #[inline(always)]
    pub(crate) fn current_chunk(&self) -> &Chunk {
        self.chunks
            .get(self.current_frame().chunk_id)
            .expect("VM invariant violated: frame chunk_id out of bounds")
    }

    pub(crate) fn runtime_error(&self, msg: String) -> String {
        let mut trace = String::new();
        for (i, frame) in self.frames.iter().rev().enumerate() {
            let ip = if frame.ip > 0 { frame.ip - 1 } else { 0 };
            let line = self
                .chunks
                .get(frame.chunk_id)
                .and_then(|c| c.lines.get(ip).copied())
                .unwrap_or(0);
            let name = self
                .chunks
                .get(frame.chunk_id)
                .map(|c| c.name.as_str())
                .unwrap_or("<unknown>");
            if i == 0 {
                trace.push_str(&format!("[line {}] in {}: {}", line, name, msg));
            } else {
                trace.push_str(&format!("\n  called from [line {}] in {}", line, name));
            }
            if i >= 10 {
                trace.push_str(&format!(
                    "\n  ... {} more frames",
                    self.frames.len() - i - 1
                ));
                break;
            }
        }
        if trace.is_empty() {
            msg
        } else {
            trace
        }
    }

    // NOTE: a pointer-caching fetch path (cache code ptr/len, revalidate by
    // Arc address + chunk id) was tried here and measured a 33% REGRESSION
    // on the sudoku workload: LLVM already hoists the chunk deref chain in
    // this simple form, and the cache's validation+writes defeated that.
    // Keep these two functions boring.
    #[inline(always)]
    pub(crate) fn read_byte(&mut self) -> Result<u8, String> {
        let idx = self.frames.len() - 1;
        let frame = &mut self.frames[idx];
        let code = &self.chunks[frame.chunk_id].code;
        if frame.ip >= code.len() {
            return Err("Unexpected EOF in bytecode".to_string());
        }
        let b = code[frame.ip];
        frame.ip += 1;
        Ok(b)
    }

    #[inline(always)]
    pub(crate) fn read_u16(&mut self) -> Result<u16, String> {
        let idx = self.frames.len() - 1;
        let frame = &mut self.frames[idx];
        let code = &self.chunks[frame.chunk_id].code;
        if frame.ip + 1 >= code.len() {
            return Err("Unexpected EOF in bytecode".to_string());
        }
        let hi = code[frame.ip] as u16;
        let lo = code[frame.ip + 1] as u16;
        frame.ip += 2;
        Ok((hi << 8) | lo)
    }

    pub(crate) fn call_value(&mut self, callee: &Value, args: Vec<Value>) -> Result<Value, String> {
        self.call_value_detailed(callee, args)
            .map_err(|failure| failure.render_compat())
    }

    /// Invoke a callable behind an explicit language-level catch boundary.
    /// Unlike an ordinary nested call, a handled failure must not leave the
    /// failed callee's frames or operands active: the caller will continue.
    pub(crate) fn call_value_caught(
        &mut self,
        callee: &Value,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let frame_depth = self.frames.len();
        let stack_depth = self.stack.len();
        let next_frame_id_before = self.next_frame_id;
        let result = self.call_value_inner_detailed(callee, args);
        let result = self.enforce_region_balance(result);
        match result {
            Ok(value) => Ok(value),
            Err(failure) => {
                let rendered = failure.render_compat();
                self.frames.truncate(frame_depth);
                self.stack.truncate(stack_depth);
                self.next_frame_id = next_frame_id_before;
                Err(rendered)
            }
        }
    }

    pub(crate) fn call_value_detailed(
        &mut self,
        callee: &Value,
        args: Vec<Value>,
    ) -> Result<Value, crate::constraint_types::VmFailure> {
        // A host call starts with no active frames and owns any settlement it
        // opens. If execution escapes before EndSettlement, unwind it here.
        // Nested VM calls (notably resolver invocation) preserve both their
        // causal context and frames so the outer boundary can render the full
        // runtime call chain before aborting.
        let frame_depth = self.frames.len();
        let stack_depth = self.stack.len();
        let next_frame_id_before = self.next_frame_id;
        let owns_execution_boundary = frame_depth == 0;
        let result = self.call_value_inner_detailed(callee, args);
        let result = if owns_execution_boundary {
            self.enforce_region_balance(result)
        } else {
            result
        };
        if result.is_err() && owns_execution_boundary {
            self.frames.truncate(frame_depth);
            self.stack.truncate(stack_depth);
            self.next_frame_id = next_frame_id_before;
        }
        result
    }
}
