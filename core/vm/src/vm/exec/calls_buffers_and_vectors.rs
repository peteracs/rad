impl VM {

    fn call_value_inner_detailed(
        &mut self,
        callee: &Value,
        args: Vec<Value>,
    ) -> Result<Value, crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
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
            let frame_id = self.allocate_frame_id();
            self.frames.push(CallFrame {
                frame_id,
                chunk_id: fv.chunk_id,
                ip: 0,
                stack_base,
                captures: None,
                system_writeback: None,
            });
            self.run_frames(saved_depth)?;
            self.pop().map_err(Into::into)
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
            let frame_id = self.allocate_frame_id();
            self.frames.push(CallFrame {
                frame_id,
                chunk_id: cv.chunk_id,
                ip: 0,
                stack_base,
                captures: Some(std::sync::Arc::new(cv.captures.clone())),
                system_writeback: None,
            });
            self.run_frames(saved_depth)?;
            self.pop().map_err(Into::into)
        } else if let Some(builtin) = callee.as_builtin() {
            self.call_builtin(builtin, args).map_err(Into::into)
        } else if let Some(native) = callee.as_native_fn().cloned() {
            self.call_native(&native, args).map_err(Into::into)
        } else if let Some(native_type) = callee.as_native_type().cloned() {
            if args.len() != 1 {
                return Err(format!(
                    "{}() expects exactly one explicit conversion argument, got {}",
                    native_type.name,
                    args.len()
                ));
            }
            cast_native_value(&mut self.gc, &native_type, &args[0]).map_err(Into::into)
        } else {
            Err(format!("Not callable: {}", callee.type_name()))
        }
    }

    /// One deterministic boundary for every native extension call. Replay
    /// never executes the library; recording captures both values and errors.
    pub(crate) fn call_native(
        &mut self,
        native: &crate::value::NativeFnInfo,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        if args.len() != native.arity as usize {
            return Err(format!(
                "native function {}() expects {} arguments, got {}",
                native.name,
                native.arity,
                args.len()
            ));
        }
        if self.settlement.is_some()
            || self.transaction.is_some()
            || self.observational_attempt_replay
        {
            return Err(
                "Effect firewall: native calls are forbidden during transactions, settlements, and observational replay"
                    .to_string(),
            );
        }
        if self.in_simulation_fork > 0
            && (native.effects.performs_io()
                || native.effects.is_async()
                || !native.effects.writes().is_empty()
                || !native.effects.emits().is_empty())
        {
            return Err(format!(
                "simulation effect firewall: native function {}() declares effects [{}]",
                native.name,
                native
                    .effects
                    .canonical_strings_for_runtime()
                    .join(", ")
            ));
        }

        let boundary = format!(
            "native:{}:{}",
            native.extension.digest(),
            native.name
        );
        let digest = crate::replay::args_digest(&args)?;
        if (self.replayer.is_some()
            || self.recorder.is_some()
            || self.nested_native_tape.is_some())
            && !native.replayable
        {
            return Err(format!(
                "native function {}() declares replayable=false and cannot cross a record/replay boundary",
                native.name
            ));
        }
        if let Some(tape) = self.nested_native_tape.as_mut() {
            if let Some(recorded) = tape.replay_result(&boundary, &digest)? {
                let result = match recorded {
                    Ok(value) => crate::replay::decode_value(&mut self.gc, &value),
                    Err(error) => Err(error),
                };
                if let Ok(value) = &result {
                    self.remember_native_cause(native, &digest, value);
                }
                return result;
            }
        }
        if let Some(replayer) = self.replayer.as_mut() {
            let record = replayer.next_io(&boundary, &digest)?;
            let result = match record.result {
                Ok(value) => crate::replay::decode_value(&mut self.gc, &value),
                Err(error) => Err(error),
            };
            if let Ok(value) = &result {
                self.remember_native_cause(native, &digest, value);
            }
            return result;
        }

        let result = crate::allocation_meter::host_boundary(|| {
            crate::ffi::invoke_native(native, &args, &mut self.gc)
        });
        if self.recorder.is_some() || self.nested_native_tape.is_some() {
            let encoded = match &result {
                Ok(value) => crate::replay::encode_value(value).map_err(|error| {
                    format!(
                        "--record: cannot encode result of native {}(): {}",
                        native.name, error
                    )
                })?,
                Err(error) => {
                    if let Some(tape) = self.nested_native_tape.as_mut() {
                        tape.record_result(
                            boundary.clone(),
                            digest.clone(),
                            Err(error.clone()),
                        );
                    }
                    if let Some(recorder) = self.recorder.as_mut() {
                        recorder.record_io(&boundary, digest, &Err(error.clone()));
                    }
                    return result;
                }
            };
            if let Some(tape) = self.nested_native_tape.as_mut() {
                tape.record_result(boundary.clone(), digest.clone(), Ok(encoded.clone()));
            }
            if let Some(recorder) = self.recorder.as_mut() {
                recorder.record_io(&boundary, digest.clone(), &Ok(encoded));
            }
        }
        if let Ok(value) = &result {
            self.remember_native_cause(native, &digest, value);
        }
        result
    }

    fn remember_native_cause(
        &mut self,
        native: &crate::value::NativeFnInfo,
        input_digest: &str,
        value: &Value,
    ) {
        let output_digest = crate::replay::args_digest(std::slice::from_ref(value))
            .unwrap_or_else(|_| "unrecordable".to_string());
        let parent = self
            .pending_host_cause
            .take()
            .unwrap_or_else(|| self.current_cause.clone());
        self.pending_host_cause = Some(crate::causality::Cause::HostCall {
            extension: native.extension.extension_id().into(),
            generation: native.extension.extension_version().unwrap_or_default().into(),
            plugin_digest: native.extension.content_digest().into(),
            export: native.name.as_str().into(),
            input_digest: input_digest.into(),
            output_digest: output_digest.into(),
            parent: Box::new(parent),
        });
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

    pub(crate) fn exec_bytebuf_set_u16_le_inplace(&mut self) -> Result<(), String> {
        const FN_NAME: &str = "bytebuf_set_u16_le";
        let value_val = self.pop()?;
        let offset_val = self.pop()?;
        let mut buf_val = self.pop()?;
        let offset = checked_bytebuf_index(offset_val, FN_NAME)?;
        let value = value_val
            .as_int()
            .and_then(|value| u16::try_from(value).ok())
            .ok_or_else(|| format!("{} expects an int value in 0..65535", FN_NAME))?;

        match buf_val.as_object_mut() {
            Some(crate::value::Object::ByteBuf(bytes)) => {
                let len = bytes.len();
                let target = bytes
                    .get_mut(offset..offset.saturating_add(2))
                    .ok_or_else(|| {
                        format!(
                            "{} offset {} out of bounds for 2-byte write (len {})",
                            FN_NAME, offset, len
                        )
                    })?;
                target.copy_from_slice(&value.to_le_bytes());
            }
            _ => return Err(format!("{} expects a bytebuf", FN_NAME)),
        }
        self.push(buf_val);
        Ok(())
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

    fn exec_vec_binary<F>(&mut self, mut op_fn: F) -> Result<(), String>
    where
        F: FnMut(&mut crate::gc::GcHeap, Value, Value) -> Result<Value, String>,
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
                    "Vectorized op: length mismatch ({} vs {})",
                    l.len(),
                    r.len()
                ));
            }
            self.meter_constraint_resources(l.len(), l.len().saturating_mul(192))?;
            let mut result = Vec::with_capacity(l.len());
            let ls = l.as_slice();
            let rs = r.as_slice();
            for (lv, rv) in ls.iter().zip(rs.iter()) {
                result.push(op_fn(&mut self.gc, *lv, *rv)?);
            }
            self.push_list_vec(result);
        } else if is_lhs_list {
            let l = lhs.into_rad_list().unwrap();
            self.meter_constraint_resources(l.len(), l.len().saturating_mul(192))?;
            let mut result = Vec::with_capacity(l.len());
            let ls = l.as_slice();
            for lv in ls {
                result.push(op_fn(&mut self.gc, *lv, rhs)?);
            }
            self.push_list_vec(result);
        } else if is_rhs_list {
            let r = rhs.into_rad_list().unwrap();
            self.meter_constraint_resources(r.len(), r.len().saturating_mul(192))?;
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
            self.meter_constraint_resources(
                l.len(),
                l.len()
                    .saturating_mul(std::mem::size_of::<Value>())
                    .saturating_mul(2),
            )?;
            let mut result = Vec::with_capacity(l.len());
            let ls = l.as_slice();
            let rs = r.as_slice();
            for (lv, rv) in ls.iter().zip(rs.iter()) {
                result.push(Value::from_bool(cmp_fn(lv, rv)?));
            }
            self.push_list_vec(result);
        } else if is_lhs_list {
            let l = lhs.into_rad_list().unwrap();
            self.meter_constraint_resources(
                l.len(),
                l.len()
                    .saturating_mul(std::mem::size_of::<Value>())
                    .saturating_mul(2),
            )?;
            let mut result = Vec::with_capacity(l.len());
            let ls = l.as_slice();
            for lv in ls {
                result.push(Value::from_bool(cmp_fn(lv, &rhs)?));
            }
            self.push_list_vec(result);
        } else if is_rhs_list {
            let r = rhs.into_rad_list().unwrap();
            self.meter_constraint_resources(
                r.len(),
                r.len()
                    .saturating_mul(std::mem::size_of::<Value>())
                    .saturating_mul(2),
            )?;
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
            self.meter_constraint_resources(list.len(), list.len().saturating_mul(192))?;
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
            self.meter_constraint_resources(
                list.len(),
                list.len()
                    .saturating_mul(std::mem::size_of::<Value>())
                    .saturating_mul(2),
            )?;
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

        self.meter_constraint_resources(
            mask_list.len(),
            mask_list
                .len()
                .saturating_mul(std::mem::size_of::<Value>())
                .saturating_mul(2),
        )?;
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
        self.meter_constraint_resources(
            n,
            n.saturating_mul(std::mem::size_of::<Value>())
                .saturating_mul(2),
        )?;
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
        self.meter_constraint_resources(
            source_list.len(),
            source_list
                .len()
                .saturating_mul(std::mem::size_of::<Value>())
                .saturating_mul(2),
        )?;

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
        self.meter_constraint_resources(column.len(), column.len().saturating_mul(192))?;
        let copied: Vec<Value> = column
            .into_iter()
            .map(|v| v.deep_copy(&mut self.gc))
            .collect();
        self.push_list_vec(copied);
        Ok(())
    }
}

fn cast_native_value(
    gc: &mut crate::gc::GcHeap,
    target: &crate::native_types::NativeTypeDescriptor,
    source: &Value,
) -> Result<Value, String> {
    use crate::native_types::{NativeScalarKind as Repr, NativeScalarValue, NativeTypeFlavor};

    #[derive(Clone, Copy)]
    enum Number {
        Signed(i64),
        Unsigned(u64),
        Float(f64),
    }

    let number = if let Some(native) = source.as_native_scalar() {
        if target.flavor != NativeTypeFlavor::Scalar
            && !(native.type_name == target.name
                || (native.flavor == NativeTypeFlavor::Scalar && native.repr == target.repr))
        {
            return Err(format!(
                "cannot convert {} directly to {}: unwrap to {} explicitly first",
                native.type_name, target.name, target.repr
            ));
        }
        if let Some(value) = native.float() {
            Number::Float(value)
        } else if native.repr.is_signed() {
            Number::Signed(native.signed().unwrap())
        } else {
            Number::Unsigned(native.unsigned().unwrap())
        }
    } else if let Some(value) = source.as_int() {
        if target.flavor != NativeTypeFlavor::Scalar {
            return Err(format!(
                "{}() requires an explicit {} value, not int; use {}(int_value) first",
                target.name, target.repr, target.repr
            ));
        }
        Number::Signed(value)
    } else if let Some(value) = source.as_float() {
        if target.flavor != NativeTypeFlavor::Scalar {
            return Err(format!(
                "{}() requires an explicit {} value, not float; use {}(float_value) first",
                target.name, target.repr, target.repr
            ));
        }
        Number::Float(value)
    } else {
        return Err(format!(
            "{}() cannot convert a {} value",
            target.name,
            source.type_name()
        ));
    };

    fn integral(number: Number, target: &str) -> Result<i128, String> {
        match number {
            Number::Signed(value) => Ok(value as i128),
            Number::Unsigned(value) => Ok(value as i128),
            Number::Float(value)
                if value.is_finite()
                    && value.fract() == 0.0
                    && value >= i64::MIN as f64
                    && value <= u64::MAX as f64 =>
            {
                Ok(value as i128)
            }
            Number::Float(value) => Err(format!(
                "{} conversion requires a finite integral value, got {}",
                target, value
            )),
        }
    }

    let bits = match target.repr {
        Repr::U8 => u8::try_from(integral(number, &target.name)?)
            .map(u64::from)
            .map_err(|_| format!("{} conversion overflow for u8", target.name))?,
        Repr::U16 => u16::try_from(integral(number, &target.name)?)
            .map(u64::from)
            .map_err(|_| format!("{} conversion overflow for u16", target.name))?,
        Repr::U32 => u32::try_from(integral(number, &target.name)?)
            .map(u64::from)
            .map_err(|_| format!("{} conversion overflow for u32", target.name))?,
        Repr::U64 => u64::try_from(integral(number, &target.name)?)
            .map_err(|_| format!("{} conversion overflow for u64", target.name))?,
        Repr::I8 => i8::try_from(integral(number, &target.name)?)
            .map(|value| value as u8 as u64)
            .map_err(|_| format!("{} conversion overflow for i8", target.name))?,
        Repr::I16 => i16::try_from(integral(number, &target.name)?)
            .map(|value| value as u16 as u64)
            .map_err(|_| format!("{} conversion overflow for i16", target.name))?,
        Repr::I32 => i32::try_from(integral(number, &target.name)?)
            .map(|value| value as u32 as u64)
            .map_err(|_| format!("{} conversion overflow for i32", target.name))?,
        Repr::I64 => i64::try_from(integral(number, &target.name)?)
            .map(|value| value as u64)
            .map_err(|_| format!("{} conversion overflow for i64", target.name))?,
        Repr::F32 => {
            let value = match number {
                Number::Signed(value) => value as f32,
                Number::Unsigned(value) => value as f32,
                Number::Float(value) => value as f32,
            };
            if value.is_nan() { f32::NAN.to_bits() as u64 } else { value.to_bits() as u64 }
        }
        Repr::F64 => {
            let value = match number {
                Number::Signed(value) => value as f64,
                Number::Unsigned(value) => value as f64,
                Number::Float(value) => value,
            };
            if value.is_nan() { f64::NAN.to_bits() } else { value.to_bits() }
        }
    };
    if !target.accepts_bits(bits) {
        return Err(format!(
            "0x{bits:x} is not a declared value of {}",
            target.name
        ));
    }
    Ok(Value::from_native_scalar(
        gc,
        NativeScalarValue {
            type_name: target.name.clone(),
            repr: target.repr,
            flavor: target.flavor,
            bits,
        },
    ))
}
