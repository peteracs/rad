impl VM {

    fn execute_call_and_result_opcode(&mut self, op: Op) -> Result<FrameControl, crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
        match op {
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
                    self.guard_frame_exit("Return")?;
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
                                self.guard_frame_exit("Try propagation")?;
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
                                self.guard_frame_exit("Try propagation")?;
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
                        self.meter_constraint_resources(
                            tuple.len(),
                            tuple.len().saturating_mul(std::mem::size_of::<Value>()),
                        )?;
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
            _ => unreachable!("opcode dispatcher selected the wrong partition"),
        }
        Ok(FrameControl::Next)
    }

    fn execute_value_opcode(&mut self, op: Op) -> Result<FrameControl, crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
        match op {
            Op::MakeList => {
                    let n = self.read_u16()? as usize;
                    self.meter_constraint_resources(
                        n,
                        n.saturating_mul(std::mem::size_of::<Value>())
                            .saturating_mul(2),
                    )?;
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n {
                        items.push(self.pop()?);
                    }
                    items.reverse();
                    self.push_list_vec(items);
                }
                Op::MakeTuple => {
                    let n = self.read_u16()? as usize;
                    self.meter_constraint_resources(
                        n,
                        n.saturating_mul(std::mem::size_of::<Value>())
                            .saturating_mul(2),
                    )?;
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
                    self.meter_constraint_resources(n, n.saturating_mul(192))?;
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
                    self.meter_constraint_resources(field_count, field_count.saturating_mul(192))?;
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
                        self.meter_constraint_resources(
                            c.values.len(),
                            c.values.len().saturating_mul(192),
                        )?;
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
                    let slot_val = *self
                        .stack
                        .get(base.saturating_add(slot))
                        .ok_or_else(|| format!("Invalid local offset {}", slot))?;
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
                    let slot_val = *self
                        .stack
                        .get(base.saturating_add(slot))
                        .ok_or_else(|| format!("Invalid local offset {}", slot))?;
                    let obj = if let Some(cell) = slot_val.as_cell() {
                        unsafe { (*cell).get() }
                    } else {
                        slot_val
                    };
                    let idx_raw = *self
                        .stack
                        .get(base.saturating_add(idx_slot))
                        .ok_or_else(|| format!("Invalid local offset {}", idx_slot))?;
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
                    self.meter_constraint_resources(total, total.saturating_mul(2))?;
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
            _ => unreachable!("opcode dispatcher selected the wrong partition"),
        }
        Ok(FrameControl::Next)
    }

    fn execute_world_opcode(&mut self, op: Op) -> Result<FrameControl, crate::constraint_types::VmFailure> {
        #[allow(non_snake_case)]
        fn Err<T, E: Into<crate::constraint_types::VmFailure>>(
            error: E,
        ) -> Result<T, crate::constraint_types::VmFailure> {
            Result::Err(error.into())
        }
        match op {
            Op::MakeState => {
                    let machine_idx = self.read_u16()? as usize;
                    let state_idx = self.read_u16()? as usize;
                    let machine = helpers::constant_string(self.current_chunk(), machine_idx)?;
                    let state = helpers::constant_string(self.current_chunk(), state_idx)?;
                    self.meter_constraint_resources(
                        1,
                        machine
                            .len()
                            .saturating_add(state.len())
                            .saturating_add(128),
                    )?;
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
                Op::ReadBaseComponent | Op::ReadCandidateComponent => {
                    let candidate = op == Op::ReadCandidateComponent;
                    let component_idx = self.read_u16()? as usize;
                    let component = helpers::constant_string(self.current_chunk(), component_idx)?;
                    let entity = self.pop()?;
                    let value = self.read_constraint_component(entity, &component, candidate)?;
                    self.push(value);
                }
                Op::RequireConstraint => {
                    let code_idx = self.read_u16()? as usize;
                    let code = helpers::constant_string(self.current_chunk(), code_idx)?;
                    let condition = self.pop()?;
                    let frame = self.current_frame();
                    let line = self
                        .chunks
                        .get(frame.chunk_id)
                        .and_then(|chunk| chunk.lines.get(frame.ip.saturating_sub(3)).copied())
                        .unwrap_or(0);
                    self.require_constraint(condition, code, line)?;
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
                        self.meter_constraint_resources(
                            c.values.len(),
                            c.values.len().saturating_mul(192),
                        )?;
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
                    self.meter_constraint_resources(
                        field_count,
                        field_count
                            .saturating_mul(std::mem::size_of::<Value>())
                            .saturating_mul(2),
                    )?;
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
            _ => unreachable!("opcode dispatcher selected the wrong partition"),
        }
        Ok(FrameControl::Next)
    }}