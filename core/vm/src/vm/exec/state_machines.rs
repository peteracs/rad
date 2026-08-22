// State-machine opcodes: guarded transitions, variant construction, and the
// timeline snapshot/rollback pair they are checkpointed against.

impl VM {
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
        let frame_id = self.allocate_frame_id();
        self.frames.push(CallFrame {
            frame_id,
            chunk_id: guard_chunk_id,
            ip: 0,
            stack_base,
            captures: None,
            system_writeback: None,
        });
        self.run_frames(saved_depth)
            .map_err(|error| error.to_string())?;
        let value = self.pop()?;
        self.stack.truncate(stack_base);
        Ok(value.is_truthy())
    }

    pub(crate) fn exec_make_variant(&mut self) -> Result<(), String> {
        let type_idx = self.read_u16()? as usize;
        let variant_idx = self.read_u16()? as usize;
        let field_count = self.read_u16()? as usize;
        let type_name = helpers::constant_string(self.current_chunk(), type_idx)?;
        let variant = helpers::constant_string(self.current_chunk(), variant_idx)?;
        self.meter_constraint_resources(field_count, field_count.saturating_mul(192))?;
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
}
