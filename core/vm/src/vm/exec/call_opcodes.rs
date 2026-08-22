// Invocation opcodes: direct and dynamic calls, closure construction, and
// the async spawn/await pair with its recorded IO payloads.

impl VM {

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
        } else if let Some(native) = callee.as_native_fn().cloned() {
            let slen = self.stack.len();
            self.stack.remove(slen - 1);
            let mut args = Vec::with_capacity(argc_us);
            for _ in 0..argc_us {
                args.push(self.pop()?);
            }
            args.reverse();
            let result = self.call_native(&native, args)?;
            self.push(result);
        } else if let Some(native_type) = callee.as_native_type().cloned() {
            let slen = self.stack.len();
            self.stack.remove(slen - 1);
            let mut args = Vec::with_capacity(argc_us);
            for _ in 0..argc_us {
                args.push(self.pop()?);
            }
            args.reverse();
            if args.len() != 1 {
                return Err(format!(
                    "{}() expects exactly one explicit conversion argument, got {}",
                    native_type.name,
                    args.len()
                ));
            }
            let result = cast_native_value(&mut self.gc, &native_type, &args[0])?;
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
}
