impl VM {

    fn execute_collection_opcode(&mut self, op: Op) -> Result<FrameControl, crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
        match op {
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
                    self.reject_captured_local_mutation(slot, "ListPushLocal")?;
                    let elem = self.pop()?;
                    let base = self.current_frame().stack_base;
                    let slot_val = self
                        .stack
                        .get_mut(base.saturating_add(slot))
                        .ok_or_else(|| format!("Invalid local offset {}", slot))?;

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
                    self.reject_captured_local_mutation(slot, "ListSetLocal")?;
                    let val = self.pop()?;
                    let idx_val = self.pop()?;
                    let idx = helpers::index_as_usize(&idx_val)?;
                    let base = self.current_frame().stack_base;
                    let slot_val = self
                        .stack
                        .get_mut(base.saturating_add(slot))
                        .ok_or_else(|| format!("Invalid local offset {}", slot))?;

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
                    let allocation_limit = self.mem_limit;
                    self.exec_vec_binary(|gc, lhs, rhs| {
                        helpers::binary_mul(gc, lhs, rhs, allocation_limit)
                    })?;
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
            _ => unreachable!("opcode dispatcher selected the wrong partition"),
        }
        Ok(FrameControl::Next)
    }

    fn execute_terminal_opcode(&mut self, op: Op) -> Result<FrameControl, crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
        match op {
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
                    if self.settlement.is_some() {
                        self.guard_frame_exit("Halt")?;
                        self.abort_settlement();
                        return Err(
                            "Internal VM error: Halt cannot execute while a settlement is active"
                                .to_string(),
                        );
                    }
                    self.frames.clear();
                    return Ok(FrameControl::Halt);
                }
            _ => unreachable!("opcode dispatcher selected the wrong partition"),
        }
        Ok(FrameControl::Next)
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
            let frame_id = self.allocate_frame_id();
            self.frames.push(CallFrame {
                frame_id,
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
            let frame_id = self.allocate_frame_id();
            self.frames.push(CallFrame {
                frame_id,
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
            if self.settlement.is_some() || self.observational_attempt_replay {
                return Err(
                    "Effect firewall: native/FFI calls are forbidden during causal or observational replay execution"
                        .to_string(),
                );
            }
            let slen = self.stack.len();
            self.stack.remove(slen - 1);
            let mut args = Vec::with_capacity(argc_us);
            for _ in 0..argc_us {
                args.push(self.pop()?);
            }
            args.reverse();
            let result = crate::ffi::invoke_native(native, &args, &mut self.gc)?;
            self.push(result);
        } else {
            return Err(format!("Not callable: {}", callee.type_name()));
        }
        Ok(())
    }

    pub(crate) fn exec_async_call(&mut self) -> Result<(), String> {
        if self.observational_attempt_replay {
            return Err("attempt replay: async/task execution is disabled".into());
        }
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
        self.meter_constraint_resources(
            capture_count,
            capture_count
                .saturating_mul(std::mem::size_of::<*mut crate::gc::CaptureCell>())
                .saturating_mul(2),
        )?;
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
            self.meter_constraint_resources(list.len(), list.len().saturating_mul(192))?;
            list.set(i, val)?;
            let __v = Value::from_rad_list(&mut self.gc, list);
            self.push(__v);
        } else if obj.as_map().is_some() {
            let map_key = MapKey::from_value(&idx_val)?;
            let mut new_map = obj.into_map().expect("map type already checked");
            self.meter_constraint_resources(new_map.len(), new_map.len().saturating_mul(256))?;
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
        let eid = self
            .get_world_mut()
            .spawn_entity(name_opt)
            .map_err(|error| error.to_string())?;
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

            let frame_id = self.allocate_frame_id();
            self.frames.push(CallFrame {
                frame_id,
                chunk_id: filter_chunk_id,
                ip: 0,
                stack_base,
                captures: captures_arc.clone(),
                system_writeback: None,
            });
            self.run_frames(saved_depth)
                .map_err(|error| error.to_string())?;

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
    }}
