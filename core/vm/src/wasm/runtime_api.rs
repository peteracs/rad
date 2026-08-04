

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl RadRuntime {
    fn checker() -> Checker {
        Checker::new_with_options(CheckerOptions {
            features: vec!["causal_laws".to_string()],
            ..CheckerOptions::default()
        })
    }

    fn compiler() -> Compiler {
        Compiler::new().with_features(vec!["causal_laws".to_string()])
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Self {
        Self {
            vm: VM::new(),
            output: Vec::new(),
            render_buffer: Vec::new(),
            session_base: None,
            session_cursor: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            render_base: None,
        }
    }

    /// Host contract fingerprint for browser/native embedders. Keep this
    /// tiny and stable: pages use it before wiring advanced session features.
    pub fn runtime_features(&self) -> String {
        let value_limits = self.vm.causal_value_limits();
        let constraint_limits = self.vm.constraint_limit_profile();
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "session": 2,
            "causal_laws": 1,
            "relations_frontend": 1,
            "host_values": 1,
            "causal_constraints": 1,
            "causal_value_limits": {
                "max_depth": value_limits.max_depth(),
                "max_nodes": value_limits.max_nodes(),
                "max_encoded_bytes": value_limits.max_encoded_bytes(),
                "max_collection_items": value_limits.max_collection_items()
            },
            "constraint_limits": {
                "version": constraint_limits.version(),
                "fingerprint": constraint_limits.fingerprint(),
                "fuel_per_invocation": constraint_limits.fuel_per_invocation(),
                "max_heap_bytes_per_invocation": constraint_limits.max_heap_bytes_per_invocation(),
                "max_aggregate_fuel": constraint_limits.max_aggregate_fuel(),
                "max_aggregate_heap_bytes": constraint_limits.max_aggregate_heap_bytes(),
                "max_violations_per_invocation": constraint_limits.max_violations_per_invocation(),
                "max_violations_per_settlement": constraint_limits.max_violations_per_settlement(),
                "max_serialized_outcome_bytes": constraint_limits.max_serialized_outcome_bytes()
            },
            "features": [
                "streaming-session",
                "render-delta",
                "render-buffer-v1",
                "session-state",
                "undo-redo",
                "inspect-why",
                "preview-fork",
                "timeline-trace"
                ,"candidate-constraints-v1"
            ]
        })
        .to_string()
    }

    pub fn add_state_transition(
        &mut self,
        machine: &str,
        from_state: &str,
        event: &str,
        to_state: &str,
    ) {
        self.add_state_transition_with_guard(machine, from_state, event, to_state, None);
    }

    pub fn add_state_transition_with_guard(
        &mut self,
        machine: &str,
        from_state: &str,
        event: &str,
        to_state: &str,
        guard_chunk_id: Option<u32>,
    ) {
        let sm = std::sync::Arc::make_mut(&mut self.vm.state_machines);
        let machine_map = sm.entry(machine.to_string()).or_default();
        let transitions = machine_map.entry(from_state.to_string()).or_default();
        transitions.push(StateTransitionInfo {
            event: event.to_string(),
            target: to_state.to_string(),
            guard_chunk_id: guard_chunk_id.map(|id| id as usize),
        });
    }

    pub fn create_chunk(&self, name: &str) -> WasmChunk {
        WasmChunk {
            inner: Chunk::new(name),
            gc: GcHeap::new(),
        }
    }

    pub fn load_and_run(&mut self, chunk: WasmChunk) -> Result<String, String> {
        self.output.clear();
        self.vm.print_buffer.clear();
        // SAFETY: WasmChunk owns the exact heap used for every heap-backed
        // constant in `inner`, and both are consumed together here.
        let cid = unsafe { self.vm.load_verified_chunk_with_gc(chunk.inner, chunk.gc) }
            .map_err(|error| error.to_string())?;

        match self.vm.run(cid) {
            Ok(()) => {
                self.output = self.vm.print_buffer.clone();
                Ok(self.output.join("\n"))
            }
            Err(e) => {
                self.output = self.vm.print_buffer.clone();
                Err(e)
            }
        }
    }

    pub fn get_output(&self) -> String {
        self.output.join("\n")
    }

    pub fn compile_and_run(&mut self, source: &str) -> Result<String, String> {
        self.compile_and_run_detailed(source)
            .map_err(|failure| failure.to_string())
    }

    /// Tagged JSON boundary for browser hosts that need structured
    /// settlement rejections rather than compatibility error strings.
    pub fn compile_and_run_result_json(&mut self, source: &str) -> String {
        match self.compile_and_run_detailed(source) {
            Ok(output) => serde_json::json!({
                "kind": "ok",
                "output": output,
            })
            .to_string(),
            Err(crate::constraint_types::VmFailure::SettlementRejected(rejection)) => {
                let bytes = match rejection.canonical_bytes(self.vm.constraint_limit_profile()) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        return serde_json::json!({
                            "kind": "host_fault",
                            "code": "constraint.rejection_encoding_failed",
                            "message": error.to_string(),
                        })
                        .to_string();
                    }
                };
                let mut value: serde_json::Value = match serde_json::from_slice(&bytes) {
                    Ok(value) => value,
                    Err(error) => {
                        return serde_json::json!({
                            "kind": "host_fault",
                            "code": "constraint.rejection_json_invalid",
                            "message": error.to_string(),
                        })
                        .to_string();
                    }
                };
                if let Some(object) = value.as_object_mut() {
                    object.insert(
                        "kind".into(),
                        serde_json::Value::String("settlement_rejected".into()),
                    );
                }
                value.to_string()
            }
            Err(crate::constraint_types::VmFailure::Runtime(error)) => serde_json::json!({
                "kind": "runtime_error",
                "code": error.code,
                "message": error.message,
            })
            .to_string(),
            Err(crate::constraint_types::VmFailure::Host(error)) => serde_json::json!({
                "kind": "host_fault",
                "code": error.code,
                "message": error.message,
            })
            .to_string(),
        }
    }

    fn compile_and_run_detailed(
        &mut self,
        source: &str,
    ) -> Result<String, crate::constraint_types::VmFailure> {
        self.vm = VM::new();
        self.output.clear();

        let mut lexer = Lexer::new(source);
        let (tokens, lex_errors) = lexer.tokenize();

        let mut parser = Parser::new(tokens);
        let program = parser.parse();

        let mut all_errors = Vec::new();

        for e in lex_errors {
            all_errors.push(format!(
                "[line {}:{}] Lex error: {}",
                e.line, e.col, e.message
            ));
        }

        for e in parser.errors() {
            all_errors.push(format!(
                "[line {}:{}] Parse error: {}",
                e.line, e.col, e.message
            ));
        }

        if program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Use(_)))
        {
            return Err("Module imports are not supported in browser playground yet"
                .to_string()
                .into());
        }

        let mut checker = Self::checker();
        let checker_errors = checker.check(&program);
        let checker_output = checker.output();

        for e in checker_errors {
            all_errors.push(format!(
                "[line {}:{}] Type error: {}",
                e.line, e.col, e.message
            ));
        }

        if !all_errors.is_empty() {
            return Err(all_errors.join("\n").into());
        }

        let compile_result = Self::compiler()
            .with_checker_output(checker_output)
            .compile(&program)
            .map_err(|e| {
                crate::constraint_types::VmFailure::from(format!("Compile error: {}", e.message))
            })?;
        self.vm.load_compile_result(compile_result);

        self.vm.print_buffer.clear();
        match self.vm.run_detailed(0) {
            Ok(()) => {
                self.output = self.vm.print_buffer.clone();
                Ok(self.output.join("\n"))
            }
            Err(e) => {
                self.output = self.vm.print_buffer.clone();
                Err(e)
            }
        }
    }

    pub fn get_world_snapshot(&self) -> String {
        self.vm.world.snapshot_json_like()
    }

    pub fn get_timeline(&self) -> String {
        if self.vm.event_log.is_empty() {
            return "(no events)".to_string();
        }
        self.vm
            .event_log
            .iter()
            .map(|e| {
                format!(
                    "[tick {}] {} - payload: {}",
                    e.tick, e.event_name, e.payload
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn compile_only(&self, source: &str) -> Result<String, String> {
        let mut lexer = Lexer::new(source);
        let (tokens, lex_errors) = lexer.tokenize();

        let mut parser = Parser::new(tokens);
        let program = parser.parse();

        let mut all_errors = Vec::new();

        for e in lex_errors {
            all_errors.push(format!(
                "[line {}:{}] Lex error: {}",
                e.line, e.col, e.message
            ));
        }

        for e in parser.errors() {
            all_errors.push(format!(
                "[line {}:{}] Parse error: {}",
                e.line, e.col, e.message
            ));
        }

        if program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Use(_)))
        {
            return Err("Module imports are not supported in browser playground yet".to_string());
        }

        let mut checker = Self::checker();
        let checker_errors = checker.check(&program);
        let checker_output = checker.output();

        for e in checker_errors {
            all_errors.push(format!(
                "[line {}:{}] Type error: {}",
                e.line, e.col, e.message
            ));
        }

        if !all_errors.is_empty() {
            return Err(all_errors.join("\n"));
        }

        let compile_result = Self::compiler()
            .with_checker_output(checker_output)
            .compile(&program)
            .map_err(|e| format!("Compile error: {}", e.message))?;
        let n = compile_result.chunks.len();
        let m = compile_result.systems.len();
        let k = compile_result.handlers.len();
        Ok(format!(
            "Compiled {} chunks, {} systems, {} handlers",
            n, m, k
        ))
    }

    pub fn check_source(&self, source: &str) -> Result<String, String> {
        let mut lexer = Lexer::new(source);
        let (tokens, lex_errors) = lexer.tokenize();

        let mut parser = Parser::new(tokens);
        let program = parser.parse();

        let mut all_errors = Vec::new();

        for e in lex_errors {
            all_errors.push(format!(
                "[line {}:{}] Lex error: {}",
                e.line, e.col, e.message
            ));
        }

        for e in parser.errors() {
            all_errors.push(format!(
                "[line {}:{}] Parse error: {}",
                e.line, e.col, e.message
            ));
        }

        if program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Use(_)))
        {
            return Err("Module imports are not supported in browser playground yet".to_string());
        }

        let mut checker = Self::checker();
        let checker_errors = checker.check(&program);

        for e in checker_errors {
            all_errors.push(format!(
                "[line {}:{}] Type error: {}",
                e.line, e.col, e.message
            ));
        }

        if !all_errors.is_empty() {
            return Err(all_errors.join("\n"));
        }

        Ok("OK".to_string())
    }

    pub fn reset(&mut self) {
        self.vm = VM::new();
        self.output.clear();
        self.session_base = None;
        self.session_cursor = 0;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.render_base = None;
    }

    // -----------------------------------------------------------------------
    // Streaming sessions (D4): compile ONCE, then push events, pump frames,
    // and subscribe to deltas. No compile_and_run per keystroke — a keystroke
    // is an `emit`, a frame is a `pump`, and what travels between tabs (or
    // from a server) is `fork_delta` per flush.
    //
    // Replication model: one session is the authority for its edits; it
    // pumps a frame and ships `session_delta()`. Replicas `session_apply()`
    // deltas in order — fork_apply fingerprints the base, so out-of-order
    // or wrong-lineage deltas are refused, not silently merged wrong.
    // `session_digest()` is the convergence receipt on every machine.
    // -----------------------------------------------------------------------

    /// Compile and run the program once, keeping the VM alive as a session.
    /// The RNG is seeded deterministically so every replica that starts the
    /// same source converges to the same initial state.
    pub fn session_start(&mut self, source: &str) -> Result<String, String> {
        self.session_base = None;
        self.session_cursor = 0;
        // Determinism across replicas is non-negotiable for convergence.
        let out = self.compile_and_run_seeded(source, 7)?;
        self.session_base = Some(self.current_fork()?);
        self.session_cursor = self.vm.print_buffer.len();
        Ok(out)
    }

    /// Push one event into the session: `fields_json` is an object keyed by
    /// the event's declared field names. `{"entity": "name"}` values resolve
    /// to live entity handles. The event fires on the next `session_pump()`.
    pub fn session_emit(&mut self, event: &str, fields_json: &str) -> Result<(), String> {
        let layout = self
            .vm
            .component_layouts
            .get(event)
            .cloned()
            .ok_or_else(|| format!("session_emit: unknown event '{}'", event))?;
        let parsed: serde_json::Value = serde_json::from_str(fields_json)
            .map_err(|e| format!("session_emit: invalid fields json: {}", e))?;
        let obj = parsed
            .as_object()
            .ok_or("session_emit: fields json must be an object")?;

        let mut values = Vec::with_capacity(layout.len());
        for field in layout.iter() {
            let jv = obj.get(field).ok_or_else(|| {
                format!(
                    "session_emit: event '{}' is missing field '{}'",
                    event, field
                )
            })?;
            values.push(self.json_to_value(jv)?);
        }
        let payload = Value::component(self.vm.gc_mut(), event.to_string(), layout, values);
        self.vm.enqueue_event(payload)
    }

    /// Pump one frame: flush the event queue through the declared handlers.
    /// Returns whatever the frame printed (the same frame boundary that
    /// record/replay and causality count).
    pub fn session_pump(&mut self) -> Result<String, String> {
        self.vm
            .call_builtin(crate::value::Builtin::FlushEvents, vec![])?;
        let new_output = self.vm.print_buffer[self.session_cursor..].join("\n");
        self.session_cursor = self.vm.print_buffer.len();
        Ok(new_output)
    }

    /// The divergence since the last `session_delta()` (or session start),
    /// as RADPACK'd `fork_delta` bytes — what an authority broadcasts after
    /// a pump. Advances the base: deltas chain.
    pub fn session_delta(&mut self) -> Result<String, String> {
        let base = self
            .session_base
            .clone()
            .ok_or("session_delta: no session (call session_start first)")?;
        let cur = self.current_fork()?;
        let base_val = Value::world_fork(self.vm.gc_mut(), base);
        let cur_val = Value::world_fork(self.vm.gc_mut(), cur.clone());
        let delta = self
            .vm
            .call_builtin(crate::value::Builtin::ForkDelta, vec![base_val, cur_val])?;
        let s = delta
            .as_str()
            .ok_or("session_delta: fork_delta returned a non-string")?
            .to_string();
        self.session_base = Some(cur);
        Ok(s)
    }

    /// Apply a delta broadcast by another session and commit it. The delta's
    /// base fingerprint must match this session's current state (replicas
    /// apply in order); a mismatch is an `Err`, never a wrong world.
    pub fn session_apply(&mut self, delta: &str) -> Result<(), String> {
        let cur = self.current_fork()?;
        let cur_val = Value::world_fork(self.vm.gc_mut(), cur);
        let delta_val = Value::from_string(self.vm.gc_mut(), delta.to_string());
        let applied = self
            .vm
            .call_builtin(crate::value::Builtin::ForkApply, vec![cur_val, delta_val])?;
        let st = applied
            .as_sum_type()
            .ok_or("session_apply: fork_apply returned a non-Result")?;
        if st.variant != "Ok" {
            let msg = st
                .fields
                .get("message")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(format!("session_apply: {}", msg));
        }
        let merged = st
            .fields
            .get("value")
            .copied()
            .ok_or("session_apply: Ok without value")?;
        self.vm
            .call_builtin(crate::value::Builtin::Commit, vec![merged])?;
        self.session_base = Some(self.current_fork()?);
        self.session_cursor = self.vm.print_buffer.len();
        Ok(())
    }

    /// Full session state as wire bytes (`fork_to_bytes`) — what a late
    /// joiner receives instead of a delta chain it has no base for.
    pub fn session_state(&mut self) -> Result<String, String> {
        let cur = self.current_fork()?;
        let fork_val = Value::world_fork(self.vm.gc_mut(), cur);
        let v = self
            .vm
            .call_builtin(crate::value::Builtin::ForkToBytes, vec![fork_val])?;
        Ok(v.as_str().unwrap_or_default().to_string())
    }

    /// Adopt a full state (`session_state` from another session): decode,
    /// commit, and rebase. The late-join handshake.
    pub fn session_load(&mut self, state: &str) -> Result<(), String> {
        let sv = Value::from_string(self.vm.gc_mut(), state.to_string());
        let decoded = self
            .vm
            .call_builtin(crate::value::Builtin::ForkFromBytes, vec![sv])?;
        let st = decoded
            .as_sum_type()
            .ok_or("session_load: fork_from_bytes returned a non-Result")?;
        if st.variant != "Ok" {
            let msg = st
                .fields
                .get("message")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(format!("session_load: {}", msg));
        }
        let fork = st
            .fields
            .get("value")
            .copied()
            .ok_or("session_load: Ok without value")?;
        self.vm
            .call_builtin(crate::value::Builtin::Commit, vec![fork])?;
        self.session_base = Some(self.current_fork()?);
        self.session_cursor = self.vm.print_buffer.len();
        Ok(())
    }

    /// State-only convergence receipt: equal digests = equal worlds,
    /// regardless of which machine ran the handlers and which applied deltas.
    pub fn session_digest(&mut self) -> Result<String, String> {
        let v = self
            .vm
            .call_builtin(crate::value::Builtin::WorldDigest, vec![])?;
        Ok(v.as_str().unwrap_or_default().to_string())
    }

    /// Incremental render feed: everything that changed since the last call
    /// (first call = the whole world as upserts). The renderer applies it
    /// to its widget map — no full-world JSON per keystroke.
    pub fn session_render_delta(&mut self) -> Result<String, String> {
        let cur = self.current_fork()?;
        let json = match &self.render_base {
            Some(prev) => cur.render_delta_json(prev),
            None => {
                let empty = crate::world::World::new().snapshot();
                cur.render_delta_json(&empty)
            }
        };
        self.render_base = Some(cur);
        Ok(json)
    }

    /// Zero-copy render buffer for hot browser hosts.
    ///
    /// Layout is `f32` values:
    /// `[version, stride, count, entity_id, player_id, x, y, target_x, target_y,
    /// target_active, command_id, model_code, ...]`.
    ///
    /// This intentionally exports the stable MOBA render contract rather than
    /// arbitrary component JSON. Scene/resources can still be read through the
    /// rare `session_render_delta()` path; movement frames read this buffer.
    pub fn session_render_buffer_refresh(&mut self) -> Result<(), String> {
        const VERSION: f32 = 1.0;
        const STRIDE: usize = 9;
        const HEADER: usize = 3;

        let cur = self.current_fork()?;
        let ids = cur.sorted_entity_ids();
        let mut count = 0usize;
        self.render_buffer.clear();
        self.render_buffer.resize(HEADER, 0.0);

        for eid in ids {
            let components = cur.components_of(eid);
            let Some(player_id) = component_int_field(&components, "PlayerControlled", "player_id")
            else {
                continue;
            };
            let Some(x) = component_float_field(&components, "Position", "x") else {
                continue;
            };
            let Some(y) = component_float_field(&components, "Position", "y") else {
                continue;
            };

            let target_x = component_float_field(&components, "MoveTarget", "x").unwrap_or(x);
            let target_y = component_float_field(&components, "MoveTarget", "y").unwrap_or(y);
            let target_active =
                component_bool_field(&components, "MoveTarget", "active").unwrap_or(false);
            let command_id =
                component_int_field(&components, "MoveTarget", "command_id").unwrap_or(0);
            let model = component_str_field(&components, "RenderAvatar", "model")
                .map(render_model_code)
                .unwrap_or(0.0);

            self.render_buffer.extend_from_slice(&[
                eid as f32,
                player_id as f32,
                x as f32,
                y as f32,
                target_x as f32,
                target_y as f32,
                if target_active { 1.0 } else { 0.0 },
                command_id as f32,
                model,
            ]);
            count += 1;
        }

        self.render_buffer[0] = VERSION;
        self.render_buffer[1] = STRIDE as f32;
        self.render_buffer[2] = count as f32;
        Ok(())
    }

    pub fn session_render_buffer_ptr(&self) -> u32 {
        self.render_buffer.as_ptr() as u32
    }

    pub fn session_render_buffer_f32_len(&self) -> u32 {
        self.render_buffer.len() as u32
    }

    // -----------------------------------------------------------------------
    // RADGUI unfair advantages: undo, inspect-why, and speculative preview.
    // All zero-app-code — the renderer drives these against the session.
    // -----------------------------------------------------------------------

    /// Push the current world onto the undo ring (call before applying a
    /// user interaction). Capped: oldest checkpoints fall off.
    pub fn session_checkpoint(&mut self) -> Result<(), String> {
        let cur = self.current_fork()?;
        self.undo_stack.push(cur);
        if self.undo_stack.len() > 256 {
            self.undo_stack.remove(0);
        }
        // a new action prunes the redo branch
        self.redo_stack.clear();
        Ok(())
    }

    /// Restore the most recent checkpoint. Returns false when the ring is
    /// empty. The whole app state rewinds — widgets, data, everything —
    /// because the world IS the undo record. The undone-from world goes on
    /// the redo stack.
    pub fn session_undo(&mut self) -> Result<bool, String> {
        let Some(snap) = self.undo_stack.pop() else {
            return Ok(false);
        };
        let cur = self.current_fork()?;
        self.redo_stack.push(cur);
        let fork_val = Value::world_fork(self.vm.gc_mut(), snap);
        self.vm
            .call_builtin(crate::value::Builtin::Commit, vec![fork_val])?;
        self.session_cursor = self.vm.print_buffer.len();
        Ok(true)
    }

    /// Walk back up the undo line (Ctrl+Shift+Z). Returns false when
    /// there's nothing to redo.
    pub fn session_redo(&mut self) -> Result<bool, String> {
        let Some(snap) = self.redo_stack.pop() else {
            return Ok(false);
        };
        let cur = self.current_fork()?;
        self.undo_stack.push(cur);
        let fork_val = Value::world_fork(self.vm.gc_mut(), snap);
        self.vm
            .call_builtin(crate::value::Builtin::Commit, vec![fork_val])?;
        self.session_cursor = self.vm.print_buffer.len();
        Ok(true)
    }

    /// `why()` against the LIVE session — inspect mode's backend. Answers
    /// for any named entity and component type, as of now.
    pub fn session_why(&mut self, entity_name: &str, component: &str) -> Result<String, String> {
        let eid = self
            .vm
            .world
            .get_entity_by_name(entity_name)
            .ok_or_else(|| format!("session_why: no entity '{}'", entity_name))?;
        Ok(self.vm.ledger.explain_entity(eid, component, u64::MAX))
    }

    /// Speculative preview: run `event` in a FORK of the live session and
    /// return the world it would produce — then put everything back,
    /// bit-for-bit. The renderer diffs the two snapshots and paints ghosts.
    /// The live session never observes the speculation.
    pub fn session_preview(&mut self, event: &str, fields_json: &str) -> Result<String, String> {
        let saved = self.current_fork()?;
        let saved_cursor = self.vm.print_buffer.len();
        // Speculation must not advance the causality clock or write the
        // ledger — same rules as simulate()'s forks.
        self.vm.in_simulation_fork += 1;
        let result = (|| {
            self.session_emit(event, fields_json)?;
            self.vm
                .call_builtin(crate::value::Builtin::FlushEvents, vec![])?;
            Ok::<String, String>(self.vm.world.snapshot_json_like())
        })();
        self.vm.in_simulation_fork -= 1;
        // roll the universe back no matter what the speculation did
        let fork_val = Value::world_fork(self.vm.gc_mut(), saved);
        self.vm
            .call_builtin(crate::value::Builtin::Commit, vec![fork_val])?;
        self.vm.print_buffer.truncate(saved_cursor);
        self.session_cursor = self.vm.print_buffer.len();
        result
    }

    // -----------------------------------------------------------------------
    // Timeline tracing (RADSCOPE): run a TARGET program with a CoW world
    // snapshot captured at every frame boundary, then scrub, inspect, and
    // interrogate causality — the debugger's whole backend in five methods.
    // -----------------------------------------------------------------------

    /// Compile and run with timeline tracing on (deterministic seed).
    /// Returns the program's output; errors still leave the partial
    /// timeline inspectable.
    pub fn run_traced(&mut self, source: &str) -> Result<String, String> {
        self.run_traced_inner(source, None)
    }

    /// Retroactive edit: re-run `source` traced, but when the causality
    /// clock reaches `frame`, set `entity.component.field = value_json`
    /// first. Determinism does the rest — every frame from there on is the
    /// future that WOULD have happened. The killer debugger move.
    pub fn run_traced_with_patch(
        &mut self,
        source: &str,
        frame: usize,
        entity: &str,
        component: &str,
        field: &str,
        value_json: &str,
    ) -> Result<String, String> {
        self.run_traced_inner(
            source,
            Some((
                frame as u64,
                entity.to_string(),
                component.to_string(),
                field.to_string(),
                value_json.to_string(),
            )),
        )
    }

    fn run_traced_inner(
        &mut self,
        source: &str,
        patch: Option<(u64, String, String, String, String)>,
    ) -> Result<String, String> {
        self.session_base = None;
        self.session_cursor = 0;
        self.vm = VM::new();
        self.vm.set_random_seed(7);
        self.vm.trace_timeline = true;
        self.vm.trace_patch = patch;
        self.output.clear();
        let res = self.compile_into_current_vm_and_run(source);
        // close the timeline with the end-of-run world
        let final_snap = self.vm.world.snapshot();
        self.vm.timeline.push(final_snap);
        res
    }

    /// Number of captured frames (scrubber range).
    pub fn timeline_len(&self) -> usize {
        self.vm.timeline.len()
    }

    /// Frame `i`'s world as renderer-shaped JSON.
    pub fn timeline_world(&self, i: usize) -> Result<String, String> {
        self.vm
            .timeline
            .get(i)
            .map(|s| s.snapshot_json_like())
            .ok_or_else(|| format!("timeline_world: no frame {}", i))
    }

    /// The emit log as JSON: `[{"tick":frame,"event":"...","payload":"..."}]`.
    /// Sourced from the causality ledger — the same provenance `why()`
    /// walks, so the event panel and the why panel can never disagree.
    pub fn timeline_events(&self) -> String {
        let mut s = String::from("[");
        for (i, e) in self.vm.ledger.emits.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(
                &serde_json::json!({
                    "tick": e.frame,
                    "event": e.event,
                    "payload": e.payload,
                })
                .to_string(),
            );
        }
        s.push(']');
        s
    }

    /// Causal explanation of `entity.component` as of frame `frame`
    /// (exclusive) — `why()` with a time machine. The entity is resolved
    /// in that frame's snapshot, so renamed/despawned entities still
    /// answer.
    pub fn why_at(
        &self,
        frame: usize,
        entity_name: &str,
        component: &str,
    ) -> Result<String, String> {
        let snap = self
            .vm
            .timeline
            .get(frame)
            .ok_or_else(|| format!("why_at: no frame {}", frame))?;
        let eid = snap
            .entity_id_by_name(entity_name)
            .ok_or_else(|| format!("why_at: no entity '{}' in frame {}", entity_name, frame))?;
        Ok(self
            .vm
            .ledger
            .explain_entity(eid, component, frame as u64 + 1))
    }

    /// Shared compile-and-run against the CURRENT vm (does not reset) —
    /// used by traced runs which pre-configure the VM.
    fn compile_into_current_vm_and_run(&mut self, source: &str) -> Result<String, String> {
        let mut lexer = Lexer::new(source);
        let (tokens, lex_errors) = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let program = parser.parse();

        let mut all_errors = Vec::new();
        for e in lex_errors {
            all_errors.push(format!(
                "[line {}:{}] Lex error: {}",
                e.line, e.col, e.message
            ));
        }
        for e in parser.errors() {
            all_errors.push(format!(
                "[line {}:{}] Parse error: {}",
                e.line, e.col, e.message
            ));
        }
        if program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Use(_)))
        {
            return Err("Module imports are not supported in browser playground yet".to_string());
        }
        let mut checker = Self::checker();
        let checker_errors = checker.check(&program);
        let checker_output = checker.output();
        for e in checker_errors {
            all_errors.push(format!(
                "[line {}:{}] Type error: {}",
                e.line, e.col, e.message
            ));
        }
        if !all_errors.is_empty() {
            return Err(all_errors.join("\n"));
        }
        let compile_result = Self::compiler()
            .with_checker_output(checker_output)
            .compile(&program)
            .map_err(|e| format!("Compile error: {}", e.message))?;
        self.vm.load_compile_result(compile_result);
        self.vm.print_buffer.clear();
        match self.vm.run(0) {
            Ok(()) => {
                self.output = self.vm.print_buffer.clone();
                Ok(self.output.join("\n"))
            }
            Err(e) => {
                self.output = self.vm.print_buffer.clone();
                Err(e)
            }
        }
    }

    fn current_fork(&mut self) -> Result<std::sync::Arc<crate::world::WorldSnapshot>, String> {
        let v = self.vm.call_builtin(crate::value::Builtin::Fork, vec![])?;
        v.as_world_fork()
            .cloned()
            .ok_or_else(|| "internal: fork() returned a non-fork".to_string())
    }}