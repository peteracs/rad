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
        } else if let Some(native) = callee.as_native_fn() {
            if self.settlement.is_some() || self.observational_attempt_replay {
                return Err(
                    "Effect firewall: native/FFI calls are forbidden during causal or observational replay execution"
                        .to_string(),
                );
            }
            crate::ffi::invoke_native(native, &args, &mut self.gc).map_err(Into::into)
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