// Event emission and dispatch: immediate, deferred, synchronous, and
// phase-scoped emits, and the handler dispatch that drains them.

impl VM {
    pub(crate) fn captures_event_log(&self) -> bool {
        !self.is_worker && (self.in_simulation_fork == 0 || self.capture_isolated_event_log)
    }

    pub(crate) fn record_event_log(&mut self, entry: crate::vm::EventLogEntry) {
        self.event_log.push_back(entry);
        if self.event_log.len() > EVENT_LOG_CAP {
            let _ = self.event_log.pop_front();
        }
    }

    pub(crate) fn dispatch_event(
        &mut self,
        event_name: &str,
        event_data: Value,
    ) -> Result<(), String> {
        if let Some(handlers) = Arc::make_mut(&mut self.event_handlers).get_mut(event_name) {
            if handlers
                .iter()
                .any(|handler| handler.contracts.exactly_once && handler.fired)
            {
                return Err(format!(
                    "Exactly-once handler for '{event_name}' received a duplicate delivery"
                ));
            }
            let to_run: Vec<(usize, u16, bool, bool, crate::ast::CallableContracts)> = handlers
                .iter()
                .filter(|h| !h.once || !h.fired)
                .map(|h| {
                    (
                        h.chunk_id,
                        h.param_slot,
                        h.once,
                        h.has_guard,
                        h.contracts.clone(),
                    )
                })
                .collect();
            for (chunk_id, param_slot, is_once, has_guard, contracts) in to_run {
                if contracts.non_reentrant
                    && self.frames.iter().any(|frame| frame.chunk_id == chunk_id)
                {
                    return Err(format!(
                        "Non-reentrant handler for '{event_name}' was entered recursively"
                    ));
                }
                if let Some(phase) = &contracts.must_complete_before {
                    if self.get_world().phase_entered(phase) {
                        return Err(format!(
                            "Handler for '{event_name}' must complete before lifecycle phase '{phase}'"
                        ));
                    }
                }
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

    pub(crate) fn exec_emit_sync(&mut self) -> Result<(), String> {
        if self.is_worker {
            return Err("Synchronous signals cannot cross a parallel worker boundary".to_string());
        }
        let event_data = self.pop()?;
        let event_name = event_data.type_name().to_string();
        let trace_id = self.current_trace_id.unwrap_or_else(|| {
            let trace_id = self.next_trace_id;
            self.next_trace_id += 1;
            trace_id
        });
        let emit_id = if self.in_simulation_fork > 0 {
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
        if self.captures_event_log() {
            self.record_event_log(crate::vm::EventLogEntry {
                tick: self.causality_frame,
                event_name: event_name.clone(),
                payload: event_data,
            });
        }
        let old_trace = self.current_trace_id.replace(trace_id);
        let old_cause = std::mem::replace(
            &mut self.current_cause,
            crate::causality::Cause::Handler {
                event: event_name.clone().into(),
                emit_id,
            },
        );
        let result = self.dispatch_event(&event_name, event_data);
        self.current_cause = old_cause;
        self.current_trace_id = old_trace;
        result
    }

    pub(crate) fn exec_emit_phase(&mut self) -> Result<(), String> {
        if self.is_worker {
            return Err("Phase-gated events cannot cross a parallel worker boundary".to_string());
        }
        let phase_idx = self.read_u16()? as usize;
        let phase = helpers::constant_string(self.current_chunk(), phase_idx)?;
        let event_data = self.pop()?;
        let event_name = event_data.type_name().to_string();
        if self.get_world().phase_entered(&phase) {
            return Err(format!(
                "Cannot emit event '{event_name}' to lifecycle phase '{phase}': that phase has already completed"
            ));
        }
        let trace_id = self.current_trace_id.unwrap_or_else(|| {
            let trace_id = self.next_trace_id;
            self.next_trace_id += 1;
            trace_id
        });
        let emit_id = if self.in_simulation_fork > 0 {
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
        self.events_next
            .push((format!("$phase${phase}${event_name}"), event_data, trace_id));
        Ok(())
    }
}
