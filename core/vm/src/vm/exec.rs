#![allow(clippy::arc_with_non_send_sync)]

use super::*;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::opcode::{Chunk, Op};
use crate::value::{
    set_profile_copy_context, ClosureValue, ComponentData, MapKey, MapStorage, Value,
};

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

thread_local! {
    static WORKER_VM: std::cell::RefCell<Option<crate::vm::VM>> = const { std::cell::RefCell::new(None) };
}

/// Run `f` against this thread's pooled worker VM, creating it on first use.
/// Used by parallel system batches and `simulate_par` fork exploration.
pub(crate) fn with_worker_vm<R>(
    shared: &crate::vm::VmSharedState,
    f: impl FnOnce(&mut crate::vm::VM) -> R,
) -> R {
    WORKER_VM.with(|cell| {
        let mut opt = cell.borrow_mut();
        if opt.is_none() {
            *opt = Some(crate::vm::VM::from_shared_state(shared.clone()));
        }
        let worker = opt.as_mut().unwrap();
        worker.sync_from_shared(shared);
        f(worker)
    })
}

/// Fold one `accum` contribution into the accumulator (dogfood seq 83
/// IDEA 02): per field, `acc += contrib - base` for ints and floats.
/// Non-numeric fields keep the accumulator's value — the checker rejects
/// them on `accum` resources, so this is belt-and-braces only. Values ride
/// the persistent heap (worker commands are persisted), so a folded int
/// that outgrows the inline range allocates via `PersistentStore`.
fn fold_accum_delta(
    acc: &mut crate::value::ComponentData,
    base: &crate::value::ComponentData,
    contrib: &crate::value::ComponentData,
) {
    let n = acc
        .values
        .len()
        .min(base.values.len())
        .min(contrib.values.len());
    for i in 0..n {
        let a = &acc.values[i];
        let b = &base.values[i];
        let c = &contrib.values[i];
        if let (Some(av), Some(bv), Some(cv)) = (a.as_int(), b.as_int(), c.as_int()) {
            acc.values[i] = crate::value::Value::from_int(
                &mut crate::value::PersistentStore,
                av.wrapping_add(cv.wrapping_sub(bv)),
            );
        } else if let (Some(av), Some(bv), Some(cv)) = (a.as_float(), b.as_float(), c.as_float()) {
            acc.values[i] = crate::value::Value::from_float(av + (cv - bv));
        }
    }
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
        if let Some(caps) = &self.sandbox_caps {
            if !caps.may_write(component) {
                return Err(format!(
                    "sandbox: write to component '{}' denied by capability grant",
                    component
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

    pub(crate) fn run_frames(&mut self, min_depth: usize) -> Result<(), String> {
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
            if self.op_profile {
                self.op_counts[op_byte as usize] += 1;
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
                    let out = helpers::binary_mul(&mut self.gc, a, b)?;
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
                    let b = self.current_chunk().constants[idx];
                    let a = self.pop()?;
                    self.push(Value::from_bool(helpers::values_equal(&a, &b)));
                }
                Op::NeqConst => {
                    let idx = self.read_u16()? as usize;
                    let b = self.current_chunk().constants[idx];
                    let a = self.pop()?;
                    self.push(Value::from_bool(!helpers::values_equal(&a, &b)));
                }
                Op::EqConstJF => {
                    let idx = self.read_u16()? as usize;
                    let target = self.read_u16()? as usize;
                    let b = self.current_chunk().constants[idx];
                    let a = self.pop()?;
                    if !helpers::values_equal(&a, &b) {
                        self.current_frame_mut().ip = target;
                    }
                }
                Op::NeqConstJF => {
                    let idx = self.read_u16()? as usize;
                    let target = self.read_u16()? as usize;
                    let b = self.current_chunk().constants[idx];
                    let a = self.pop()?;
                    if helpers::values_equal(&a, &b) {
                        self.current_frame_mut().ip = target;
                    }
                }
                Op::ConstArith => {
                    let idx = self.read_u16()? as usize;
                    let arith = self.read_byte()?;
                    let b = self.current_chunk().constants[idx];
                    let a = self.pop()?;
                    let out = match Op::from_byte(arith)? {
                        Op::Add => helpers::binary_add(&mut self.gc, a, b)?,
                        Op::Sub => helpers::binary_sub(&mut self.gc, a, b)?,
                        Op::Mul => helpers::binary_mul(&mut self.gc, a, b)?,
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
                    let k = self.current_chunk().constants[idx];
                    let base = self.current_frame().stack_base;
                    let slot_val = &mut self.stack[base + slot];
                    if let Some(cell) = slot_val.as_cell() {
                        let cur = unsafe { (*cell).get() };
                        let out = helpers::binary_add(&mut self.gc, cur, k)?;
                        unsafe { (*cell).set(out) };
                    } else {
                        let cur = *slot_val;
                        let out = helpers::binary_add(&mut self.gc, cur, k)?;
                        self.stack[base + slot] = out;
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
                    let base = self.current_frame().stack_base;
                    let cur = self.stack[base + cur_slot]
                        .as_int()
                        .ok_or_else(|| "ForRangeNext: loop counter is not an int".to_string())?;
                    let end = self.stack[base + end_slot]
                        .as_int()
                        .ok_or_else(|| "ForRangeNext: loop bound is not an int".to_string())?;
                    let next = cur + 1;
                    self.stack[base + cur_slot] = Value::from_int(&mut self.gc, next);
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
                Op::Call => {
                    self.charge_fuel()?;
                    self.maybe_gc();
                    self.exec_call()?;
                }
                Op::AsyncCall => {
                    self.charge_fuel()?;
                    self.exec_async_call()?;
                }
                Op::Await => {
                    self.exec_await()?;
                }
                Op::Yield => {}
                Op::Return => {
                    let result = self.pop()?;
                    let frame = self.frames.pop().ok_or("Frame stack underflow")?;
                    if let Some(writeback) = &frame.system_writeback {
                        for (slot, ctype) in &writeback.mutable_params {
                            let idx = frame.stack_base + (*slot as usize);
                            let comp = std::mem::replace(
                                self.stack.get_mut(idx).ok_or_else(|| {
                                    format!("System writeback slot out of range: {}", slot)
                                })?,
                                Value::NIL,
                            );
                            // A closure in the body (query filters, callbacks)
                            // may have captured the mut param, promoting the
                            // slot to a capture cell — the cell's current
                            // content is the param's final value.
                            let comp = if let Some(cell) = comp.as_cell() {
                                unsafe { (*cell).get() }
                            } else {
                                comp
                            };
                            let type_name = comp.type_name().to_string();
                            let data = comp.into_component().ok_or_else(|| {
                                format!(
                                    "System mutable param `{}` expected component, got {}",
                                    ctype, type_name
                                )
                            })?;
                            if !self.system_component_writeback_target_exists(
                                writeback.entity_id,
                                ctype,
                            ) {
                                continue;
                            }
                            if self.is_worker {
                                // Buffered values must survive worker GC
                                // until end-of-frame apply: persist now,
                                // apply consumes ownership (no re-copy).
                                let mut buffered = data.clone();
                                Value::persist_component_data(&mut buffered);
                                self.command_buffer
                                    .push(EcsCommand::SetComponent(writeback.entity_id, buffered));
                            } else {
                                // A system may dispose of the entity it is
                                // visiting (`despawn(self)` — projectiles on
                                // arrival); the writeback then has nothing
                                // to write to, by design.
                                if !self.get_world().entity_exists(writeback.entity_id) {
                                    continue;
                                }
                                let summary = Self::component_summary(&data);
                                if !self
                                    .get_world_mut()
                                    .set_component(writeback.entity_id, data)
                                {
                                    return Err(format!(
                                        "System writeback: entity {} no longer exists",
                                        writeback.entity_id
                                    ));
                                }
                                self.record_causal_write(
                                    Some(writeback.entity_id),
                                    ctype,
                                    crate::causality::WriteKind::Set,
                                    summary,
                                );
                            }
                        }
                        for (slot, rtype) in &writeback.mutable_resources {
                            let idx = frame.stack_base + (*slot as usize);
                            let comp = std::mem::replace(
                                self.stack.get_mut(idx).ok_or_else(|| {
                                    format!("System resource writeback slot out of range: {}", slot)
                                })?,
                                Value::NIL,
                            );
                            let type_name = comp.type_name().to_string();
                            let data = comp.into_component().ok_or_else(|| {
                                format!(
                                    "System mutable resource `{}` expected component, got {}",
                                    rtype, type_name
                                )
                            })?;
                            if self.is_worker {
                                let mut buffered = data.clone();
                                Value::persist_component_data(&mut buffered);
                                self.command_buffer
                                    .push(EcsCommand::SetResource(rtype.clone(), buffered));
                                // Unlike a component, a resource is shared by
                                // every entity the system visits, so the
                                // worker's private world must observe the
                                // write: the buffered command carries an
                                // absolute value, and without this the next
                                // iteration recomputes it from the snapshot
                                // and the accumulation collapses to one step.
                                self.get_world_mut().set_resource(rtype, data);
                            } else {
                                let summary = Self::component_summary(&data);
                                self.get_world_mut().set_resource(rtype, data);
                                self.record_causal_write(
                                    None,
                                    rtype,
                                    crate::causality::WriteKind::Resource,
                                    summary,
                                );
                            }
                        }
                    }
                    self.stack.truncate(frame.stack_base);
                    self.push(result);
                }
                Op::Try => {
                    let val = self.pop()?;
                    if let Some(st) = val.as_sum_type() {
                        if st.type_name == "Result" {
                            if st.variant == "Ok" {
                                let inner = st.fields.get("value").cloned().unwrap_or(Value::NIL);
                                self.push(inner);
                            } else if st.variant == "Err" {
                                let frame = self.frames.pop().ok_or("Frame stack underflow")?;
                                if let Some(writeback) = &frame.system_writeback {
                                    for (slot, ctype) in &writeback.mutable_params {
                                        let idx = frame.stack_base + (*slot as usize);
                                        if let Some(slot_ref) = self.stack.get_mut(idx) {
                                            let comp = std::mem::replace(slot_ref, Value::NIL);
                                            if let Some(data) = comp.into_component() {
                                                if !self.system_component_writeback_target_exists(
                                                    writeback.entity_id,
                                                    ctype,
                                                ) {
                                                    continue;
                                                }
                                                if self.is_worker {
                                                    let mut buffered = data.clone();
                                                    Value::persist_component_data(&mut buffered);
                                                    self.command_buffer.push(
                                                        EcsCommand::SetComponent(
                                                            writeback.entity_id,
                                                            buffered,
                                                        ),
                                                    );
                                                } else {
                                                    let cname = data.type_name.clone();
                                                    let summary = Self::component_summary(&data);
                                                    let _ = self
                                                        .get_world_mut()
                                                        .set_component(writeback.entity_id, data);
                                                    self.record_causal_write(
                                                        Some(writeback.entity_id),
                                                        &cname,
                                                        crate::causality::WriteKind::Set,
                                                        summary,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    for (slot, rtype) in &writeback.mutable_resources {
                                        let idx = frame.stack_base + (*slot as usize);
                                        if let Some(slot_ref) = self.stack.get_mut(idx) {
                                            let comp = std::mem::replace(slot_ref, Value::NIL);
                                            if let Some(data) = comp.into_component() {
                                                if self.is_worker {
                                                    let mut buffered = data.clone();
                                                    Value::persist_component_data(&mut buffered);
                                                    self.command_buffer.push(
                                                        EcsCommand::SetResource(
                                                            rtype.clone(),
                                                            buffered,
                                                        ),
                                                    );
                                                    // See the resource
                                                    // writeback on the normal
                                                    // return path: the worker's
                                                    // own world has to observe
                                                    // shared-resource writes.
                                                    self.get_world_mut().set_resource(rtype, data);
                                                } else {
                                                    let summary = Self::component_summary(&data);
                                                    self.get_world_mut().set_resource(rtype, data);
                                                    self.record_causal_write(
                                                        None,
                                                        rtype,
                                                        crate::causality::WriteKind::Resource,
                                                        summary,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                self.stack.truncate(frame.stack_base);
                                self.push(val);
                            } else {
                                return Err(format!("Unknown Result variant '{}'", st.variant));
                            }
                        } else if st.type_name == "Option" {
                            if st.variant == "Some" {
                                let inner = st.fields.get("value").cloned().unwrap_or(Value::NIL);
                                self.push(inner);
                            } else if st.variant == "None" {
                                let frame = self.frames.pop().ok_or("Frame stack underflow")?;
                                if let Some(writeback) = &frame.system_writeback {
                                    for (slot, ctype) in &writeback.mutable_params {
                                        let idx = frame.stack_base + (*slot as usize);
                                        if let Some(slot_ref) = self.stack.get_mut(idx) {
                                            let comp = std::mem::replace(slot_ref, Value::NIL);
                                            if let Some(data) = comp.into_component() {
                                                if !self.system_component_writeback_target_exists(
                                                    writeback.entity_id,
                                                    ctype,
                                                ) {
                                                    continue;
                                                }
                                                if self.is_worker {
                                                    let mut buffered = data.clone();
                                                    Value::persist_component_data(&mut buffered);
                                                    self.command_buffer.push(
                                                        EcsCommand::SetComponent(
                                                            writeback.entity_id,
                                                            buffered,
                                                        ),
                                                    );
                                                } else {
                                                    let cname = data.type_name.clone();
                                                    let summary = Self::component_summary(&data);
                                                    let _ = self
                                                        .get_world_mut()
                                                        .set_component(writeback.entity_id, data);
                                                    self.record_causal_write(
                                                        Some(writeback.entity_id),
                                                        &cname,
                                                        crate::causality::WriteKind::Set,
                                                        summary,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    for (slot, rtype) in &writeback.mutable_resources {
                                        let idx = frame.stack_base + (*slot as usize);
                                        if let Some(slot_ref) = self.stack.get_mut(idx) {
                                            let comp = std::mem::replace(slot_ref, Value::NIL);
                                            if let Some(data) = comp.into_component() {
                                                if self.is_worker {
                                                    let mut buffered = data.clone();
                                                    Value::persist_component_data(&mut buffered);
                                                    self.command_buffer.push(
                                                        EcsCommand::SetResource(
                                                            rtype.clone(),
                                                            buffered,
                                                        ),
                                                    );
                                                    // See the resource
                                                    // writeback on the normal
                                                    // return path: the worker's
                                                    // own world has to observe
                                                    // shared-resource writes.
                                                    self.get_world_mut().set_resource(rtype, data);
                                                } else {
                                                    let summary = Self::component_summary(&data);
                                                    self.get_world_mut().set_resource(rtype, data);
                                                    self.record_causal_write(
                                                        None,
                                                        rtype,
                                                        crate::causality::WriteKind::Resource,
                                                        summary,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                self.stack.truncate(frame.stack_base);
                                self.push(val);
                            } else {
                                return Err(format!("Unknown Option variant '{}'", st.variant));
                            }
                        } else {
                            return Err(format!(
                                "`?` operator can only be used on Result or Option, got {}",
                                st.type_name
                            ));
                        }
                    } else {
                        return Err(format!(
                            "`?` operator can only be used on Result or Option, got {}",
                            val.type_name()
                        ));
                    }
                }

                Op::Unpack => {
                    let v = self.pop()?;
                    let type_name = v.type_name().to_string();
                    if let Some(tuple) = v.as_tuple() {
                        let items: Vec<Value> = tuple.clone();
                        for item in items {
                            self.push(item);
                        }
                    } else if let Some(list) = v.into_rad_list() {
                        for item in list.into_vec() {
                            self.push(item);
                        }
                    } else {
                        return Err(format!("Unpack expected list/tuple, got {}", type_name));
                    }
                }
                Op::Closure => {
                    self.exec_closure()?;
                }

                Op::MakeList => {
                    let n = self.read_u16()? as usize;
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n {
                        items.push(self.pop()?);
                    }
                    items.reverse();
                    self.push_list_vec(items);
                }
                Op::MakeTuple => {
                    let n = self.read_u16()? as usize;
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n {
                        items.push(self.pop()?);
                    }
                    items.reverse();
                    let tup = Value::tuple(&mut self.gc, items);
                    self.push(tup);
                }
                Op::MakeMap => {
                    let n = self.read_u16()? as usize;
                    let mut entries = MapStorage::new();
                    for _ in 0..n {
                        let val = self.pop()?;
                        let key = self.pop()?;
                        let map_key = MapKey::from_value(&key)?;
                        entries.insert(map_key, val);
                    }
                    let map_val = Value::map(&mut self.gc, entries);
                    self.push(map_val);
                }
                Op::MakeComp => {
                    let type_idx = self.read_u16()? as usize;
                    let type_name = helpers::constant_string(self.current_chunk(), type_idx)?;
                    let field_count = self.read_u16()? as usize;
                    let layout = self
                        .component_layouts
                        .get(&type_name)
                        .ok_or_else(|| format!("No layout for component `{}`", type_name))?
                        .clone();
                    let mut fields = HashMap::new();
                    for _ in 0..field_count {
                        let val = self.pop()?;
                        let name_val = self.pop()?;
                        let name = name_val.as_str().map(|s| s.to_string()).ok_or_else(|| {
                            format!(
                                "Component field name must be string, got {}",
                                name_val.type_name()
                            )
                        })?;
                        fields.insert(name, val);
                    }
                    let mut vals = vec![Value::NIL; layout.len()];
                    for (i, name) in layout.iter().enumerate() {
                        if let Some(val) = fields.remove(name) {
                            vals[i] = val;
                        }
                    }
                    let comp_val = Value::component(&mut self.gc, type_name, layout, vals);
                    self.push(comp_val);
                }
                Op::GetField => {
                    let field_idx = self.read_u16()? as usize;
                    let field = helpers::constant_string(self.current_chunk(), field_idx)?;
                    let obj = self.pop()?;
                    if let Some(c) = obj.as_component() {
                        if let Some(idx) = c.layout.iter().position(|n| n == &field) {
                            let v = c.values.get(idx).cloned().unwrap_or(Value::NIL);
                            self.push(v);
                        } else {
                            return Err(format!("Unknown field `{}`", field));
                        }
                    } else if let Some(st) = obj.as_sum_type() {
                        let v = st.fields.get(&field).cloned().ok_or_else(|| {
                            format!(
                                "Unknown field `{}` on {}::{}",
                                field, st.type_name, st.variant
                            )
                        })?;
                        self.push(v);
                    } else {
                        return Err(format!(
                            "GetField expected component or variant, got {}",
                            obj.type_name()
                        ));
                    }
                }
                Op::SetField => {
                    let field_idx = self.read_u16()? as usize;
                    let field = helpers::constant_string(self.current_chunk(), field_idx)?;
                    let val = self.pop()?;
                    let obj = self.pop()?;
                    let type_name = obj.type_name().to_string();
                    if let Some(mut c) = obj.into_component() {
                        if let Some(idx) = c.layout.iter().position(|n| n == &field) {
                            c.values[idx] = val;
                            let out = Value::from_component_data(&mut self.gc, c);
                            self.push(out);
                        } else {
                            return Err(format!("Unknown field `{}`", field));
                        }
                    } else {
                        return Err(format!("SetField expected component, got {}", type_name));
                    }
                }
                Op::GetIndex => {
                    self.exec_get_index()?;
                }
                Op::ListGetLocal => {
                    let slot = self.read_u16()? as usize;
                    let idx_val = self.pop()?;
                    let base = self.current_frame().stack_base;
                    let slot_val = self.stack[base + slot];
                    let obj = if let Some(cell) = slot_val.as_cell() {
                        unsafe { (*cell).get() }
                    } else {
                        slot_val
                    };
                    self.index_into(obj, idx_val)?;
                }
                Op::ListGetLL => {
                    let slot = self.read_u16()? as usize;
                    let idx_slot = self.read_u16()? as usize;
                    let base = self.current_frame().stack_base;
                    let slot_val = self.stack[base + slot];
                    let obj = if let Some(cell) = slot_val.as_cell() {
                        unsafe { (*cell).get() }
                    } else {
                        slot_val
                    };
                    let idx_raw = self.stack[base + idx_slot];
                    let idx_val = if let Some(cell) = idx_raw.as_cell() {
                        unsafe { (*cell).get() }
                    } else {
                        idx_raw
                    };
                    self.index_into(obj, idx_val)?;
                }
                Op::SetIndex => {
                    self.exec_set_index()?;
                }

                Op::EcsGet => {
                    self.exec_ecs_get()?;
                }
                Op::EcsSet => {
                    self.exec_ecs_set()?;
                }
                Op::EcsHas => {
                    self.exec_ecs_has()?;
                }
                Op::EcsSpawn => {
                    self.exec_ecs_spawn()?;
                }
                Op::EcsQuery => {
                    self.exec_ecs_query()?;
                }
                Op::LogicalLoad => {
                    self.exec_logical_load()?;
                }
                Op::LogicalStore => {
                    self.exec_logical_store()?;
                }
                Op::MaterializeAoS => {
                    self.exec_materialize_aos()?;
                }
                Op::ConcatN => {
                    let n = self.read_byte()? as usize;
                    if self.stack.len() < n {
                        return Err("ConcatN: stack underflow".to_string());
                    }
                    let base = self.stack.len() - n;
                    // Strict: every operand must be a string. f-string parts
                    // are routed through str()/format_value, and fused `+`
                    // chains can only succeed all-string anyway (rad has no
                    // implicit coercion) — a non-string here is the same
                    // type error binary `+` would have raised.
                    let mut total = 0usize;
                    for v in &self.stack[base..] {
                        match v.as_str() {
                            Some(s) => total += s.len(),
                            None => return Err(format!("Cannot add {} and str", v.type_name())),
                        }
                    }
                    let mut buf = String::with_capacity(total);
                    for v in &self.stack[base..] {
                        buf.push_str(v.as_str().unwrap());
                    }
                    // Parts stay rooted on the stack until the buffer owns
                    // their bytes; only then are they popped.
                    self.stack.truncate(base);
                    let out = Value::from_string(&mut self.gc, buf);
                    self.push(out);
                }
                Op::InitResource => {
                    self.exec_init_resource()?;
                }

                Op::MakeState => {
                    let machine_idx = self.read_u16()? as usize;
                    let state_idx = self.read_u16()? as usize;
                    let machine = helpers::constant_string(self.current_chunk(), machine_idx)?;
                    let state = helpers::constant_string(self.current_chunk(), state_idx)?;
                    let __v = Value::from_state(&mut self.gc, machine, state);
                    self.push(__v);
                }
                Op::Transition => {
                    self.exec_transition()?;
                }
                Op::MakeVariant => {
                    self.exec_make_variant()?;
                }

                Op::Emit => {
                    self.exec_emit()?;
                }
                Op::EmitAfter => {
                    self.exec_emit_after()?;
                }

                Op::RunSystem => {
                    self.exec_run_system()?;
                }
                Op::RunSchedule => {
                    self.exec_run_schedule_op()?;
                }

                Op::RunScheduleSerial => {
                    self.exec_run_schedule_serial_op()?;
                }
                Op::BeginSettlement => {
                    self.begin_settlement()?;
                }
                Op::EndSettlement => {
                    self.finish_settlement()?;
                }
                Op::ProposeIntent => {
                    let intent_idx = self.read_u16()? as usize;
                    let intent = helpers::constant_string(self.current_chunk(), intent_idx)?;
                    let payload = self.pop()?;
                    let frame = self.current_frame();
                    let line = self
                        .chunks
                        .get(frame.chunk_id)
                        .and_then(|chunk| chunk.lines.get(frame.ip.saturating_sub(3)).copied())
                        .unwrap_or(0);
                    self.propose_intent(&intent, payload, line)?;
                }
                Op::StageCandidate => {
                    let component = self.pop()?;
                    let entity = self.pop()?;
                    self.stage_candidate(entity, component)?;
                }

                Op::MatchState => {
                    let pattern_idx = self.read_u16()? as usize;
                    let jump_target = self.read_u16()? as usize;
                    let pattern = helpers::constant_string(self.current_chunk(), pattern_idx)?;
                    let subject = *self.peek()?;
                    let matches = if let Some(s) = subject.as_state() {
                        s.state == pattern
                    } else if let Some(st) = subject.as_sum_type() {
                        st.variant == pattern
                    } else {
                        false
                    };
                    if !matches {
                        self.current_frame_mut().ip = jump_target;
                    }
                }

                Op::Print => {
                    let argc = self.read_byte()? as usize;
                    let mut parts = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        parts.push(self.pop()?);
                    }
                    parts.reverse();
                    let s = parts
                        .iter()
                        .map(|v| v.print_display())
                        .collect::<Vec<_>>()
                        .join(" ");
                    self.print_buffer.push(s.clone());
                    if !self.suppress_output {
                        println!("{}", s);
                    }
                }
                Op::Len => {
                    let v = self.pop()?;
                    let n = if let Some(items) = v.as_list() {
                        items.len()
                    } else if let Some(t) = v.as_tuple() {
                        t.len()
                    } else if let Some(s) = v.as_str() {
                        s.chars().count()
                    } else if let Some(m) = v.as_map() {
                        m.len()
                    } else if let Some(bytes) = v.as_bytebuf() {
                        bytes.len()
                    } else {
                        return Err(format!("len() not defined for {}", v.type_name()));
                    };
                    let len_val = Value::from_int(&mut self.gc, n as i64);
                    self.push(len_val);
                }
                Op::TypeOf => {
                    let v = self.pop()?;
                    let tname = Value::from_string(&mut self.gc, v.type_name().to_string());
                    self.push(tname);
                }
                Op::Break => {
                    return Err(
                        "Opcode Break is unsupported: 'break' must be compiled to Jump".to_string(),
                    );
                }
                Op::GetFieldSlot => {
                    let slot = self.read_u16()? as usize;
                    let obj = self.pop()?;
                    if let Some(c) = obj.as_component() {
                        let v = c.values.get(slot).cloned().ok_or_else(|| {
                            format!("Field slot {} out of range for {}", slot, c.type_name)
                        })?;
                        self.push(v);
                    } else if let Some(st) = obj.as_sum_type() {
                        let key = (st.type_name.clone(), st.variant.clone());
                        if let Some(layout) = self.variant_layouts.get(&key) {
                            let field_name = layout.get(slot).ok_or_else(|| {
                                format!(
                                    "Field slot {} out of range for {}::{}",
                                    slot, st.type_name, st.variant
                                )
                            })?;
                            let v = st.fields.get(field_name).cloned().ok_or_else(|| {
                                format!(
                                    "Unknown field `{}` on {}::{}",
                                    field_name, st.type_name, st.variant
                                )
                            })?;
                            self.push(v);
                        } else {
                            return Err(format!(
                                "No layout for variant `{}::{}`",
                                st.type_name, st.variant
                            ));
                        }
                    } else {
                        return Err(format!(
                            "GetFieldSlot expected component or variant, got {}",
                            obj.type_name()
                        ));
                    }
                }
                Op::SetFieldSlot => {
                    let slot = self.read_u16()? as usize;
                    let val = self.pop()?;
                    let obj = self.pop()?;
                    let type_name = obj.type_name().to_string();
                    if let Some(mut c) = obj.into_component() {
                        if slot < c.values.len() {
                            c.values[slot] = val;
                            let __v = Value::from_component_data(&mut self.gc, c);
                            self.push(__v);
                        } else {
                            return Err(format!(
                                "Field slot {} out of range for {}",
                                slot, c.type_name
                            ));
                        }
                    } else {
                        return Err(format!(
                            "SetFieldSlot expected component, got {}",
                            type_name
                        ));
                    }
                }
                Op::MakeCompSlot => {
                    let type_idx = self.read_u16()? as usize;
                    let type_name = helpers::constant_string(self.current_chunk(), type_idx)?;
                    let field_count = self.read_u16()? as usize;
                    let layout = self
                        .component_layouts
                        .get(&type_name)
                        .ok_or_else(|| format!("No layout for component `{}`", type_name))?
                        .clone();
                    let mut vals: Vec<Value> = Vec::with_capacity(field_count);
                    for _ in 0..field_count {
                        vals.push(self.pop()?);
                    }
                    vals.reverse();
                    let __v = Value::component(&mut self.gc, type_name, layout, vals);
                    self.push(__v);
                }
                Op::QueryFilter => {
                    self.exec_query_filter()?;
                }
                Op::QueryProject => {
                    self.exec_query_project()?;
                }
                Op::Snapshot => {
                    self.exec_snapshot()?;
                }
                Op::Rollback => {
                    self.exec_rollback()?;
                }
                Op::BitsetSetInplace => {
                    self.exec_bitset_set_inplace()?;
                }
                Op::BitsetClearInplace => {
                    self.exec_bitset_clear_inplace()?;
                }
                Op::BufferAppendInplace => {
                    self.exec_buffer_append_inplace()?;
                }
                Op::ByteBufSetU8Inplace => {
                    self.exec_bytebuf_set_u8_inplace()?;
                }
                Op::ByteBufSetU32LeInplace => {
                    self.exec_bytebuf_set_u32_le_inplace()?;
                }
                Op::ByteBufSetI32LeInplace => {
                    self.exec_bytebuf_set_i32_le_inplace()?;
                }
                Op::GetIter => {
                    let val = self.pop()?;
                    if let Some(map) = val.as_map() {
                        let map_clone = map.clone();
                        let mut sorted_keys: Vec<MapKey> = map.keys().cloned().collect();
                        sorted_keys.sort();
                        let __v = Value::map_iter(&mut self.gc, map_clone, sorted_keys);
                        self.push(__v);
                    } else {
                        return Err(format!("GetIter expected map, got {}", val.type_name()));
                    }
                }
                Op::IterNext => {
                    let bindings_count = self.read_byte()?;
                    let iter_val = self.pop()?;
                    if let Some((map, idx_cell, keys)) = iter_val.as_map_iter() {
                        let idx = idx_cell.get();
                        if idx < keys.len() {
                            let k = &keys[idx];
                            let v = *map.get(k).unwrap();
                            idx_cell.set(idx + 1);

                            if bindings_count == 1 {
                                let key_v = k.to_value(&mut self.gc);
                                self.push(key_v);
                            } else {
                                let key_v = k.to_value(&mut self.gc);
                                self.push(key_v);
                                self.push(v);
                            }
                            self.push(Value::from_bool(true));
                        } else {
                            self.push(Value::from_bool(false));
                        }
                    } else {
                        return Err(format!(
                            "IterNext expected map iterator, got {}",
                            iter_val.type_name()
                        ));
                    }
                }
                // Stack order must match `Compiler::compile_lowered_pipeline`:
                // push item (top), then ListPushLocal <slot> — so pop item, then mutate list at slot.
                Op::ListPushLocal => {
                    let slot = self.read_u16()? as usize;
                    let elem = self.pop()?;
                    let base = self.current_frame().stack_base;
                    let slot_val = &mut self.stack[base + slot];

                    let mut list_val = if let Some(cell) = slot_val.as_cell() {
                        unsafe { (*cell).get() }
                    } else {
                        *slot_val
                    };

                    if let Some(crate::value::Object::List(list)) = list_val.as_object_mut() {
                        list.push(elem);
                    } else {
                        return Err(format!(
                            "ListPushLocal expected list at slot {}, got {}",
                            slot,
                            list_val.type_name()
                        ));
                    }

                    if let Some(cell) = slot_val.as_cell() {
                        unsafe { (*cell).set(list_val) };
                    } else {
                        *slot_val = list_val;
                    }
                }
                // Stack: index, value (top). Mutates the list living in the
                // local slot directly — no stack round-trip, no second Arc
                // reference, no copy-on-write clone. Only emitted for
                // `let unique` locals, whose aliasing freedom the checker
                // already guarantees.
                Op::ListSetLocal => {
                    let slot = self.read_u16()? as usize;
                    let val = self.pop()?;
                    let idx_val = self.pop()?;
                    let idx = helpers::index_as_usize(&idx_val)?;
                    let base = self.current_frame().stack_base;
                    let slot_val = &mut self.stack[base + slot];

                    let mut list_val = if let Some(cell) = slot_val.as_cell() {
                        unsafe { (*cell).get() }
                    } else {
                        *slot_val
                    };

                    if let Some(crate::value::Object::List(list)) = list_val.as_object_mut() {
                        list.set(idx, val)?;
                    } else {
                        return Err(format!(
                            "ListSetLocal expected list at slot {}, got {}",
                            slot,
                            list_val.type_name()
                        ));
                    }

                    if let Some(cell) = slot_val.as_cell() {
                        unsafe { (*cell).set(list_val) };
                    } else {
                        *slot_val = list_val;
                    }
                }
                Op::IsVariant => {
                    let pattern_idx = self.read_u16()? as usize;
                    let pattern = helpers::constant_string(self.current_chunk(), pattern_idx)?;
                    let val = self.pop()?;
                    let res = if let Some(st) = val.as_sum_type() {
                        st.variant == pattern
                    } else if let Some(s) = val.as_state() {
                        s.state == pattern
                    } else {
                        false
                    };
                    self.push(Value::from_bool(res));
                }
                Op::VecAdd => {
                    self.exec_vec_binary(helpers::binary_add)?;
                }
                Op::VecSub => {
                    self.exec_vec_binary(helpers::binary_sub)?;
                }
                Op::VecMul => {
                    self.exec_vec_binary(helpers::binary_mul)?;
                }
                Op::VecDiv => {
                    self.exec_vec_binary(helpers::binary_div)?;
                }
                Op::VecMod => {
                    self.exec_vec_binary(helpers::binary_mod)?;
                }
                Op::VecNeg => {
                    self.exec_vec_unary(helpers::unary_neg)?;
                }
                Op::VecNot => {
                    self.exec_vec_not()?;
                }
                Op::VecEq => {
                    self.exec_vec_cmp(|a, b| Ok(helpers::values_equal(a, b)))?;
                }
                Op::VecNeq => {
                    self.exec_vec_cmp(|a, b| Ok(!helpers::values_equal(a, b)))?;
                }
                Op::VecLt => {
                    self.exec_vec_cmp(helpers::cmp_lt)?;
                }
                Op::VecGt => {
                    self.exec_vec_cmp(helpers::cmp_gt)?;
                }
                Op::VecLte => {
                    self.exec_vec_cmp(helpers::cmp_lte)?;
                }
                Op::VecGte => {
                    self.exec_vec_cmp(helpers::cmp_gte)?;
                }
                Op::VecFilter => {
                    self.exec_vec_filter()?;
                }
                Op::VecSelect => {
                    self.exec_vec_select()?;
                }
                Op::LoadColumn => {
                    self.exec_load_column()?;
                }
                Op::VecBroadcast => {
                    self.exec_vec_broadcast()?;
                }

                Op::OnceGuardPass => {
                    self.once_guard_passed = true;
                }

                Op::PopCheckErr => {
                    let val = self.pop()?;
                    if let Some(st) = val.as_sum_type() {
                        if st.type_name == "Result" && st.variant == "Err" {
                            let msg = st
                                .fields
                                .get("value")
                                .or_else(|| st.fields.get("message"))
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "unknown error".to_string());
                            return Err(format!("Unhandled error from main(): {}", msg));
                        }
                        if st.type_name == "Option" && st.variant == "None" {
                            return Err("Unhandled None returned from main()".to_string());
                        }
                    }
                }

                Op::Halt => {
                    self.frames.clear();
                    return Ok(());
                }
            }
        }
    }

    pub(crate) fn exec_call(&mut self) -> Result<(), String> {
        let argc = self.read_byte()?;
        let argc_us = argc as usize;
        if self.stack.len() < argc_us + 1 {
            return Err("Stack underflow in Call".to_string());
        }
        let callee = self.stack[self.stack.len() - 1];
        if let Some(fv) = callee.as_fn() {
            if fv.arity != argc {
                return Err(format!(
                    "Arity mismatch: expected {}, got {}",
                    fv.arity, argc
                ));
            }
            if fv.chunk_id >= self.chunks.len() {
                return Err(format!("Invalid function chunk {}", fv.chunk_id));
            }
            if self.frames.len() >= MAX_CALL_DEPTH {
                return Err(format!(
                    "Stack overflow: exceeded {} call frames",
                    MAX_CALL_DEPTH
                ));
            }
            let chunk_id = fv.chunk_id;
            let slen = self.stack.len();
            self.stack.remove(slen - 1);
            let stack_base = self.stack.len() - argc_us;
            self.frames.push(CallFrame {
                chunk_id,
                ip: 0,
                stack_base,
                captures: None,
                system_writeback: None,
            });
        } else if let Some(cv) = callee.as_closure() {
            if cv.arity != argc {
                return Err(format!(
                    "Arity mismatch: expected {}, got {}",
                    cv.arity, argc
                ));
            }
            if cv.chunk_id >= self.chunks.len() {
                return Err(format!("Invalid closure chunk {}", cv.chunk_id));
            }
            if self.frames.len() >= MAX_CALL_DEPTH {
                return Err(format!(
                    "Stack overflow: exceeded {} call frames",
                    MAX_CALL_DEPTH
                ));
            }
            let captures = cv.captures.clone();
            let chunk_id = cv.chunk_id;
            let slen = self.stack.len();
            self.stack.remove(slen - 1);
            let stack_base = self.stack.len() - argc_us;
            self.frames.push(CallFrame {
                chunk_id,
                ip: 0,
                stack_base,
                captures: Some(Arc::new(captures)),
                system_writeback: None,
            });
        } else if let Some(builtin) = callee.as_builtin() {
            let slen = self.stack.len();
            self.stack.remove(slen - 1);
            let mut args = Vec::with_capacity(argc_us);
            for _ in 0..argc_us {
                args.push(self.pop()?);
            }
            args.reverse();
            let result = self.call_builtin(builtin, args)?;
            self.push(result);
        } else if let Some(native) = callee.as_native_fn() {
            let slen = self.stack.len();
            self.stack.remove(slen - 1);
            let mut args = Vec::with_capacity(argc_us);
            for _ in 0..argc_us {
                args.push(self.pop()?);
            }
            args.reverse();
            let raw_args: Vec<u64> = args.iter().map(|v| v.to_raw()).collect();
            let result_raw = unsafe { (native.func)(raw_args.as_ptr(), raw_args.len()) };
            if let Some(err) = crate::ffi::take_native_error() {
                return Err(err);
            }
            self.push(Value::from_raw(result_raw));
        } else {
            return Err(format!("Not callable: {}", callee.type_name()));
        }
        Ok(())
    }

    pub(crate) fn exec_async_call(&mut self) -> Result<(), String> {
        let argc = self.read_byte()?;
        let argc_us = argc as usize;
        if self.stack.len() < argc_us + 1 {
            return Err("Stack underflow in AsyncCall".to_string());
        }
        let callee = self.stack[self.stack.len() - 1];
        let slen = self.stack.len();
        self.stack.remove(slen - 1);
        let mut args = Vec::with_capacity(argc_us);
        for _ in 0..argc_us {
            args.push(self.pop()?);
        }
        args.reverse();

        let task_id = self.allocate_task_id();
        let previous_async = self.in_async_context;
        self.in_async_context = true;
        let result = self.call_value(&callee, args);
        self.in_async_context = previous_async;
        let status = match result {
            Ok(value) => TaskStatus::Completed(value),
            Err(err) => TaskStatus::Failed(err),
        };
        self.tasks.insert(
            task_id,
            TaskRecord {
                id: task_id,
                status,
            },
        );
        let __v = Value::from_task(&mut self.gc, task_id);
        self.push(__v);
        Ok(())
    }

    fn io_payload_to_value(&mut self, payload: IoTaskPayload) -> Value {
        match payload {
            IoTaskPayload::String(s) => Value::from_string(&mut self.gc, s),
            IoTaskPayload::Nil => Value::NIL,
            IoTaskPayload::Int(n) => Value::from_int(&mut self.gc, n),
            IoTaskPayload::StringList(items) => {
                let values = items
                    .into_iter()
                    .map(|s| Value::from_string(&mut self.gc, s))
                    .collect();
                Value::list(&mut self.gc, values)
            }
            IoTaskPayload::Bytes(bytes) => {
                let mut vec = Vec::with_capacity(bytes.len());
                for b in bytes {
                    vec.push(Value::from_int(&mut self.gc, b as i64));
                }
                Value::list(&mut self.gc, vec)
            }
            IoTaskPayload::ValueMap(pairs) => {
                let mut map = crate::value::MapStorage::new();
                for (k, v) in pairs {
                    map.insert(crate::value::MapKey::Str(k), self.io_payload_to_value(v));
                }
                Value::map(&mut self.gc, map)
            }
        }
    }

    pub(crate) fn exec_await(&mut self) -> Result<(), String> {
        let task_val = self.pop()?;
        let task_id = task_val
            .as_task()
            .ok_or_else(|| format!("Await expected task, got {}", task_val.type_name()))?;
        if let Some(rx) = self.pending_io.remove(&task_id) {
            match rx.recv() {
                Ok(Ok(payload)) => {
                    let value = self.io_payload_to_value(payload);
                    self.tasks.insert(
                        task_id,
                        TaskRecord {
                            id: task_id,
                            status: TaskStatus::Completed(value),
                        },
                    );
                    self.push(value);
                    return Ok(());
                }
                Ok(Err(err)) => {
                    self.tasks.insert(
                        task_id,
                        TaskRecord {
                            id: task_id,
                            status: TaskStatus::Failed(err.clone()),
                        },
                    );
                    return Err(format!("Task {} failed: {}", task_id, err));
                }
                Err(err) => {
                    return Err(format!(
                        "Task {} failed receiving IO result: {}",
                        task_id, err
                    ));
                }
            }
        }
        let record = self
            .tasks
            .get(&task_id)
            .ok_or_else(|| format!("Unknown task id {}", task_id))?;
        match &record.status {
            TaskStatus::Completed(value) => {
                self.push(*value);
                Ok(())
            }
            TaskStatus::Failed(err) => Err(format!("Task {} failed: {}", task_id, err)),
            TaskStatus::Ready => Err(format!("Task {} is not ready", task_id)),
        }
    }

    pub(crate) fn exec_closure(&mut self) -> Result<(), String> {
        let chunk_id = self.read_u16()? as usize;
        let arity = self.read_byte()?;
        let capture_count = self.read_byte()? as usize;
        let mut captures: Vec<*mut crate::gc::CaptureCell> = Vec::with_capacity(capture_count);
        for _ in 0..capture_count {
            let is_local = self.read_byte()? == 1;
            let index = self.read_u16()? as usize;
            let cell = if is_local {
                let base = self.current_frame().stack_base;
                let stack_idx = base + index;
                let slot = self
                    .stack
                    .get_mut(stack_idx)
                    .ok_or_else(|| format!("Invalid capture local {}", index))?;
                if let Some(existing_cell) = slot.as_cell() {
                    existing_cell
                } else {
                    let val = *slot;
                    let cell_ptr = self.gc.alloc(crate::gc::CaptureCell::new(val));
                    *slot = Value::from_cell(&mut self.gc, cell_ptr);
                    cell_ptr
                }
            } else {
                self.current_frame()
                    .captures
                    .as_ref()
                    .and_then(|c| c.get(index).copied())
                    .ok_or_else(|| format!("Invalid capture upvalue {}", index))?
            };
            captures.push(cell);
        }
        let name = self
            .chunks
            .get(chunk_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("<closure@{}>", chunk_id));
        let clo = Value::from_closure(
            &mut self.gc,
            ClosureValue {
                name,
                arity,
                chunk_id,
                captures,
            },
        );
        self.push(clo);
        Ok(())
    }

    pub(crate) fn exec_get_index(&mut self) -> Result<(), String> {
        let idx_val = self.pop()?;
        let obj = self.pop()?;
        self.index_into(obj, idx_val)
    }

    /// Shared indexing core for GetIndex and the fused ListGetLocal.
    #[inline]
    fn index_into(&mut self, obj: Value, idx_val: Value) -> Result<(), String> {
        if let Some(items) = obj.as_list() {
            let i = helpers::index_as_usize(&idx_val)?;
            let v = items
                .get(i)
                .cloned()
                .ok_or_else(|| format!("List index {} out of bounds", i))?;
            self.push(v);
        } else if let Some(s) = obj.as_str() {
            let i = helpers::index_as_usize(&idx_val)?;
            let b = s
                .as_bytes()
                .get(i)
                .ok_or_else(|| format!("String index {} out of bounds", i))?;
            let __v = Value::from_int(&mut self.gc, *b as i64);
            self.push(__v);
        } else if let Some(t) = obj.as_tuple() {
            let i = helpers::index_as_usize(&idx_val)?;
            let v = t
                .get(i)
                .cloned()
                .ok_or_else(|| format!("Tuple index {} out of bounds (len {})", i, t.len()))?;
            self.push(v);
        } else if let Some(m) = obj.as_map() {
            let map_key = MapKey::from_value(&idx_val)?;
            let v = m.get(&map_key).cloned().unwrap_or(Value::NIL);
            self.push(v);
        } else {
            return Err(format!(
                "GetIndex expected list, string, tuple, or map, got {}",
                obj.type_name()
            ));
        }
        Ok(())
    }

    pub(crate) fn exec_set_index(&mut self) -> Result<(), String> {
        let val = self.pop()?;
        let idx_val = self.pop()?;
        let obj = self.pop()?;
        if obj.as_list().is_some() {
            let i = helpers::index_as_usize(&idx_val)?;
            let mut list = obj.into_rad_list().expect("list type already checked");
            list.set(i, val)?;
            let __v = Value::from_rad_list(&mut self.gc, list);
            self.push(__v);
        } else if obj.as_map().is_some() {
            let map_key = MapKey::from_value(&idx_val)?;
            let mut new_map = obj.into_map().expect("map type already checked");
            new_map.insert(map_key, val);
            let __v = Value::map(&mut self.gc, new_map);
            self.push(__v);
        } else {
            return Err(format!(
                "SetIndex expected list or map, got {}",
                obj.type_name()
            ));
        }
        Ok(())
    }

    pub(crate) fn exec_ecs_get(&mut self) -> Result<(), String> {
        let type_idx = self.read_u16()? as usize;
        let ctype = helpers::constant_string(self.current_chunk(), type_idx)?;
        let ent = self.pop()?;
        let eid = helpers::entity_id(&ent)?;
        let comp = self
            .world
            .get_component(eid, &ctype)
            .ok_or_else(|| format!("Missing component `{}` on entity {}", ctype, eid))?;
        let __v = Value::from_component_data(&mut self.gc, comp);
        self.push(__v);
        Ok(())
    }

    pub(crate) fn exec_logical_load(&mut self) -> Result<(), String> {
        self.exec_ecs_get()
    }

    pub(crate) fn exec_ecs_set(&mut self) -> Result<(), String> {
        let comp_val = self.pop()?;
        let ent = self.pop()?;
        let eid = helpers::entity_id(&ent)?;
        let type_name = comp_val.type_name().to_string();
        let mut data = comp_val
            .into_component()
            .ok_or_else(|| format!("EcsSet expected component, got {}", type_name))?;
        self.sandbox_check_write(&data.type_name)?;
        // EcsSet is emitted only as the end-of-iteration writeback of a
        // `mut` query loop. The body may despawn the entity it is visiting
        // (the guide's TTL/particle cleanup idiom) or remove the bound
        // component; the writeback then has nothing to write to, by design —
        // the same rule the system writeback path applies on Op::Return.
        if !self.system_component_writeback_target_exists(eid, &data.type_name) {
            return Ok(());
        }
        Value::persist_component_data(&mut data);
        if self.is_worker {
            self.command_buffer
                .push(EcsCommand::SetComponent(eid, data));
        } else {
            let cname = data.type_name.clone();
            let summary = Self::component_summary(&data);
            // Data is already persisted above; the owned sink transfers
            // ownership instead of deep-copying a second time.
            if !self.get_world_mut().add_component_owned(eid, data) {
                return Err(format!(
                    "Cannot set component on non-existent entity {}",
                    eid
                ));
            }
            self.record_causal_write(Some(eid), &cname, crate::causality::WriteKind::Set, summary);
        }
        Ok(())
    }

    pub(crate) fn exec_logical_store(&mut self) -> Result<(), String> {
        self.exec_ecs_set()
    }

    pub(crate) fn exec_materialize_aos(&mut self) -> Result<(), String> {
        Ok(())
    }

    pub(crate) fn exec_ecs_has(&mut self) -> Result<(), String> {
        let type_idx = self.read_u16()? as usize;
        let ctype = helpers::constant_string(self.current_chunk(), type_idx)?;
        let ent = self.pop()?;
        let eid = helpers::entity_id(&ent)?;
        self.push(Value::from_bool(
            self.get_world().has_component(eid, &ctype),
        ));
        Ok(())
    }

    pub(crate) fn exec_ecs_spawn(&mut self) -> Result<(), String> {
        let n = self.read_byte()? as usize;
        let mut comps = Vec::with_capacity(n);
        for _ in 0..n {
            let v = self.pop()?;
            let type_name = v.type_name().to_string();
            if let Some(state) = v.as_state() {
                comps.push(ComponentData {
                    type_name: state.machine.clone(),
                    layout: std::sync::Arc::new(vec!["state".to_string()]),
                    values: vec![Value::from_string(&mut self.gc, state.state.clone())],
                });
            } else {
                let mut data = v.into_component().ok_or_else(|| {
                    format!("EcsSpawn expected component or state, got {}", type_name)
                })?;
                Value::persist_component_data(&mut data);
                comps.push(data);
            }
        }
        let name_source = self.read_byte()?;
        let dynamic_name: Option<String> = if name_source == 1 {
            let _placeholder = self.read_u16()?;
            let name_val = self.pop()?;
            match name_val.as_str() {
                Some(s) if !s.is_empty() => Some(s.to_string()),
                _ => None,
            }
        } else {
            let name_idx = self.read_u16()? as usize;
            let name_str = helpers::constant_string(self.current_chunk(), name_idx)?;
            if name_str.is_empty() {
                None
            } else {
                Some(name_str.to_string())
            }
        };
        let name_opt = dynamic_name.as_deref();
        if self.sandbox_caps.is_some() {
            for c in &comps {
                self.sandbox_check_write(&c.type_name)?;
            }
        }
        let eid = self.get_world_mut().spawn_entity(name_opt);
        if self.is_worker {
            let mut comps_clone = Vec::with_capacity(comps.len());
            for c in comps.iter().rev() {
                comps_clone.push(c.clone());
            }
            self.command_buffer.push(EcsCommand::SpawnEntity(
                name_opt.map(|s| s.to_string()),
                comps_clone,
                eid,
            ));
        } else {
            for c in comps.into_iter().rev() {
                let _ = self.get_world_mut().add_component(eid, c);
            }
        }
        let __v = Value::from_entity_id(&mut self.gc, eid);
        self.push(__v);
        Ok(())
    }

    pub(crate) fn exec_init_resource(&mut self) -> Result<(), String> {
        let type_idx = self.read_u16()? as usize;
        let field_count = self.read_u16()? as usize;
        let type_name = helpers::constant_string(self.current_chunk(), type_idx)?.to_string();
        let layout = self
            .component_layouts
            .get(&type_name)
            .cloned()
            .unwrap_or_else(|| std::sync::Arc::new(Vec::new()));
        let mut values = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            values.push(self.pop()?);
        }
        values.reverse();
        let data = ComponentData {
            type_name: type_name.clone(),
            layout,
            values,
        };
        self.get_world_mut().init_resource(&type_name, data);
        Ok(())
    }

    pub(crate) fn exec_ecs_query(&mut self) -> Result<(), String> {
        let with_count = self.read_byte()? as usize;
        let without_count = self.read_byte()? as usize;

        let mut with_types = Vec::with_capacity(with_count);
        for _ in 0..with_count {
            let v = self.pop()?;
            let s = v
                .as_str()
                .ok_or("EcsQuery: expected string component type")?
                .to_string();
            with_types.push(s);
        }
        with_types.reverse();

        let mut without_types = Vec::with_capacity(without_count);
        for _ in 0..without_count {
            let v = self.pop()?;
            let s = v
                .as_str()
                .ok_or("EcsQuery: expected string component type")?
                .to_string();
            without_types.push(s);
        }
        without_types.reverse();

        // A `query { A } without { B }` reveals which entities have A and lack
        // B, so both sides are reads and both honor the read ACL. No-op unless
        // the grant carries an explicit `"read"` allowlist.
        if self.sandbox_caps.is_some() {
            for ctype in with_types.iter().chain(without_types.iter()) {
                self.sandbox_check_read(ctype)?;
            }
        }

        let eids = self.get_world().query(&with_types, &without_types);
        let list = eids
            .into_iter()
            .map(|eid| Value::from_entity_id(&mut self.gc, eid))
            .collect();
        self.push_list_vec(list);
        Ok(())
    }

    pub(crate) fn exec_query_filter(&mut self) -> Result<(), String> {
        let comp_count = self.read_byte()? as usize;
        let filter_val = self.pop()?;
        let (filter_chunk_id, captures_arc) = if let Some(cv) = filter_val.as_closure() {
            (cv.chunk_id, Some(Arc::new(cv.captures.clone())))
        } else if let Some(fv) = filter_val.as_fn() {
            (fv.chunk_id, None)
        } else {
            return Err("QueryFilter: expected closure or function".to_string());
        };
        let mut comp_types = Vec::with_capacity(comp_count);
        for _ in 0..comp_count {
            let v = self.pop()?;
            let s = v
                .as_str()
                .ok_or("QueryFilter: expected string component type")?
                .to_string();
            comp_types.push(s);
        }
        comp_types.reverse();
        let entity_list = self.pop()?;
        let entities = entity_list
            .into_rad_list()
            .ok_or("QueryFilter: expected entity list")?
            .into_vec();

        let mut result = Vec::new();
        for entity_val in entities.into_iter() {
            let eid = entity_val
                .as_entity_id()
                .ok_or("QueryFilter: expected entity id")?;

            let saved_depth = self.frames.len();
            let stack_base = self.stack.len();

            self.push(entity_val);
            for ctype in &comp_types {
                if let Some(comp) = self.get_world().get_component(eid, ctype) {
                    let __v = Value::from_component_data(&mut self.gc, comp);
                    self.push(__v);
                } else {
                    self.push(Value::NIL);
                }
            }

            self.frames.push(CallFrame {
                chunk_id: filter_chunk_id,
                ip: 0,
                stack_base,
                captures: captures_arc.clone(),
                system_writeback: None,
            });
            self.run_frames(saved_depth)?;

            let keep = self.pop()?.is_truthy();
            self.stack.truncate(stack_base);

            if keep {
                result.push(entity_val);
            }
        }
        self.push_list_vec(result);
        Ok(())
    }

    pub(crate) fn exec_query_project(&mut self) -> Result<(), String> {
        let select_count = self.read_byte()? as usize;
        let mut select_types = Vec::with_capacity(select_count);
        for _ in 0..select_count {
            let v = self.pop()?;
            let s = v
                .as_str()
                .ok_or("QueryProject: expected string component type")?
                .to_string();
            select_types.push(s);
        }
        select_types.reverse();
        let entity_list = self.pop()?;
        let entities = entity_list
            .into_rad_list()
            .ok_or("QueryProject: expected entity list")?
            .into_vec();

        let mut result = Vec::new();
        for entity_val in entities {
            let eid = entity_val
                .as_entity_id()
                .ok_or("QueryProject: expected entity id")?;

            if select_types.len() == 1 {
                if let Some(comp) = self.get_world().get_component(eid, &select_types[0]) {
                    result.push(Value::from_component_data(&mut self.gc, comp));
                } else {
                    result.push(Value::NIL);
                }
            } else {
                let mut fields = Vec::with_capacity(select_types.len());
                for ctype in &select_types {
                    if let Some(comp) = self.get_world().get_component(eid, ctype) {
                        fields.push(Value::from_component_data(&mut self.gc, comp));
                    } else {
                        fields.push(Value::NIL);
                    }
                }
                result.push(Value::tuple(&mut self.gc, fields));
            }
        }
        self.push_list_vec(result);
        Ok(())
    }

    pub(crate) fn exec_snapshot(&mut self) -> Result<(), String> {
        let snapshot = self.get_world().snapshot();
        self.timeline.push(snapshot);
        Ok(())
    }

    pub(crate) fn exec_rollback(&mut self) -> Result<(), String> {
        if let Some(snapshot) = self.timeline.pop() {
            self.get_world_mut().restore(snapshot);
            self.push(Value::from_bool(true));
        } else {
            self.push(Value::from_bool(false));
        }
        Ok(())
    }

    pub(crate) fn exec_transition(&mut self) -> Result<(), String> {
        let event_idx = self.read_u16()? as usize;
        let event = helpers::constant_string(self.current_chunk(), event_idx)?;
        let inst = self.pop()?;
        let s = inst
            .as_state()
            .ok_or_else(|| format!("Transition expected state, got {}", inst.type_name()))?;
        let machine = s.machine.clone();
        let state = s.state.clone();
        let result = self.transition_result(machine, state, event)?;
        self.push(result);
        Ok(())
    }

    pub(crate) fn transition_result(
        &mut self,
        machine: String,
        state: String,
        event: String,
    ) -> Result<Value, String> {
        let transitions = self
            .state_machines
            .get(&machine)
            .and_then(|m| m.get(&state))
            .cloned();
        match transitions {
            Some(trans) => {
                for transition in trans {
                    if transition.event != event {
                        continue;
                    }
                    if let Some(guard_chunk_id) = transition.guard_chunk_id {
                        let guard_ok = self.eval_state_guard(guard_chunk_id)?;
                        if !guard_ok {
                            let mut fields = HashMap::new();
                            fields.insert(
                                "message".to_string(),
                                Value::from_string(
                                    &mut self.gc,
                                    format!(
                                        "Guard failed for '{}' from '{}::{}'",
                                        event, machine, state
                                    ),
                                ),
                            );
                            return Ok(Value::sum_type(
                                &mut self.gc,
                                "Result".to_string(),
                                "Err".to_string(),
                                fields,
                            ));
                        }
                    }
                    let new_state =
                        Value::from_state(&mut self.gc, machine.clone(), transition.target.clone());
                    let mut fields = HashMap::new();
                    fields.insert("value".to_string(), new_state);
                    return Ok(Value::sum_type(
                        &mut self.gc,
                        "Result".to_string(),
                        "Ok".to_string(),
                        fields,
                    ));
                }
                let mut fields = HashMap::new();
                fields.insert(
                    "message".to_string(),
                    Value::from_string(
                        &mut self.gc,
                        format!(
                            "No transition on '{}' from state '{}::{}'",
                            event, machine, state
                        ),
                    ),
                );
                Ok(Value::sum_type(
                    &mut self.gc,
                    "Result".to_string(),
                    "Err".to_string(),
                    fields,
                ))
            }
            None => {
                let mut fields = HashMap::new();
                fields.insert(
                    "message".to_string(),
                    Value::from_string(
                        &mut self.gc,
                        format!("No state machine '{}' state '{}'", machine, state),
                    ),
                );
                Ok(Value::sum_type(
                    &mut self.gc,
                    "Result".to_string(),
                    "Err".to_string(),
                    fields,
                ))
            }
        }
    }

    pub(crate) fn eval_state_guard(&mut self, guard_chunk_id: usize) -> Result<bool, String> {
        if guard_chunk_id >= self.chunks.len() {
            return Err(format!("Invalid guard chunk id {}", guard_chunk_id));
        }
        let saved_depth = self.frames.len();
        let stack_base = self.stack.len();
        self.frames.push(CallFrame {
            chunk_id: guard_chunk_id,
            ip: 0,
            stack_base,
            captures: None,
            system_writeback: None,
        });
        self.run_frames(saved_depth)?;
        let value = self.pop()?;
        self.stack.truncate(stack_base);
        Ok(value.is_truthy())
    }

    /// `schedule serial [...]` (dogfood feature seq 83): same operands as
    /// `RunSchedule`, but every system runs one at a time in topological
    /// order — no worker snapshots, no merge. The per-call spelling of the
    /// global `--serial-schedule` lever.
    pub(crate) fn exec_run_schedule_serial_op(&mut self) -> Result<(), String> {
        let count = self.read_u16()? as usize;
        let mut systems = Vec::with_capacity(count);
        for _ in 0..count {
            let idx = self.read_u16()? as usize;
            systems.push(helpers::constant_resolved_system_name(
                self.current_chunk(),
                idx,
            )?);
        }
        let ordered = self.build_system_schedule(&systems)?;
        for name in &ordered {
            self.run_system_by_name(name)?;
        }
        self.bi_flush_events(vec![])?;
        Ok(())
    }

    pub(crate) fn exec_run_schedule_op(&mut self) -> Result<(), String> {
        let count = self.read_u16()? as usize;
        let mut systems = Vec::with_capacity(count);
        for _ in 0..count {
            let idx = self.read_u16()? as usize;
            systems.push(helpers::constant_resolved_system_name(
                self.current_chunk(),
                idx,
            )?);
        }
        let ordered = self.build_system_schedule(&systems)?;

        // `--serial-schedule`: run every system one at a time in topological
        // order — no worker snapshots, no merge (dogfood feature seq 83). The
        // single-system batch path below already runs serially; this is that
        // path for the whole schedule, the correctness-critical / differential-
        // test mode. Explicit simulate_par/simulate_many are unaffected.
        if self.serial_schedule {
            for name in &ordered {
                self.run_system_by_name(name)?;
            }
            // Same end-of-schedule flush as the parallel path below: the
            // differential test only works if the two modes are observably
            // identical apart from execution order.
            self.bi_flush_events(vec![])?;
            return Ok(());
        }

        let batches = parallel::partition_parallel_batches(&ordered, &self.systems)?;

        for batch in batches {
            if batch.len() == 1 {
                self.run_system_by_name(&batch[0])?;
            } else {
                let snapshot = self.world.snapshot();
                let shared = self.shared_state();

                let run_one = |name: &String| {
                    WORKER_VM.with(|cell| {
                        let mut opt = cell.borrow_mut();
                        if opt.is_none() {
                            *opt = Some(crate::vm::VM::from_shared_state(shared.clone()));
                        }
                        let worker = opt.as_mut().unwrap();
                        worker.sync_from_shared(&shared);
                        worker.world.restore(snapshot.clone());
                        // Determinism (spec §7.2): pooled worker VMs are
                        // reused across tasks by whatever rayon thread picks
                        // the task up, so any counter that survives reuse
                        // makes the run depend on thread scheduling. Trace
                        // ids restart at 1 per task (the merge below sorts by
                        // them, then renumbers on the main timeline); the rng
                        // restarts from the schedule-time seed.
                        worker.next_trace_id = 1;
                        worker.rng_state = shared.rng_state;

                        worker.run_system_by_name(name)?;
                        let cmds = std::mem::take(&mut worker.command_buffer);
                        let evts = std::mem::take(&mut worker.events_next);
                        Ok(crate::vm::WorkerResult { cmds, evts })
                    })
                };
                // wasm32 has no threads: rayon's pool creation would trap, so
                // the batch runs sequentially (same worker-VM isolation).
                #[cfg(target_arch = "wasm32")]
                let results: Vec<Result<crate::vm::WorkerResult, String>> =
                    batch.iter().map(run_one).collect();
                #[cfg(not(target_arch = "wasm32"))]
                let results: Vec<Result<crate::vm::WorkerResult, String>> =
                    batch.par_iter().map(run_one).collect();

                // Carry the originating system name so merged writes and
                // events keep their causal attribution on the main VM.
                let mut all_evts: Vec<(String, Value, u64, String)> = Vec::new();
                // `accum` resources (dogfood seq 83 IDEA 02): several systems
                // in this batch may have folded into the same resource. Each
                // worker saw the same base snapshot, so its final value is
                // base + its own contribution; the merge sums the per-field
                // DELTAS onto the base, in schedule order (deterministic,
                // also for floats). Entries: (resource, last contributor,
                // folded value).
                let mut accum_state: Vec<(String, String, crate::value::ComponentData)> =
                    Vec::new();
                for (sys_name, res) in batch.iter().zip(results) {
                    let wr = res?;
                    let cmds = wr.cmds;
                    let accum_of_sys = self
                        .systems
                        .get(sys_name)
                        .map(|i| i.accum_resources.clone())
                        .unwrap_or_default();
                    let evts = wr
                        .evts
                        .into_iter()
                        .map(|(name, payload, trace_id)| {
                            (
                                name,
                                payload.deep_copy(&mut self.gc),
                                trace_id,
                                sys_name.clone(),
                            )
                        })
                        .collect::<Vec<_>>();
                    let prev_cause = std::mem::replace(
                        &mut self.current_cause,
                        crate::causality::Cause::System {
                            name: sys_name.clone(),
                        },
                    );
                    let mut eid_map = HashMap::new();
                    // This system's FINAL value per accum resource (a
                    // per-entity system re-emits the writeback each
                    // iteration; only the last one is its contribution).
                    let mut sys_accum_last: Vec<(String, crate::value::ComponentData)> = Vec::new();
                    for cmd in cmds {
                        match cmd {
                            crate::vm::EcsCommand::SetResource(name, data)
                                if accum_of_sys.contains(&name) =>
                            {
                                match sys_accum_last.iter_mut().find(|(n, _)| *n == name) {
                                    Some(slot) => slot.1 = data,
                                    None => sys_accum_last.push((name, data)),
                                }
                            }
                            crate::vm::EcsCommand::SetComponent(eid, data) => {
                                let real_eid = eid_map.get(&eid).copied().unwrap_or(eid);
                                let cname = data.type_name.clone();
                                let summary = Self::component_summary(&data);
                                // Commands buffer persisted data; the owned
                                // sinks take ownership without re-copying.
                                let _ = self.get_world_mut().add_component_owned(real_eid, data);
                                self.record_causal_write(
                                    Some(real_eid),
                                    &cname,
                                    crate::causality::WriteKind::Set,
                                    summary,
                                );
                            }
                            crate::vm::EcsCommand::SetResource(name, data) => {
                                let summary = Self::component_summary(&data);
                                self.get_world_mut().set_resource_owned(&name, data);
                                self.record_causal_write(
                                    None,
                                    &name,
                                    crate::causality::WriteKind::Resource,
                                    summary,
                                );
                            }
                            crate::vm::EcsCommand::SpawnEntity(name, comps, local_eid) => {
                                let real_eid = self.get_world_mut().spawn_entity(name.as_deref());
                                if real_eid != local_eid {
                                    eid_map.insert(local_eid, real_eid);
                                }
                                for c in comps {
                                    let cname = c.type_name.clone();
                                    let summary = Self::component_summary(&c);
                                    let _ = self.get_world_mut().add_component_owned(real_eid, c);
                                    self.record_causal_write(
                                        Some(real_eid),
                                        &cname,
                                        crate::causality::WriteKind::Spawn,
                                        summary,
                                    );
                                }
                            }
                            crate::vm::EcsCommand::RemoveComponent(eid, ctype) => {
                                let real_eid = eid_map.get(&eid).copied().unwrap_or(eid);
                                self.get_world_mut().remove_component(real_eid, &ctype);
                                self.record_causal_write(
                                    Some(real_eid),
                                    &ctype,
                                    crate::causality::WriteKind::Remove,
                                    String::new(),
                                );
                            }
                            crate::vm::EcsCommand::DespawnEntity(eid) => {
                                let real_eid = eid_map.get(&eid).copied().unwrap_or(eid);
                                // Capture the name before destroy wipes it.
                                self.record_causal_write(
                                    Some(real_eid),
                                    "*",
                                    crate::causality::WriteKind::Despawn,
                                    String::new(),
                                );
                                self.get_world_mut().destroy_entity(real_eid);
                            }
                        }
                    }
                    // Fold this system's accum contributions: delta against
                    // the batch's base snapshot, summed field-by-field.
                    for (rname, contrib) in sys_accum_last {
                        let base = snapshot.get_resource(&rname);
                        match accum_state.iter_mut().find(|(n, _, _)| *n == rname) {
                            Some((_, contributor, acc)) => {
                                *contributor = sys_name.clone();
                                if let Some(base) = base {
                                    fold_accum_delta(acc, &base, &contrib);
                                } else {
                                    // No base to delta against (undeclared
                                    // resource — unreachable in checked
                                    // programs): last write wins.
                                    *acc = contrib;
                                }
                            }
                            None => {
                                accum_state.push((rname, sys_name.clone(), contrib));
                            }
                        }
                    }
                    self.current_cause = prev_cause;
                    all_evts.extend(evts);
                }
                // Apply the folded accum resources once, after every
                // contributor has been merged (schedule order preserved).
                for (rname, contributor, data) in accum_state {
                    let summary = Self::component_summary(&data);
                    let prev_cause = std::mem::replace(
                        &mut self.current_cause,
                        crate::causality::Cause::System { name: contributor },
                    );
                    self.get_world_mut().set_resource_owned(&rname, data);
                    self.record_causal_write(
                        None,
                        &rname,
                        crate::causality::WriteKind::Resource,
                        summary,
                    );
                    self.current_cause = prev_cause;
                }
                // Deterministic ordering for events emitted in parallel
                // (trace id, then name). Worker trace ids restart at 1 per
                // task, so the id is the per-system emission index and the
                // stable sort breaks remaining ties by schedule order.
                all_evts.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
                for (name, payload, _worker_trace_id, sys_name) in all_evts {
                    // Re-id on the main timeline: worker-local ids collide
                    // across workers and with ids the main VM already used.
                    let trace_id = self.next_trace_id;
                    self.next_trace_id += 1;
                    let summary = crate::causality::summarize(&self.ledger_payload(&payload));
                    let emit_id = self.ledger.record_emit(
                        self.causality_frame,
                        &name,
                        summary,
                        crate::causality::Cause::System { name: sys_name },
                    );
                    self.emit_ids_next.push(emit_id);
                    self.events_next.push((name, payload, trace_id));
                }
            }
        }
        self.bi_flush_events(vec![])?;
        Ok(())
    }

    pub(crate) fn build_system_schedule(
        &self,
        system_names: &[String],
    ) -> Result<Vec<String>, String> {
        let name_set: HashSet<&str> = system_names.iter().map(|s| s.as_str()).collect();
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        for name in system_names {
            graph.insert(name.clone(), Vec::new());
        }
        for name in system_names {
            let info = self
                .systems
                .get(name)
                .ok_or_else(|| format!("Unknown system '{}'", name))?;
            for dep in &info.after {
                if name_set.contains(dep.as_str()) {
                    graph.entry(name.clone()).or_default().push(dep.clone());
                }
            }
            for dep in &info.before {
                if name_set.contains(dep.as_str()) {
                    graph.entry(dep.clone()).or_default().push(name.clone());
                }
            }
        }
        let mut result = Vec::with_capacity(system_names.len());
        let mut visited = HashSet::new();
        let mut visiting = HashSet::new();
        for name in system_names {
            self.visit_schedule_node(name, &graph, &mut visited, &mut visiting, &mut result)?;
        }
        Ok(result)
    }

    pub(crate) fn visit_schedule_node(
        &self,
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) -> Result<(), String> {
        if visiting.contains(node) {
            return Err(format!(
                "Circular system dependency detected involving '{}'",
                node
            ));
        }
        if visited.contains(node) {
            return Ok(());
        }
        visiting.insert(node.to_string());
        if let Some(deps) = graph.get(node) {
            for dep in deps {
                self.visit_schedule_node(dep, graph, visited, visiting, result)?;
            }
        }
        visiting.remove(node);
        visited.insert(node.to_string());
        result.push(node.to_string());
        Ok(())
    }

    pub(crate) fn dispatch_event(
        &mut self,
        event_name: &str,
        event_data: Value,
    ) -> Result<(), String> {
        if let Some(handlers) = Arc::make_mut(&mut self.event_handlers).get_mut(event_name) {
            let to_run: Vec<(usize, u16, bool, bool)> = handlers
                .iter()
                .filter(|h| !h.once || !h.fired)
                .map(|h| (h.chunk_id, h.param_slot, h.once, h.has_guard))
                .collect();
            for (chunk_id, param_slot, is_once, has_guard) in to_run {
                let saved_guard_flag = self.once_guard_passed;
                if is_once && has_guard {
                    self.once_guard_passed = false;
                }
                let saved_depth = self.frames.len();
                let stack_base = self.stack.len();
                for _ in 0..param_slot {
                    self.push(Value::NIL);
                }
                self.push(event_data);
                self.frames.push(CallFrame {
                    chunk_id,
                    ip: 0,
                    stack_base,
                    captures: None,
                    system_writeback: None,
                });
                self.run_frames(saved_depth)?;
                let guard_passed = self.once_guard_passed;
                self.once_guard_passed = saved_guard_flag;
                if is_once && (!has_guard || guard_passed) {
                    if let Some(hs) = Arc::make_mut(&mut self.event_handlers).get_mut(event_name) {
                        if let Some(h) = hs.iter_mut().find(|h| h.chunk_id == chunk_id && h.once) {
                            h.fired = true;
                        }
                    }
                }
                self.stack.truncate(stack_base);
            }
        }
        Ok(())
    }

    pub(crate) fn run_system_by_name(&mut self, sys_name: &str) -> Result<(), String> {
        // Causality: writes performed by this system (writebacks included)
        // are attributed to it.
        let prev_cause = std::mem::replace(
            &mut self.current_cause,
            crate::causality::Cause::System {
                name: sys_name.to_string(),
            },
        );
        let res = self.run_system_by_name_impl(sys_name);
        self.current_cause = prev_cause;
        res
    }

    fn run_system_by_name_impl(&mut self, sys_name: &str) -> Result<(), String> {
        self.arena.reset();
        let info = self
            .systems
            .get(sys_name)
            .cloned()
            .ok_or_else(|| format!("Unknown system '{}'", sys_name))?;
        // Sandbox gate: a system whose signature declares `mut` access to a
        // component outside the capability grant is rejected before it runs.
        // This single check covers all four writeback paths in run_frames.
        // "__body_" entries are scheduler-only metadata (body writes found
        // by conflict analysis); the actual writes they describe are still
        // capability-checked at execution time.
        if let Some(caps) = &self.sandbox_caps {
            for (pname, is_mut, ctype) in info.params.iter().chain(info.resource_params.iter()) {
                if pname.starts_with("__body_") {
                    continue;
                }
                if *is_mut && !caps.may_write(ctype) {
                    return Err(format!(
                        "sandbox: system '{}' declares mutable access to component '{}' denied by capability grant",
                        sys_name, ctype
                    ));
                }
                // A non-mut param still injects the component value into the
                // system body, so a read param is a read of that component
                // and must honor the read ACL (confidentiality dimension).
                if !is_mut && !caps.may_read(ctype) {
                    return Err(format!(
                        "sandbox: system '{}' reads component '{}' denied by capability grant",
                        sys_name, ctype
                    ));
                }
            }
        }
        let resource_only = info.params.is_empty();
        let ctypes: Vec<String> = info.params.iter().map(|(_, _, t)| t.clone()).collect();
        let eids = if resource_only {
            vec![0_u32]
        } else {
            self.get_world().query(&ctypes, &[])
        };
        for eid in eids {
            let saved_depth = self.frames.len();
            let stack_base = self.stack.len();
            for (_pname, _is_mut, ctype) in &info.params {
                if let Some(comp) = self.get_world().get_component(eid, ctype) {
                    let __v = Value::from_component_data(&mut self.gc, comp);
                    self.push(__v);
                }
            }
            // "__body_" resource entries are scheduler-only metadata — they
            // are never injected as params, so they must not shift the slot
            // layout here or in the writeback registration below.
            for (_pname, _is_mut, rtype) in info
                .resource_params
                .iter()
                .filter(|(pname, _, _)| !pname.starts_with("__body_"))
            {
                if let Some(res) = self.get_world().get_resource(rtype) {
                    let __v = Value::from_component_data(&mut self.gc, res);
                    self.push(__v);
                }
            }
            if resource_only {
                self.push(Value::NIL);
            } else {
                let __v = Value::from_entity_id(&mut self.gc, eid);
                self.push(__v);
            }
            let mut mutable_params = Vec::new();
            for (idx, (_pname, is_mut, ctype)) in info.params.iter().enumerate() {
                if *is_mut {
                    mutable_params.push((idx as u16, ctype.clone()));
                }
            }
            let mut mutable_resources = Vec::new();
            for (idx, (_pname, is_mut, rtype)) in info
                .resource_params
                .iter()
                .filter(|(pname, _, _)| !pname.starts_with("__body_"))
                .enumerate()
            {
                if *is_mut {
                    mutable_resources.push(((info.params.len() + idx) as u16, rtype.clone()));
                }
            }
            self.frames.push(CallFrame {
                chunk_id: info.chunk_id,
                ip: 0,
                stack_base,
                captures: None,
                system_writeback: Some(SystemWriteback {
                    entity_id: eid,
                    mutable_params,
                    mutable_resources,
                }),
            });
            self.run_frames(saved_depth)?;
            self.stack.truncate(stack_base);
        }
        Ok(())
    }

    pub(crate) fn exec_make_variant(&mut self) -> Result<(), String> {
        let type_idx = self.read_u16()? as usize;
        let variant_idx = self.read_u16()? as usize;
        let field_count = self.read_u16()? as usize;
        let type_name = helpers::constant_string(self.current_chunk(), type_idx)?;
        let variant = helpers::constant_string(self.current_chunk(), variant_idx)?;
        let mut fields = HashMap::new();
        for _ in 0..field_count {
            let val = self.pop()?;
            let name_val = self.pop()?;
            let name = name_val.as_str().map(|s| s.to_string()).ok_or_else(|| {
                format!(
                    "Variant field name must be string, got {}",
                    name_val.type_name()
                )
            })?;
            fields.insert(name, val);
        }
        let __v = Value::sum_type(&mut self.gc, type_name, variant, fields);
        self.push(__v);
        Ok(())
    }

    /// `emit E { .. } after N` — queue the popped event to fire after N
    /// event-flush cycles. A delay of zero (or less) is an ordinary emit.
    pub(crate) fn exec_emit_after(&mut self) -> Result<(), String> {
        let event_data = self.pop()?;
        let delay_val = self.pop()?;
        let delay = delay_val.as_int().ok_or_else(|| {
            format!(
                "emit ... after expects an int tick count, got {}",
                delay_val.type_name()
            )
        })?;
        if self.is_worker {
            return Err(
                "emit ... after is not supported inside parallel system batches yet — emit it from a handler or a single-system schedule".to_string(),
            );
        }
        if delay <= 0 {
            self.push(event_data);
            return self.exec_emit();
        }
        let event_name = event_data.type_name().to_string();
        let emit_id = if self.is_worker || self.in_simulation_fork > 0 {
            0
        } else {
            let payload = crate::causality::summarize(&self.ledger_payload(&event_data));
            self.ledger.record_emit(
                self.causality_frame,
                &event_name,
                payload,
                self.current_cause.clone(),
            )
        };
        // GC-heap payload like every queued event; collect_cycles roots
        // the delayed queue so it survives until its tick.
        self.delayed_events
            .push((delay, event_name, event_data, emit_id));
        Ok(())
    }

    pub(crate) fn exec_emit(&mut self) -> Result<(), String> {
        let event_data = self.pop()?;

        let event_name = event_data.type_name().to_string();

        let trace_id = if let Some(tid) = self.current_trace_id {
            tid
        } else {
            let tid = self.next_trace_id;
            self.next_trace_id += 1;
            tid
        };

        // Inside simulate() the event queues are the *simulation's own*
        // (saved and restored around the run), so emits enqueue normally:
        // they fire on later simulated ticks or travel with the result fork
        // as in-flight leftovers. They used to be silently dropped here —
        // the same hole class the composition pass closed at fork/commit.
        //
        // Causality: every main-timeline event *instance* gets an emit
        // record carrying who emitted it; handler writes link back through
        // this id. Workers and simulations push 0 — the ledger describes
        // the main timeline only.
        let emit_id = if self.is_worker || self.in_simulation_fork > 0 {
            0
        } else {
            let payload = crate::causality::summarize(&self.ledger_payload(&event_data));
            self.ledger.record_emit(
                self.causality_frame,
                &event_name,
                payload,
                self.current_cause.clone(),
            )
        };
        self.emit_ids_next.push(emit_id);
        self.events_next.push((event_name, event_data, trace_id));
        Ok(())
    }

    pub(crate) fn exec_run_system(&mut self) -> Result<(), String> {
        let name_idx = self.read_u16()? as usize;
        let sys_name = helpers::constant_resolved_system_name(self.current_chunk(), name_idx)?;
        self.run_system_by_name(&sys_name)?;
        self.bi_flush_events(vec![])?;
        Ok(())
    }

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

    pub fn call_value(&mut self, callee: &Value, args: Vec<Value>) -> Result<Value, String> {
        // A host call starts with no active frames and owns any settlement it
        // opens. If execution escapes before EndSettlement, unwind it here.
        // Nested VM calls (notably resolver invocation) preserve both their
        // causal context and frames so the outer boundary can render the full
        // runtime call chain before aborting.
        let frame_depth = self.frames.len();
        let stack_depth = self.stack.len();
        let owns_execution_boundary = frame_depth == 0;
        let result = self.call_value_inner(callee, args);
        if result.is_err() && owns_execution_boundary {
            self.frames.truncate(frame_depth);
            self.stack.truncate(stack_depth);
            self.abort_settlement();
        }
        result
    }

    fn call_value_inner(&mut self, callee: &Value, args: Vec<Value>) -> Result<Value, String> {
        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err(format!(
                "Stack overflow: exceeded {} call frames",
                MAX_CALL_DEPTH
            ));
        }
        if let Some(fv) = callee.as_fn() {
            if fv.chunk_id >= self.chunks.len() {
                return Err(format!("Invalid function chunk {}", fv.chunk_id));
            }
            let saved_depth = self.frames.len();
            let args_len = args.len();
            for arg in args {
                self.push(arg);
            }
            let stack_base = self.stack.len() - args_len;
            self.frames.push(CallFrame {
                chunk_id: fv.chunk_id,
                ip: 0,
                stack_base,
                captures: None,
                system_writeback: None,
            });
            self.run_frames(saved_depth)?;
            self.pop()
        } else if let Some(cv) = callee.as_closure() {
            if cv.chunk_id >= self.chunks.len() {
                return Err(format!("Invalid closure chunk {}", cv.chunk_id));
            }
            let saved_depth = self.frames.len();
            let args_len = args.len();
            for arg in args {
                self.push(arg);
            }
            let stack_base = self.stack.len() - args_len;
            self.frames.push(CallFrame {
                chunk_id: cv.chunk_id,
                ip: 0,
                stack_base,
                captures: Some(std::sync::Arc::new(cv.captures.clone())),
                system_writeback: None,
            });
            self.run_frames(saved_depth)?;
            self.pop()
        } else if let Some(builtin) = callee.as_builtin() {
            self.call_builtin(builtin, args)
        } else if let Some(native) = callee.as_native_fn() {
            let raw_args: Vec<u64> = args.iter().map(|v| v.to_raw()).collect();
            let result_raw = unsafe { (native.func)(raw_args.as_ptr(), raw_args.len()) };
            if let Some(err) = crate::ffi::take_native_error() {
                return Err(err);
            }
            Ok(Value::from_raw(result_raw))
        } else {
            Err(format!("Not callable: {}", callee.type_name()))
        }
    }

    pub(crate) fn exec_bitset_set_inplace(&mut self) -> Result<(), String> {
        let idx_val = self.pop()?;
        let mut bs_val = self.pop()?;
        let idx = idx_val
            .as_int()
            .ok_or_else(|| "bitset_set expects an integer as second argument".to_string())?;
        if idx < 0 {
            self.push(bs_val);
            return Ok(());
        }
        if idx > 100_000_000 {
            return Err(format!(
                "bitset_set index out of bounds: {} (max 100,000,000)",
                idx
            ));
        }

        let word_idx = (idx / 64) as usize;
        if let Some(crate::value::Object::BitSet(words)) = bs_val.as_object_mut() {
            if word_idx >= words.len() {
                let mut new_cap = if words.is_empty() { 8 } else { words.len() };
                while new_cap <= word_idx {
                    new_cap *= 2;
                }
                words.resize(new_cap, 0);
            }
            words[word_idx] |= 1 << (idx % 64);
        }
        self.push(bs_val);
        Ok(())
    }

    pub(crate) fn exec_bitset_clear_inplace(&mut self) -> Result<(), String> {
        let idx_val = self.pop()?;
        let mut bs_val = self.pop()?;
        let idx = idx_val
            .as_int()
            .ok_or_else(|| "bitset_clear expects an integer as second argument".to_string())?;
        if idx < 0 {
            self.push(bs_val);
            return Ok(());
        }

        let word_idx = (idx / 64) as usize;
        if let Some(crate::value::Object::BitSet(words)) = bs_val.as_object_mut() {
            if word_idx < words.len() {
                words[word_idx] &= !(1 << (idx % 64));
            }
        }
        self.push(bs_val);
        Ok(())
    }

    pub(crate) fn exec_buffer_append_inplace(&mut self) -> Result<(), String> {
        let s_val = self.pop()?;
        let mut buf_val = self.pop()?;

        let s = s_val
            .as_str()
            .ok_or_else(|| "buffer_append expects a string".to_string())?;

        if let Some(crate::value::Object::Buffer(buf)) = buf_val.as_object_mut() {
            buf.push_str(s);
        }
        self.push(buf_val);
        Ok(())
    }

    pub(crate) fn exec_bytebuf_set_u8_inplace(&mut self) -> Result<(), String> {
        let byte_val = self.pop()?;
        let idx_val = self.pop()?;
        let mut buf_val = self.pop()?;
        let idx = checked_bytebuf_index(idx_val, "bytebuf_set_u8")?;
        let byte = checked_byte_value(byte_val, "bytebuf_set_u8")?;

        match buf_val.as_object_mut() {
            Some(crate::value::Object::ByteBuf(bytes)) => {
                if idx >= bytes.len() {
                    return Err(format!(
                        "bytebuf_set_u8 index {} out of bounds (len {})",
                        idx,
                        bytes.len()
                    ));
                }
                bytes[idx] = byte;
            }
            _ => return Err("bytebuf_set_u8 expects a bytebuf".to_string()),
        }
        self.push(buf_val);
        Ok(())
    }

    pub(crate) fn exec_bytebuf_set_u32_le_inplace(&mut self) -> Result<(), String> {
        self.exec_bytebuf_set_i32_or_u32_le_inplace("bytebuf_set_u32_le")
    }

    pub(crate) fn exec_bytebuf_set_i32_le_inplace(&mut self) -> Result<(), String> {
        self.exec_bytebuf_set_i32_or_u32_le_inplace("bytebuf_set_i32_le")
    }

    fn exec_bytebuf_set_i32_or_u32_le_inplace(&mut self, fn_name: &str) -> Result<(), String> {
        let value_val = self.pop()?;
        let offset_val = self.pop()?;
        let mut buf_val = self.pop()?;
        let offset = checked_bytebuf_index(offset_val, fn_name)?;
        let value = value_val
            .as_int()
            .ok_or_else(|| format!("{} expects an int value", fn_name))?;

        match buf_val.as_object_mut() {
            Some(crate::value::Object::ByteBuf(bytes)) => {
                if offset + 4 > bytes.len() {
                    return Err(format!(
                        "{} offset {} out of bounds for 4-byte write (len {})",
                        fn_name,
                        offset,
                        bytes.len()
                    ));
                }
                let n = value as u32;
                bytes[offset] = (n & 0xff) as u8;
                bytes[offset + 1] = ((n >> 8) & 0xff) as u8;
                bytes[offset + 2] = ((n >> 16) & 0xff) as u8;
                bytes[offset + 3] = ((n >> 24) & 0xff) as u8;
            }
            _ => return Err(format!("{} expects a bytebuf", fn_name)),
        }
        self.push(buf_val);
        Ok(())
    }

    fn exec_vec_binary(
        &mut self,
        op_fn: fn(&mut crate::gc::GcHeap, Value, Value) -> Result<Value, String>,
    ) -> Result<(), String> {
        let rhs = self.pop()?;
        let lhs = self.pop()?;

        let is_lhs_list = lhs.as_list().is_some();
        let is_rhs_list = rhs.as_list().is_some();

        if is_lhs_list && is_rhs_list {
            let l = lhs.into_rad_list().unwrap();
            let r = rhs.into_rad_list().unwrap();
            if l.len() != r.len() {
                return Err(format!(
                    "Vectorized op: length mismatch ({} vs {})",
                    l.len(),
                    r.len()
                ));
            }
            let mut result = Vec::with_capacity(l.len());
            let ls = l.as_slice();
            let rs = r.as_slice();
            for (lv, rv) in ls.iter().zip(rs.iter()) {
                result.push(op_fn(&mut self.gc, *lv, *rv)?);
            }
            self.push_list_vec(result);
        } else if is_lhs_list {
            let l = lhs.into_rad_list().unwrap();
            let mut result = Vec::with_capacity(l.len());
            let ls = l.as_slice();
            for lv in ls {
                result.push(op_fn(&mut self.gc, *lv, rhs)?);
            }
            self.push_list_vec(result);
        } else if is_rhs_list {
            let r = rhs.into_rad_list().unwrap();
            let mut result = Vec::with_capacity(r.len());
            let rs = r.as_slice();
            for rv in rs {
                result.push(op_fn(&mut self.gc, lhs, *rv)?);
            }
            self.push_list_vec(result);
        } else {
            let v = op_fn(&mut self.gc, lhs, rhs)?;
            self.push(v);
        }
        Ok(())
    }

    fn exec_vec_cmp<F>(&mut self, cmp_fn: F) -> Result<(), String>
    where
        F: Fn(&Value, &Value) -> Result<bool, String>,
    {
        let rhs = self.pop()?;
        let lhs = self.pop()?;

        let is_lhs_list = lhs.as_list().is_some();
        let is_rhs_list = rhs.as_list().is_some();

        if is_lhs_list && is_rhs_list {
            let l = lhs.into_rad_list().unwrap();
            let r = rhs.into_rad_list().unwrap();
            if l.len() != r.len() {
                return Err(format!(
                    "Vectorized cmp: length mismatch ({} vs {})",
                    l.len(),
                    r.len()
                ));
            }
            let mut result = Vec::with_capacity(l.len());
            let ls = l.as_slice();
            let rs = r.as_slice();
            for (lv, rv) in ls.iter().zip(rs.iter()) {
                result.push(Value::from_bool(cmp_fn(lv, rv)?));
            }
            self.push_list_vec(result);
        } else if is_lhs_list {
            let l = lhs.into_rad_list().unwrap();
            let mut result = Vec::with_capacity(l.len());
            let ls = l.as_slice();
            for lv in ls {
                result.push(Value::from_bool(cmp_fn(lv, &rhs)?));
            }
            self.push_list_vec(result);
        } else if is_rhs_list {
            let r = rhs.into_rad_list().unwrap();
            let mut result = Vec::with_capacity(r.len());
            let rs = r.as_slice();
            for rv in rs {
                result.push(Value::from_bool(cmp_fn(&lhs, rv)?));
            }
            self.push_list_vec(result);
        } else {
            self.push(Value::from_bool(cmp_fn(&lhs, &rhs)?));
        }
        Ok(())
    }

    fn exec_vec_unary(
        &mut self,
        op_fn: fn(&mut crate::gc::GcHeap, Value) -> Result<Value, String>,
    ) -> Result<(), String> {
        let val = self.pop()?;
        if let Some(list) = val.into_rad_list() {
            let mut result = Vec::with_capacity(list.len());
            let slice = list.as_slice();
            for v in slice {
                result.push(op_fn(&mut self.gc, *v)?);
            }
            self.push_list_vec(result);
        } else {
            let v = op_fn(&mut self.gc, val)?;
            self.push(v);
        }
        Ok(())
    }

    fn exec_vec_not(&mut self) -> Result<(), String> {
        let val = self.pop()?;
        if let Some(list) = val.into_rad_list() {
            let mut result = Vec::with_capacity(list.len());
            for item in list.iter() {
                result.push(Value::from_bool(!item.is_truthy()));
            }
            self.push_list_vec(result);
        } else {
            self.push(Value::from_bool(!val.is_truthy()));
        }
        Ok(())
    }

    fn exec_vec_select(&mut self) -> Result<(), String> {
        let false_branch = self.pop()?;
        let true_branch = self.pop()?;
        let mask = self.pop()?;

        let mask_list = mask
            .into_rad_list()
            .ok_or("VecSelect: mask must be a list")?;

        let is_true_list = true_branch.as_list().is_some();
        let is_false_list = false_branch.as_list().is_some();

        let mut result = Vec::with_capacity(mask_list.len());
        let msk = mask_list.as_slice();

        if is_true_list && is_false_list {
            let t = true_branch.into_rad_list().unwrap();
            let f = false_branch.into_rad_list().unwrap();
            if t.len() != msk.len() || f.len() != msk.len() {
                return Err("VecSelect: length mismatch".to_string());
            }
            let ts = t.as_slice();
            let fs = f.as_slice();
            for ((&m, &tv), &fv) in msk.iter().zip(ts.iter()).zip(fs.iter()) {
                result.push(if m.is_truthy() { tv } else { fv });
            }
        } else if is_true_list {
            let t = true_branch.into_rad_list().unwrap();
            if t.len() != msk.len() {
                return Err("VecSelect: length mismatch".to_string());
            }
            let ts = t.as_slice();
            for (&m, &tv) in msk.iter().zip(ts.iter()) {
                result.push(if m.is_truthy() { tv } else { false_branch });
            }
        } else if is_false_list {
            let f = false_branch.into_rad_list().unwrap();
            if f.len() != msk.len() {
                return Err("VecSelect: length mismatch".to_string());
            }
            let fs = f.as_slice();
            for (&m, &fv) in msk.iter().zip(fs.iter()) {
                result.push(if m.is_truthy() { true_branch } else { fv });
            }
        } else {
            for m in msk.iter() {
                result.push(if m.is_truthy() {
                    true_branch
                } else {
                    false_branch
                });
            }
        }

        self.push_list_vec(result);
        Ok(())
    }

    fn exec_vec_broadcast(&mut self) -> Result<(), String> {
        let template = self.pop()?;
        let fill = self.pop()?;
        let list = template
            .into_rad_list()
            .ok_or("VecBroadcast: expected list template")?;
        let n = list.len();
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(fill);
        }
        self.push_list_vec(out);
        Ok(())
    }

    fn exec_vec_filter(&mut self) -> Result<(), String> {
        let mask = self.pop()?;
        let source = self.pop()?;

        let mask_list = mask
            .into_rad_list()
            .ok_or("VecFilter: mask must be a list")?;
        let source_list = source
            .into_rad_list()
            .ok_or("VecFilter: source must be a list")?;

        if mask_list.len() != source_list.len() {
            return Err(format!(
                "VecFilter: length mismatch (source {} vs mask {})",
                source_list.len(),
                mask_list.len()
            ));
        }

        let src = source_list.as_slice();
        let msk = mask_list.as_slice();
        let mut result = Vec::new();
        for i in 0..src.len() {
            if msk[i].is_truthy() {
                result.push(src[i]);
            }
        }
        self.push_list_vec(result);
        Ok(())
    }

    fn exec_load_column(&mut self) -> Result<(), String> {
        let comp_name_idx = self.read_u16()? as usize;
        let field_idx = self.read_byte()? as usize;
        let comp_name = helpers::constant_string(self.current_chunk(), comp_name_idx)?;
        let column = self.get_world().get_column_values(&comp_name, field_idx)?;
        let copied: Vec<Value> = column
            .into_iter()
            .map(|v| v.deep_copy(&mut self.gc))
            .collect();
        self.push_list_vec(copied);
        Ok(())
    }
}

fn checked_bytebuf_index(value: Value, fn_name: &str) -> Result<usize, String> {
    let idx = value
        .as_int()
        .ok_or_else(|| format!("{} expects an int offset/index", fn_name))?;
    if idx < 0 {
        return Err(format!("{} offset/index must be non-negative", fn_name));
    }
    usize::try_from(idx).map_err(|_| format!("{} offset/index too large", fn_name))
}

fn checked_byte_value(value: Value, fn_name: &str) -> Result<u8, String> {
    let byte = value
        .as_int()
        .ok_or_else(|| format!("{} expects an int byte value", fn_name))?;
    if !(0..=255).contains(&byte) {
        return Err(format!(
            "{} byte value {} out of range 0..255",
            fn_name, byte
        ));
    }
    Ok(byte as u8)
}

/// Regression tests for the ECS scheduling/soundness cluster (dogfood bugs
/// seq 39, 40, 74, 75). They live with the executor because the paths they
/// pin down — the mut-query writeback, the query filter dispatch, and the
/// parallel batch write/event merge — are all implemented here.
#[cfg(test)]
mod scheduling_tests {
    fn run_source(src: &str) -> Vec<String> {
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = crate::parser::Parser::new(tokens).parse();
        let compiler = crate::compiler::Compiler::new();
        let result = compiler.compile(&program).expect("program should compile");
        let mut vm = crate::vm::VM::new();
        vm.load_compile_result(result);
        vm.run(0).expect("program should run");
        vm.print_buffer.clone()
    }

    /// Like `run_source` but through the checker (as `rad file.rad`
    /// compiles), so per-fn effect sets reach the compiler's body-access
    /// analysis.
    fn run_source_checked(src: &str) -> Vec<String> {
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = crate::parser::Parser::new(tokens).parse();
        let mut checker = crate::checker::Checker::new();
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "check errors: {:?}", errors);
        let result = crate::compiler::Compiler::new()
            .with_checker_output(checker.output())
            .compile(&program)
            .expect("program should compile");
        let mut vm = crate::vm::VM::new();
        vm.load_compile_result(result);
        vm.run(0).expect("program should run");
        vm.print_buffer.clone()
    }

    #[test]
    fn query_where_filters_by_component_values_end_to_end() {
        // The readonly-predicate contract, through the full checked
        // pipeline: `get` inside the predicate reads the snapshotted world
        // and the filter returns exactly the matching entities.
        let out = run_source_checked(
            r#"
            component Hero { level: int = 0 }
            let a = spawn("a", Hero { level: 1 })
            let b = spawn("b", Hero { level: 3 })
            let c = spawn("c", Hero { level: 5 })
            let veterans = query_where(Hero, fn(id: entity) -> bool {
                let h = get(id, Hero) |> unwrap
                return h.level >= 3
            })
            print(len(veterans))
            "#,
        );
        assert_eq!(out, vec!["2"]);
    }

    #[test]
    fn query_map_maps_component_values_end_to_end() {
        // query_map's read-only mapper, through the full checked pipeline.
        let out = run_source_checked(
            r#"
            component Hero { level: int = 0 }
            let a = spawn("a", Hero { level: 1 })
            let b = spawn("b", Hero { level: 3 })
            let doubled = query_map(Hero, fn(id: entity) -> int {
                let h = get(id, Hero) |> unwrap
                return h.level * 2
            })
            let mut total = 0
            for v in doubled {
                total = total + v
            }
            print(total)
            "#,
        );
        assert_eq!(out, vec!["8"]);
    }

    #[test]
    fn query_where_read_predicate_inside_simulated_system_is_accepted() {
        // Interaction of the two purity systems: a read-only query_where
        // predicate inside a SYSTEM run by simulate() must pass both the
        // predicate contract (reads ok) and the simulation-purity analysis
        // (reads are legal in simulated systems).
        let out = run_source_checked(
            r#"
            component C { v: int = 0 }
            system Work(c: mut C) {
                let picked = query_where(C, fn(id: entity) -> bool {
                    let row = get(id, C) |> unwrap
                    return row.v >= 0
                })
                c.v = c.v + len(picked)
            }
            let e = spawn("e", C { v: 0 })
            let f = fork()
            let r = simulate(f, [system::Work], 3)
            let got = peek(r, e, C) |> unwrap
            print(got.v)
            "#,
        );
        assert_eq!(out, vec!["3"]);
    }

    /// Like `run_source` but with the `--serial-schedule` lever engaged, so
    /// scheduled systems run one at a time in topological order.
    fn run_source_serial(src: &str) -> Vec<String> {
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = crate::parser::Parser::new(tokens).parse();
        let compiler = crate::compiler::Compiler::new();
        let result = compiler.compile(&program).expect("program should compile");
        let mut vm = crate::vm::VM::new();
        vm.set_serial_schedule(true);
        vm.load_compile_result(result);
        vm.run(0).expect("program should run");
        vm.print_buffer.clone()
    }

    #[test]
    fn serial_schedule_runs_multi_system_phase_correctly() {
        // dogfood feature seq 83: --serial-schedule runs a whole phase one
        // system at a time in topological order (no parallel batch, no merge).
        // Inc writes each entity's own N; Count writes a resource — non-
        // conflicting, so the default path batches them in parallel and the
        // serial path runs them sequentially. Both must give the same, correct
        // answer; the serial run is the correctness-critical/differential mode.
        let src = r#"
            component N { v: 0 }
            resource C { k: 0 }
            system Inc(n: mut N) { n = N { v: n.v + 1 } }
            system Count(c: mut C) { c.k = c.k + 1 }
            let e = spawn("e", N { v: 0 })
            phase P [Inc, Count]
            schedule [P]
            print(f"{(get(e, N) |> unwrap).v},{res(C).k}")
        "#;
        assert_eq!(run_source_serial(src), vec!["1,1"]);
        // Serial mode is behavior-preserving for a well-formed program.
        assert_eq!(run_source_serial(src), run_source(src));
    }

    #[test]
    fn schedule_serial_keyword_runs_and_matches_parallel_result() {
        // dogfood feature seq 83 (per-call spelling): `schedule serial [...]`
        // runs the listed systems one at a time on the main VM — no flag
        // needed — and is behavior-preserving vs the parallel spelling.
        let serial_src = r#"
            component W { n: 1 }
            resource RA { a: 0 }
            resource RB { b: 0 }
            system SA(w: W, r: mut RA) { r.a = r.a + w.n }
            system SB(w: W, r: mut RB) { r.b = r.b + w.n }
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            schedule serial [SA, SB]
            print(f"{res(RA).a},{res(RB).b}")
        "#;
        let parallel_src = &serial_src.replace("schedule serial [", "schedule [");
        assert_eq!(run_source(serial_src), vec!["3,3"]);
        assert_eq!(run_source(serial_src), run_source(parallel_src));
    }

    #[test]
    fn accum_resource_folds_parallel_contributions() {
        // dogfood seq 83 IDEA 02: two `accum` systems of the same resource
        // SHARE a batch (unit-tested in parallel.rs) and the merge folds
        // per-field deltas in schedule order: 3 entities × 2 systems = 6,
        // and the float field accumulates exactly. The serial spelling is
        // the differential check — identical result, no parallel machinery.
        let src = r#"
            component W { n: 1 }
            resource Tally { hits: 0, weight: 0.0 }
            system CountA(w: W, t: accum Tally) {
                t.hits = t.hits + w.n
                t.weight = t.weight + 0.5
            }
            system CountB(w: W, t: accum Tally) { t.hits = t.hits + w.n }
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            schedule [CountA, CountB]
            print(res(Tally).hits)
            print(f"{res(Tally).weight == 1.5}")
        "#;
        assert_eq!(run_source(src), vec!["6", "true"]);
        // Determinism: same program, same answer, every run.
        assert_eq!(run_source(src), run_source(src));
        // Differential: the serial spelling computes the same totals.
        let serial = &src.replace("schedule [", "schedule serial [");
        assert_eq!(run_source(serial), vec!["6", "true"]);
    }

    #[test]
    fn serial_phase_members_run_in_separate_batches_and_stay_correct() {
        // dogfood feature seq 83: `serial phase` members never share a batch
        // (unit-tested in parallel.rs); end to end the declaration parses,
        // compiles, stamps the group, and the schedule still computes the
        // right answer.
        let out = run_source(
            r#"
            component W { n: 1 }
            resource RA { a: 0 }
            resource RB { b: 0 }
            system SA(w: W, r: mut RA) { r.a = r.a + w.n }
            system SB(w: W, r: mut RB) { r.b = r.b + w.n }
            serial phase Line [SA, SB]
            spawn(W { n: 1 })
            spawn(W { n: 1 })
            schedule [Line]
            print(f"{res(RA).a},{res(RB).b}")
        "#,
        );
        assert_eq!(out, vec!["2,2"]);
    }

    /// seq 40: `despawn(id)` inside a `mut` query loop used to die with
    /// "Cannot set component on non-existent entity N" because the
    /// end-of-iteration writeback ran unconditionally.
    #[test]
    fn despawn_inside_mut_query_loop_completes() {
        let out = run_source(
            r#"
            component Pos { y: 0.0 }
            component Vel { dy: 0.0 }
            for i in range(0, 4) {
                spawn(Pos { y: 1.0 - float(i) }, Vel { dy: -1.0 })
            }
            for (id, pos, vel) in query { mut Pos, Vel } {
                pos.y = pos.y + vel.dy
                if pos.y < 0.0 {
                    despawn(id)
                }
            }
            print(len(query { Pos }))
        "#,
        );
        assert_eq!(out, vec!["1"]);
    }

    /// seq 40 (bugs/02b): the crash fired even when the body never assigned
    /// to the `mut` binding — binding mut + despawning was enough.
    #[test]
    fn despawn_with_untouched_mut_binding_completes() {
        let out = run_source(
            r#"
            component A { n: 0 }
            for i in range(0, 4) {
                spawn(A { n: i })
            }
            for (id, a) in query { mut A } {
                if a.n % 2 == 0 {
                    despawn(id)
                }
            }
            print(len(query { A }))
        "#,
        );
        assert_eq!(out, vec!["2"]);
    }

    /// seq 75: `query { mut X } where <cond>` used to die at runtime with
    /// "QueryFilter: expected closure or function" — the mut-loop lowering
    /// pushed the filter chunk id as a bare int instead of a fn value.
    #[test]
    fn mut_query_with_where_filters_and_writes_back() {
        let out = run_source(
            r#"
            component Fsm { n: 0 }
            for i in range(0, 6) {
                spawn(Fsm { n: i })
            }
            let mut hits = 0
            for (_id, f) in query { mut Fsm } where Fsm.n > 2 {
                f.n = f.n + 10
                hits = hits + 1
            }
            print(hits)
            print(len(query { Fsm } where Fsm.n > 12))
        "#,
        );
        assert_eq!(out, vec!["3", "3"]);
    }

    /// seq 75: a filter that captures an enclosing local exercises the
    /// closure (upvalue) packaging path rather than the plain-fn path.
    #[test]
    fn mut_query_with_capturing_where_filter() {
        let out = run_source(
            r#"
            component Fsm { n: 0 }
            for i in range(0, 6) {
                spawn(Fsm { n: i })
            }
            let threshold = 3
            let mut hits = 0
            for (_id, f) in query { mut Fsm } where Fsm.n >= threshold {
                f.n = f.n + 100
                hits = hits + 1
            }
            print(hits)
            print(len(query { Fsm } where Fsm.n >= 100))
        "#,
        );
        assert_eq!(out, vec!["3", "3"]);
    }

    /// seq 39: a per-entity `mut` resource accumulator in a PARALLEL batch
    /// collapsed to a single increment (each iteration recomputed from the
    /// snapshot). Both spellings must count all ten entities.
    #[test]
    fn parallel_batch_resource_accumulation_counts_every_entity() {
        let out = run_source(
            r#"
            component Tag { probe: 0 }
            resource A { n: 0 }
            resource B { n: 0 }
            system BumpA(_t: Tag, a: mut A) { a.n = a.n + 1 }
            system BumpB(_t: Tag, b: mut B) { b.n = b.n + 1 }
            for _i in range(0, 10) {
                spawn(Tag { probe: 0 })
            }
            schedule [BumpA, BumpB]
            print(res(A).n)
            print(res(B).n)
        "#,
        );
        assert_eq!(out, vec!["10", "10"]);
    }

    /// seq 45: a resource written with `update(R)` in a system body used to
    /// be invisible to parallel conflict analysis, so the pair below shared
    /// a batch and one write was silently lost (R1 = 1 instead of 101).
    #[test]
    fn update_in_system_body_conflicts_with_mut_param() {
        let out = run_source(
            r#"
            resource R1 { n: 0 }
            resource D1 { x: 0 }
            resource R2 { n: 0 }
            resource D2 { x: 0 }
            resource D3 { x: 0 }
            system MutParam(r: mut R1) { r.n = r.n + 100 }
            system ViaUpdate(_d: mut D1) { update(R1) { n = res(R1).n + 1 } }
            system UpdA(_d: mut D2) { update(R2) { n = res(R2).n + 100 } }
            system UpdB(_d: mut D3) { update(R2) { n = res(R2).n + 1 } }
            schedule [MutParam, ViaUpdate]
            print(res(R1).n)
            schedule [UpdA, UpdB]
            print(res(R2).n)
        "#,
        );
        assert_eq!(out, vec!["101", "101"]);
    }

    /// seq 45 (case 3): the `update(R)` hidden one call frame deep in a
    /// helper fn — the shape real code has. The helper's checker effects
    /// mark it as a potential ECS writer, which must serialize the caller
    /// against the conflicting `mut` param system.
    #[test]
    fn update_via_helper_fn_serializes_against_mut_param() {
        let out = run_source_checked(
            r#"
            resource R3 { n: 0 }
            resource D1 { x: 0 }
            fn bump3() { update(R3) { n = res(R3).n + 1 } }
            system MutParam3(r: mut R3) { r.n = r.n + 100 }
            system ViaFn(_d: mut D1) { bump3() }
            schedule [MutParam3, ViaFn]
            print(res(R3).n)
        "#,
        );
        assert_eq!(out, vec!["101"]);
    }

    /// A2 seq 124/143 (memory corruption): a pooled worker VM kept its
    /// CREATION-time copy of the main VM's globals whenever the program
    /// (chunks Arc) matched. Global values are main-GC heap handles, and
    /// top-level `let mut` rebinding turns the old objects into garbage the
    /// main collector frees — after which the worker's own collector traced
    /// the stale handles as roots and dereferenced freed memory (the
    /// simulate_par 0xC0000005). sync_from_shared must refresh globals on
    /// EVERY sync.
    #[test]
    fn worker_sync_refreshes_globals_from_shared_state() {
        let src = r#"
            let mut g = 1
            print(g)
        "#;
        let mut lexer = crate::lexer::Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = crate::parser::Parser::new(tokens).parse();
        let compiler = crate::compiler::Compiler::new();
        let result = compiler.compile(&program).expect("program should compile");
        let mut vm = crate::vm::VM::new();
        vm.load_compile_result(result);
        vm.run(0).expect("program should run");

        let slot = vm
            .global_names
            .iter()
            .position(|n| n == "g")
            .expect("global g should exist");
        assert_eq!(vm.globals[slot].as_int(), Some(1));

        // Worker pooled while `g` was 1...
        let mut worker = crate::vm::VM::from_shared_state(vm.shared_state());
        // ...main rebinding makes a new value current (the old one would be
        // garbage on the main heap)...
        let new_val = crate::value::Value::from_int(&mut vm.gc, 42);
        vm.globals[slot] = new_val;
        // ...and the next sync against the SAME program must adopt it.
        let shared = vm.shared_state();
        worker.sync_from_shared(&shared);
        assert_eq!(
            worker.globals[slot].as_int(),
            Some(42),
            "pooled worker kept a stale creation-time global"
        );
    }

    /// A2 seq 124/143, end-to-end shape: generations of commit() +
    /// simulate_par() + peeks off result forks, with rebound top-level
    /// globals in between. Deterministic by seeding, so two runs must agree
    /// (and not die with an access violation, as this shape did ~1 in 3).
    #[test]
    fn simulate_par_generations_with_peeks_are_stable() {
        const SRC: &str = r#"
            resource Bank { gold: int = 100 }
            component Body { tag: str = "", hp: int = 10 }
            system Grow(b: mut Body) { b.hp = b.hp + 1 }
            system Drift(b: mut Body) after Grow {
                let r = rand_int(-2, 2)
                b.hp = b.hp + r
            }
            system Earn(b: Body, k: mut Bank) after Drift { k.gold = k.gold + b.hp }

            let mut ents = []
            for i in range(3) {
                ents = push(ents, spawn(f"e{i}", Body { tag: f"body-{i}", hp: 10 + i }))
            }
            let mut beam = [fork()]
            let mut acc = 0
            for gen in range(2) {
                let mut next = []
                for cand in range(3) {
                    commit(beam[0])
                    let outs = simulate_par(fork(), [system::Grow, system::Drift, system::Earn], 3, 4, 77 + gen * 31 + cand)
                    for f in outs {
                        let k = peek_resource(f, Bank) |> unwrap
                        acc = acc + k.gold
                        for e in ents {
                            let b = peek(f, e, Body) |> unwrap
                            acc = acc + b.hp + len(b.tag)
                        }
                    }
                    next = push(next, outs[0])
                }
                beam = [next[0]]
            }
            print(acc)
        "#;
        let first = run_source(SRC);
        let second = run_source(SRC);
        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert!(first[0].parse::<i64>().is_ok(), "acc should be an int");
    }

    /// seq 74: the same parallel schedule must deliver parallel-emitted
    /// events in the same order on every run (spec §7.2: writes merge in
    /// schedule order; events sort by trace id, then event name). Pooled
    /// worker VMs used to carry their trace-id counter from whatever task
    /// last ran on the same rayon thread, so the sort order — and therefore
    /// handler outcomes — changed between runs.
    #[test]
    fn parallel_emitted_events_are_ordered_deterministically() {
        const SRC: &str = r#"
            component T { k: 0 }
            resource Log { s: "" }
            resource GA { n: 0 }
            resource GB { n: 0 }
            event EvA { k: int }
            event EvB { k: int }
            system SysA(t: T, _g: mut GA) { emit EvA { k: t.k } }
            system SysB(t: T, _g: mut GB) { emit EvB { k: t.k } }
            on EvA(e) { update(Log) { s = res(Log).s + "A" + str(e.k) } }
            on EvB(e) { update(Log) { s = res(Log).s + "B" + str(e.k) } }
            for i in range(0, 5) {
                spawn(T { k: i })
            }
            for _t in range(0, 20) {
                schedule [SysA, SysB]
                flush_events()
            }
            print(res(Log).s)
        "#;
        // Per tick: five entities, ids ascending; per emission index the
        // sort places EvA before EvB ("EvA" < "EvB").
        let per_tick = "A0B0A1B1A2B2A3B3A4B4";
        let want = vec![per_tick.repeat(20)];
        let first = run_source(SRC);
        let second = run_source(SRC);
        assert_eq!(first, want);
        assert_eq!(second, want);
    }
}
