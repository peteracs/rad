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
                                let real_eid = self
                                    .get_world_mut()
                                    .spawn_entity(name.as_deref())
                                    .map_err(|error| error.to_string())?;
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
                let frame_id = self.allocate_frame_id();
                self.frames.push(CallFrame {
                    frame_id,
                    chunk_id,
                    ip: 0,
                    stack_base,
                    captures: None,
                    system_writeback: None,
                });
                self.run_frames(saved_depth)
                    .map_err(|error| error.to_string())?;
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
            let frame_id = self.allocate_frame_id();
            self.frames.push(CallFrame {
                frame_id,
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
            self.run_frames(saved_depth)
                .map_err(|error| error.to_string())?;
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

    pub(crate) fn call_value(&mut self, callee: &Value, args: Vec<Value>) -> Result<Value, String> {
        self.call_value_detailed(callee, args)
            .map_err(|failure| failure.render_compat())
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
            self.enforce_settlement_balance(result)
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

/// Fold one `accum` contribution into the accumulator: per field,
/// `acc += contribution - base` for ints and floats.
fn fold_accum_delta(
    accumulator: &mut crate::value::ComponentData,
    base: &crate::value::ComponentData,
    contribution: &crate::value::ComponentData,
) {
    let field_count = accumulator
        .values
        .len()
        .min(base.values.len())
        .min(contribution.values.len());
    for index in 0..field_count {
        let current = &accumulator.values[index];
        let original = &base.values[index];
        let changed = &contribution.values[index];
        if let (Some(current), Some(original), Some(changed)) =
            (current.as_int(), original.as_int(), changed.as_int())
        {
            accumulator.values[index] = crate::value::Value::from_int(
                &mut crate::value::PersistentStore,
                current.wrapping_add(changed.wrapping_sub(original)),
            );
        } else if let (Some(current), Some(original), Some(changed)) = (
            current.as_float(),
            original.as_float(),
            changed.as_float(),
        ) {
            accumulator.values[index] =
                crate::value::Value::from_float(current + (changed - original));
        }
    }
}
