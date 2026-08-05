#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameControl {
    Next,
    Halt,
}

impl VM {
    /// Charge one unit of fuel and enforce the memory ceiling.
    ///
    /// Called on loop back-edges and calls only, so any unbounded execution
    /// crosses a charge point while straight-line code stays unmetered.
    /// `u64::MAX` fuel (the default) short-circuits to a single comparison.
    #[inline(always)]
    pub(crate) fn charge_fuel(&mut self) -> Result<(), String> {
        if self.fuel == u64::MAX {
            return Ok(());
        }
        if self.fuel == 0 {
            return Err("Budget exhausted: instruction (fuel) limit reached".to_string());
        }
        self.fuel -= 1;
        if self.gc.bytes_allocated() > self.mem_limit {
            return Err(format!(
                "Budget exhausted: memory limit exceeded ({} bytes allocated)",
                self.gc.bytes_allocated()
            ));
        }
        Ok(())
    }

    #[inline(always)]
    fn charge_constraint_instruction(&mut self) -> Result<(), String> {
        if let Some(meter) = &mut self.constraint_meter {
            meter.charge_instruction()?;
        }
        Ok(())
    }

    #[inline(always)]
    fn check_constraint_heap(&self, temporary: usize) -> Result<(), String> {
        if let Some(meter) = &self.constraint_meter {
            meter.ensure_heap(self.gc.bytes_allocated(), temporary)?;
        }
        Ok(())
    }

    #[inline]
    fn preflight_constraint_binary_allocation(
        &self,
        op: Op,
        left: &Value,
        right: &Value,
    ) -> Result<(), String> {
        let retained = std::mem::size_of::<crate::value::Object>();
        let temporary = match op {
            Op::Add => {
                if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
                    left.len()
                        .saturating_add(right.len())
                        .saturating_mul(2)
                        .saturating_add(retained)
                } else if let (Some(left), Some(right)) = (left.as_list(), right.as_list()) {
                    left.len()
                        .saturating_add(right.len())
                        .saturating_mul(std::mem::size_of::<Value>())
                        .saturating_mul(2)
                        .saturating_add(retained)
                } else {
                    0
                }
            }
            Op::Mul => {
                let repeated = if let (Some(text), Some(count)) = (left.as_str(), right.as_int()) {
                    (count >= 0).then(|| text.len().saturating_mul(count as usize))
                } else if let (Some(count), Some(text)) = (left.as_int(), right.as_str()) {
                    (count >= 0).then(|| text.len().saturating_mul(count as usize))
                } else {
                    None
                };
                repeated
                    .unwrap_or(0)
                    .saturating_mul(2)
                    .saturating_add(retained)
            }
            _ => 0,
        };
        self.check_constraint_heap(temporary)
    }

    /// Collect floating garbage when the heap crosses its growth threshold.
    ///
    /// Polled at loop back-edges and calls — the same points that charge
    /// fuel — so straight-line code pays one load+cmp and any program that
    /// allocates without bound crosses a collection point. Without this, a
    /// long-running server that never calls `gc_collect()` accretes every
    /// transient payload it ever built (the syncdesk soak hit 3 GB in 50 s).
    ///
    /// Metered VMs (sandboxes) are exempt: their `mem_bytes` cap is a
    /// *total allocation* budget that doubles as a work bound, and
    /// collecting garbage out from under it would quietly change it into a
    /// (much slower to trip) live-memory cap. Sandboxed code can still call
    /// `gc_collect()` if granted.
    #[inline(always)]
    fn maybe_gc(&mut self) {
        // `gc_pause`: a builtin is holding heap values in Rust locals across
        // this nested execution (simulate's saved timeline, decode-path
        // migrations) — the collector cannot see them as roots.
        if self.mem_limit == usize::MAX && self.gc_pause == 0 && self.gc.should_collect() {
            self.collect_cycles();
        }
    }

    /// Enforce the sandbox component-write ACL. No-op for trusted code.
    #[inline]
    pub(crate) fn sandbox_check_write(&self, component: &str) -> Result<(), String> {
        self.sandbox_check_named_write("component", component)
    }

    /// Enforce the same deny-by-default write grant for an authoritative
    /// relation identity. Grants name the world type they permit; component
    /// and relation mutation share one capability mechanism.
    #[inline]
    pub(crate) fn sandbox_check_relation_write(&self, relation: &str) -> Result<(), String> {
        self.sandbox_check_named_write("relation", relation)
    }

    #[inline]
    fn sandbox_check_named_write(&self, kind: &str, identity: &str) -> Result<(), String> {
        if let Some(caps) = &self.sandbox_caps {
            if !caps.may_write(identity) {
                return Err(format!(
                    "sandbox: write to {kind} '{identity}' denied by capability grant"
                ));
            }
        }
        Ok(())
    }

    /// Enforce the sandbox component-read ACL (confidentiality dimension).
    /// No-op for trusted code, and no-op for any grant without an explicit
    /// `"read"` key (those read everything). Mirrors `sandbox_check_write`.
    #[inline]
    pub(crate) fn sandbox_check_read(&self, component: &str) -> Result<(), String> {
        if let Some(caps) = &self.sandbox_caps {
            if !caps.may_read(component) {
                return Err(format!(
                    "sandbox: read of component '{}' denied by capability grant",
                    component
                ));
            }
        }
        Ok(())
    }

    /// A whole-world reader (`save_world`, `world_digest`, unfiltered
    /// `entities()`) cannot be keyed to one component, so it requires the
    /// wildcard read grant — the confidentiality mirror of
    /// `sandbox_check_despawn`. No-op for trusted code.
    #[inline]
    pub(crate) fn sandbox_check_bulk_read(&self, what: &str) -> Result<(), String> {
        if let Some(caps) = &self.sandbox_caps {
            if !caps.may_read_all() {
                return Err(format!(
                    "sandbox: {} reads all world state and requires the \"*\" read grant",
                    what
                ));
            }
        }
        Ok(())
    }

    /// Despawning touches every component on the entity, so it requires the
    /// wildcard (`"*"`) grant. No-op for trusted code.
    #[inline]
    pub(crate) fn sandbox_check_despawn(&self) -> Result<(), String> {
        if let Some(caps) = &self.sandbox_caps {
            if !caps.may_despawn() {
                return Err(
                    "sandbox: despawn denied by capability grant (requires the \"*\" write grant)"
                        .to_string(),
                );
            }
        }
        Ok(())
    }

    fn system_component_writeback_target_exists(&self, entity_id: u32, ctype: &str) -> bool {
        if self.is_worker {
            for cmd in self.command_buffer.iter().rev() {
                match cmd {
                    EcsCommand::DespawnEntity(eid) if *eid == entity_id => return false,
                    EcsCommand::RemoveComponent(eid, removed)
                        if *eid == entity_id && removed == ctype =>
                    {
                        return false;
                    }
                    EcsCommand::SetComponent(eid, data)
                        if *eid == entity_id && data.type_name == ctype =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
        }

        self.get_world().entity_exists(entity_id)
            && self.get_world().get_component(entity_id, ctype).is_some()
    }

    pub(crate) fn run_frames(
        &mut self,
        min_depth: usize,
    ) -> Result<(), crate::constraint_types::VmFailure> {
        loop {
            if self.frames.len() <= min_depth {
                return Ok(());
            }

            // The copy-profiler needs a source line per instruction, but the
            // lookup plus two thread-local writes used to run UNCONDITIONALLY
            // — a tax on every opcode of every program ever run with the
            // profiler off (which is all of them). Pay it only when asked.
            // (Bounds and chunk validity are enforced by read_byte itself;
            // re-deriving chunk/len/ip here cost two extra indirections on
            // every single dispatch.)
            if self.profile_copies {
                let frame = self.current_frame();
                let line = self
                    .chunks
                    .get(frame.chunk_id)
                    .and_then(|chunk| chunk.lines.get(frame.ip).copied())
                    .unwrap_or(0);
                set_profile_copy_context(true, line);
            }

            let op_byte = self.read_byte()?;
            let op = Op::from_byte(op_byte)?;
            self.enforce_settlement_opcode(op)?;
            self.charge_constraint_instruction()?;
            if self.op_profile {
                self.op_counts[op_byte as usize] += 1;
            }

            match self.execute_opcode(op)? {
                FrameControl::Next => {}
                FrameControl::Halt => return Ok(()),
            }
            // Defense in depth for allocation paths whose size is dynamic
            // (including pure builtins). Variable-size aggregate opcodes are
            // preflighted above; this catches every retained GC allocation
            // before another instruction can observe it.
            self.check_constraint_heap(0)?;
        }
    }

    fn execute_opcode(
        &mut self,
        op: Op,
    ) -> Result<FrameControl, crate::constraint_types::VmFailure> {
        match op {
            Op::Const | Op::Pop | Op::PopN | Op::Dup | Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod | Op::BitAnd | Op::BitOr | Op::BitXor | Op::Shl | Op::Shr | Op::Neg | Op::BitNot | Op::Eq | Op::Neq | Op::Lt | Op::Gt | Op::Lte | Op::Gte | Op::Not | Op::And | Op::Or | Op::DefGlobal | Op::GetGlobal | Op::SetGlobal | Op::GetLocal | Op::GetLocal2 | Op::SetLocal | Op::MoveLocal | Op::GetUpvalue | Op::SetUpvalue | Op::Jump | Op::JumpIfFalse | Op::EqJF | Op::NeqJF | Op::LtJF | Op::LteJF | Op::GtJF | Op::GteJF | Op::EqConst | Op::NeqConst | Op::EqConstJF | Op::NeqConstJF | Op::ConstArith | Op::IncLocal | Op::JumpBack | Op::ForRangeNext | Op::Pipe => self.execute_stack_and_flow_opcode(op),
            Op::Call | Op::AsyncCall | Op::Await | Op::Yield | Op::Return | Op::Try | Op::Unpack | Op::Closure => self.execute_call_and_result_opcode(op),
            Op::MakeList | Op::MakeTuple | Op::MakeMap | Op::MakeComp | Op::GetField | Op::SetField | Op::GetIndex | Op::ListGetLocal | Op::ListGetLL | Op::SetIndex | Op::EcsGet | Op::EcsSet | Op::EcsHas | Op::EcsSpawn | Op::EcsQuery | Op::LogicalLoad | Op::LogicalStore | Op::MaterializeAoS | Op::ConcatN | Op::InitResource => self.execute_value_opcode(op),
            Op::MakeState | Op::Transition | Op::MakeVariant | Op::Emit | Op::EmitAfter | Op::RunSystem | Op::RunSchedule | Op::RunScheduleSerial | Op::BeginSettlement | Op::EndSettlement | Op::ProposeIntent | Op::StageCandidate | Op::ReadBaseComponent | Op::ReadCandidateComponent | Op::RequireConstraint | Op::MatchState | Op::Print | Op::Len | Op::TypeOf | Op::Break | Op::GetFieldSlot | Op::SetFieldSlot | Op::MakeCompSlot | Op::QueryFilter | Op::QueryProject | Op::Snapshot => self.execute_world_opcode(op),
            Op::Rollback | Op::BitsetSetInplace | Op::BitsetClearInplace | Op::BufferAppendInplace | Op::ByteBufSetU8Inplace | Op::ByteBufSetU32LeInplace | Op::ByteBufSetI32LeInplace | Op::GetIter | Op::IterNext | Op::ListPushLocal | Op::ListSetLocal | Op::IsVariant | Op::VecAdd | Op::VecSub | Op::VecMul | Op::VecDiv | Op::VecMod | Op::VecNeg | Op::VecNot | Op::VecEq | Op::VecNeq | Op::VecLt | Op::VecGt | Op::VecLte | Op::VecGte | Op::VecFilter | Op::VecSelect | Op::LoadColumn | Op::VecBroadcast | Op::OnceGuardPass => self.execute_collection_opcode(op),
            Op::PopCheckErr | Op::Halt => self.execute_terminal_opcode(op),
        }
    }

    fn execute_stack_and_flow_opcode(&mut self, op: Op) -> Result<FrameControl, crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
        match op {
            Op::Const => {
                    let idx = self.read_u16()? as usize;
                    let v = self
                        .current_chunk()
                        .constants
                        .get(idx)
                        .cloned()
                        .ok_or_else(|| format!("Invalid constant index {}", idx))?;
                    self.push(v);
                }
                Op::Pop => {
                    self.pop()?;
                }
                Op::PopN => {
                    let n = self.read_byte()? as usize;
                    let len = self.stack.len();
                    if len < n {
                        return Err("PopN: stack underflow".to_string());
                    }
                    self.stack.truncate(len - n);
                }
                Op::Dup => {
                    let v = *self.peek()?;
                    self.push(v);
                }

                Op::Add => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.preflight_constraint_binary_allocation(Op::Add, &a, &b)?;
                    let out = helpers::binary_add(&mut self.gc, a, b)?;
                    self.push(out);
                }
                Op::Sub => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let out = helpers::binary_sub(&mut self.gc, a, b)?;
                    self.push(out);
                }
                Op::Mul => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.preflight_constraint_binary_allocation(Op::Mul, &a, &b)?;
                    let out = helpers::binary_mul(&mut self.gc, a, b, self.mem_limit)?;
                    self.push(out);
                }
                Op::Div => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let out = helpers::binary_div(&mut self.gc, a, b)?;
                    self.push(out);
                }
                Op::Mod => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let out = helpers::binary_mod(&mut self.gc, a, b)?;
                    self.push(out);
                }
                Op::BitAnd => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let out = helpers::binary_bitand(&mut self.gc, a, b)?;
                    self.push(out);
                }
                Op::BitOr => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let out = helpers::binary_bitor(&mut self.gc, a, b)?;
                    self.push(out);
                }
                Op::BitXor => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let out = helpers::binary_bitxor(&mut self.gc, a, b)?;
                    self.push(out);
                }
                Op::Shl => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let out = helpers::binary_shl(&mut self.gc, a, b)?;
                    self.push(out);
                }
                Op::Shr => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let out = helpers::binary_shr(&mut self.gc, a, b)?;
                    self.push(out);
                }
                Op::Neg => {
                    let v = self.pop()?;
                    let out = helpers::unary_neg(&mut self.gc, v)?;
                    self.push(out);
                }
                Op::BitNot => {
                    let v = self.pop()?;
                    let out = helpers::unary_bitnot(&mut self.gc, v)?;
                    self.push(out);
                }

                Op::Eq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(Value::from_bool(helpers::values_equal(&a, &b)));
                }
                Op::Neq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(Value::from_bool(!helpers::values_equal(&a, &b)));
                }
                Op::Lt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(Value::from_bool(helpers::cmp_lt(&a, &b)?));
                }
                Op::Gt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(Value::from_bool(helpers::cmp_gt(&a, &b)?));
                }
                Op::Lte => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(Value::from_bool(helpers::cmp_lte(&a, &b)?));
                }
                Op::Gte => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(Value::from_bool(helpers::cmp_gte(&a, &b)?));
                }

                Op::Not => {
                    let v = self.pop()?;
                    self.push(Value::from_bool(!v.is_truthy()));
                }
                Op::And => {
                    return Err("Opcode And is unsupported: logical 'and' must be compiled via short-circuit jumps".to_string());
                }
                Op::Or => {
                    return Err("Opcode Or is unsupported: logical 'or' must be compiled via short-circuit jumps".to_string());
                }

                Op::DefGlobal => {
                    let slot = self.read_u16()? as usize;
                    let val = self.pop()?;
                    if slot >= self.globals.len() {
                        self.globals.resize(slot + 1, Value::NIL);
                    }
                    self.globals[slot] = val;
                }
                Op::GetGlobal => {
                    let slot = self.read_u16()? as usize;
                    if slot >= self.globals.len() {
                        let name = self
                            .global_names
                            .get(slot)
                            .cloned()
                            .unwrap_or_else(|| format!("slot#{}", slot));
                        return Err(format!("Undefined global `{}`", name));
                    }
                    let v = self.globals[slot];
                    self.push(v);
                }
                Op::SetGlobal => {
                    let slot = self.read_u16()? as usize;
                    let val = self.pop()?;
                    if slot >= self.globals.len() {
                        let name = self
                            .global_names
                            .get(slot)
                            .cloned()
                            .unwrap_or_else(|| format!("slot#{}", slot));
                        return Err(format!("Undefined global `{}`", name));
                    }
                    self.globals[slot] = val;
                }
                Op::GetLocal => {
                    let off = self.read_u16()? as usize;
                    let base = self.current_frame().stack_base;
                    let idx = base + off;
                    let out = self
                        .stack
                        .get(idx)
                        .ok_or_else(|| format!("Invalid local offset {}", off))?;
                    let out = if let Some(cell) = out.as_cell() {
                        unsafe { (*cell).get() }
                    } else {
                        *out
                    };
                    self.push(out);
                }
                // Two fused GetLocals (peephole) — one dispatch, two pushes.
                // The first value MUST be pushed before the second slot is
                // read: when the first push creates a fresh binding's slot,
                // the second GetLocal may legally read exactly that slot
                // (loop binding followed by its first use).
                Op::GetLocal2 => {
                    let off1 = self.read_u16()? as usize;
                    let off2 = self.read_u16()? as usize;
                    let base = self.current_frame().stack_base;
                    let v1 = *self
                        .stack
                        .get(base + off1)
                        .ok_or_else(|| format!("Invalid local offset {}", off1))?;
                    let v1 = if let Some(cell) = v1.as_cell() {
                        unsafe { (*cell).get() }
                    } else {
                        v1
                    };
                    self.push(v1);
                    let v2 = *self
                        .stack
                        .get(base + off2)
                        .ok_or_else(|| format!("Invalid local offset {}", off2))?;
                    let v2 = if let Some(cell) = v2.as_cell() {
                        unsafe { (*cell).get() }
                    } else {
                        v2
                    };
                    self.push(v2);
                }
                Op::SetLocal => {
                    let off = self.read_u16()? as usize;
                    self.reject_captured_local_mutation(off, "SetLocal")?;
                    let base = self.current_frame().stack_base;
                    let idx = base + off;
                    let val = self.pop()?;
                    let slot = self
                        .stack
                        .get_mut(idx)
                        .ok_or_else(|| format!("Invalid local offset {}", off))?;
                    if let Some(cell) = slot.as_cell() {
                        unsafe { (*cell).set(val) };
                    } else {
                        *slot = val;
                    }
                }
                Op::MoveLocal => {
                    let off = self.read_u16()? as usize;
                    self.reject_captured_local_mutation(off, "MoveLocal")?;
                    let base = self.current_frame().stack_base;
                    let idx = base + off;
                    let slot = self
                        .stack
                        .get_mut(idx)
                        .ok_or_else(|| format!("Invalid local offset {}", off))?;
                    let out = if let Some(cell) = slot.as_cell() {
                        let v = unsafe { (*cell).get() };
                        unsafe { (*cell).set(Value::NIL) };
                        v
                    } else {
                        std::mem::replace(slot, Value::NIL)
                    };
                    self.push(out);
                }

                Op::GetUpvalue => {
                    let idx = self.read_u16()? as usize;
                    let cell = self
                        .current_frame()
                        .captures
                        .as_ref()
                        .and_then(|c| c.get(idx).copied())
                        .ok_or_else(|| format!("Invalid upvalue index {}", idx))?;
                    self.push(unsafe { (*cell).get() });
                }
                Op::SetUpvalue => {
                    let idx = self.read_u16()? as usize;
                    let val = self.pop()?;
                    let captures = self
                        .current_frame()
                        .captures
                        .as_ref()
                        .ok_or_else(|| "No captures in current frame".to_string())?;
                    if idx >= captures.len() {
                        return Err(format!("Invalid upvalue index {}", idx));
                    }
                    unsafe { (*captures[idx]).set(val) };
                }

                Op::Jump => {
                    let target = self.read_u16()? as usize;
                    self.current_frame_mut().ip = target;
                }
                Op::JumpIfFalse => {
                    let target = self.read_u16()? as usize;
                    let cond = self.pop()?;
                    if !cond.is_truthy() {
                        self.current_frame_mut().ip = target;
                    }
                }
                // fused compare-and-branch (peephole): jump when FALSE.
                Op::EqJF => {
                    let target = self.read_u16()? as usize;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if !helpers::values_equal(&a, &b) {
                        self.current_frame_mut().ip = target;
                    }
                }
                Op::NeqJF => {
                    let target = self.read_u16()? as usize;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if helpers::values_equal(&a, &b) {
                        self.current_frame_mut().ip = target;
                    }
                }
                Op::LtJF => {
                    let target = self.read_u16()? as usize;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if !helpers::cmp_lt(&a, &b)? {
                        self.current_frame_mut().ip = target;
                    }
                }
                Op::LteJF => {
                    let target = self.read_u16()? as usize;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if !helpers::cmp_lte(&a, &b)? {
                        self.current_frame_mut().ip = target;
                    }
                }
                Op::GtJF => {
                    let target = self.read_u16()? as usize;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if !helpers::cmp_gt(&a, &b)? {
                        self.current_frame_mut().ip = target;
                    }
                }
                Op::GteJF => {
                    let target = self.read_u16()? as usize;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if !helpers::cmp_gte(&a, &b)? {
                        self.current_frame_mut().ip = target;
                    }
                }
                // constant-rhs fusions: the Const dispatch folded away.
                Op::EqConst => {
                    let idx = self.read_u16()? as usize;
                    let b = helpers::constant_value(self.current_chunk(), idx)?;
                    let a = self.pop()?;
                    self.push(Value::from_bool(helpers::values_equal(&a, &b)));
                }
                Op::NeqConst => {
                    let idx = self.read_u16()? as usize;
                    let b = helpers::constant_value(self.current_chunk(), idx)?;
                    let a = self.pop()?;
                    self.push(Value::from_bool(!helpers::values_equal(&a, &b)));
                }
                Op::EqConstJF => {
                    let idx = self.read_u16()? as usize;
                    let target = self.read_u16()? as usize;
                    let b = helpers::constant_value(self.current_chunk(), idx)?;
                    let a = self.pop()?;
                    if !helpers::values_equal(&a, &b) {
                        self.current_frame_mut().ip = target;
                    }
                }
                Op::NeqConstJF => {
                    let idx = self.read_u16()? as usize;
                    let target = self.read_u16()? as usize;
                    let b = helpers::constant_value(self.current_chunk(), idx)?;
                    let a = self.pop()?;
                    if helpers::values_equal(&a, &b) {
                        self.current_frame_mut().ip = target;
                    }
                }
                Op::ConstArith => {
                    let idx = self.read_u16()? as usize;
                    let arith = self.read_byte()?;
                    let b = helpers::constant_value(self.current_chunk(), idx)?;
                    let a = self.pop()?;
                    let out = match Op::from_byte(arith)? {
                        Op::Add => helpers::binary_add(&mut self.gc, a, b)?,
                        Op::Sub => helpers::binary_sub(&mut self.gc, a, b)?,
                        Op::Mul => helpers::binary_mul(&mut self.gc, a, b, self.mem_limit)?,
                        Op::Div => helpers::binary_div(&mut self.gc, a, b)?,
                        Op::Mod => helpers::binary_mod(&mut self.gc, a, b)?,
                        Op::BitAnd => helpers::binary_bitand(&mut self.gc, a, b)?,
                        Op::BitOr => helpers::binary_bitor(&mut self.gc, a, b)?,
                        Op::BitXor => helpers::binary_bitxor(&mut self.gc, a, b)?,
                        Op::Shl => helpers::binary_shl(&mut self.gc, a, b)?,
                        Op::Shr => helpers::binary_shr(&mut self.gc, a, b)?,
                        other => return Err(format!("ConstArith: unsupported op {:?}", other)),
                    };
                    self.push(out);
                }
                // x = x + K in one dispatch (int fast path; falls back to
                // the generic add semantics for float locals).
                Op::IncLocal => {
                    let slot = self.read_u16()? as usize;
                    let idx = self.read_u16()? as usize;
                    self.reject_captured_local_mutation(slot, "IncLocal")?;
                    let k = helpers::constant_value(self.current_chunk(), idx)?;
                    let base = self.current_frame().stack_base;
                    let stack_index = base
                        .checked_add(slot)
                        .ok_or_else(|| format!("Invalid local offset {}", slot))?;
                    let slot_val = *self
                        .stack
                        .get(stack_index)
                        .ok_or_else(|| format!("Invalid local offset {}", slot))?;
                    if let Some(cell) = slot_val.as_cell() {
                        let cur = unsafe { (*cell).get() };
                        let out = helpers::binary_add(&mut self.gc, cur, k)?;
                        unsafe { (*cell).set(out) };
                    } else {
                        let out = helpers::binary_add(&mut self.gc, slot_val, k)?;
                        *self
                            .stack
                            .get_mut(stack_index)
                            .ok_or_else(|| format!("Invalid local offset {}", slot))? = out;
                    }
                }
                Op::JumpBack => {
                    self.charge_fuel()?;
                    self.maybe_gc();
                    let delta = self.read_u16()? as usize;
                    let ip = self.current_frame().ip;
                    if ip < delta {
                        return Err("JumpBack underflow".to_string());
                    }
                    self.current_frame_mut().ip = ip - delta;
                }
                // increment + bounds test + back-jump of a counted range
                // loop, one dispatch (see opcode docs).
                Op::ForRangeNext => {
                    let cur_slot = self.read_u16()? as usize;
                    let end_slot = self.read_u16()? as usize;
                    let delta = self.read_u16()? as usize;
                    self.reject_captured_local_mutation(cur_slot, "ForRangeNext")?;
                    let base = self.current_frame().stack_base;
                    let cur_index = base
                        .checked_add(cur_slot)
                        .ok_or_else(|| format!("Invalid local offset {}", cur_slot))?;
                    let end_index = base
                        .checked_add(end_slot)
                        .ok_or_else(|| format!("Invalid local offset {}", end_slot))?;
                    let cur = self
                        .stack
                        .get(cur_index)
                        .ok_or_else(|| format!("Invalid local offset {}", cur_slot))?
                        .as_int()
                        .ok_or_else(|| "ForRangeNext: loop counter is not an int".to_string())?;
                    let end = self
                        .stack
                        .get(end_index)
                        .ok_or_else(|| format!("Invalid local offset {}", end_slot))?
                        .as_int()
                        .ok_or_else(|| "ForRangeNext: loop bound is not an int".to_string())?;
                    let next = cur + 1;
                    let next_value = Value::from_int(&mut self.gc, next);
                    *self
                        .stack
                        .get_mut(cur_index)
                        .ok_or_else(|| format!("Invalid local offset {}", cur_slot))? = next_value;
                    if next < end {
                        self.charge_fuel()?;
                        self.maybe_gc();
                        let ip = self.current_frame().ip;
                        if ip < delta {
                            return Err("ForRangeNext underflow".to_string());
                        }
                        self.current_frame_mut().ip = ip - delta;
                    }
                }

                Op::Pipe => {
                    return Err("Opcode Pipe is unsupported: pipe expressions are compiled directly to Call".to_string());
                }
            _ => unreachable!("opcode dispatcher selected the wrong partition"),
        }
        Ok(FrameControl::Next)
    }}
