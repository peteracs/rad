#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::ast::Decl;
use crate::checker::{Checker, CheckerOptions};
use crate::compiler::{Compiler, StateTransitionInfo};
use crate::gc::GcHeap;
use crate::lexer::Lexer;
use crate::opcode::{Chunk, Op};
use crate::parser::Parser;
use crate::value::{ComponentData, Value};
use crate::vm::VM;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct RadRuntime {
    vm: VM,
    output: Vec<String>,
    render_buffer: Vec<f32>,
    /// Streaming-session state (D4). `session_base` is the snapshot the next
    /// `session_delta()` diffs against — held as a bare `Arc`, NOT a gc
    /// `Value`: the collector cannot see RadRuntime fields as roots, and a
    /// swept fork here would be a use-after-free with extra steps.
    session_base: Option<std::sync::Arc<crate::world::WorldSnapshot>>,
    /// How much of `vm.print_buffer` earlier pumps already returned.
    session_cursor: usize,
    /// RADGUI undo ring: one CoW fork per user interaction, capped. Undo is
    /// `commit(pop())` — no app participates, the world itself is the
    /// undo record. Bare Arcs for the same GC-root reason as session_base.
    undo_stack: Vec<std::sync::Arc<crate::world::WorldSnapshot>>,
    /// Worlds undone FROM — Ctrl+Shift+Z walks back up. Any new
    /// checkpoint (a fresh user action) invalidates the redo branch,
    /// standard undo-tree-pruned-to-a-line semantics.
    redo_stack: Vec<std::sync::Arc<crate::world::WorldSnapshot>>,
    /// What the renderer last saw — `session_render_delta()` diffs against
    /// this, so unchanged widgets cost zero serialization.
    render_base: Option<std::sync::Arc<crate::world::WorldSnapshot>>,
}

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
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "session": 2,
            "causal_laws": 1,
            "features": [
                "streaming-session",
                "render-delta",
                "render-buffer-v1",
                "session-state",
                "undo-redo",
                "inspect-why",
                "preview-fork",
                "timeline-trace"
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
        let cid = self.vm.load_chunk_with_gc(chunk.inner, chunk.gc);

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
    }

    fn json_to_value(&mut self, jv: &serde_json::Value) -> Result<Value, String> {
        match jv {
            serde_json::Value::Null => Ok(Value::NIL),
            serde_json::Value::Bool(b) => Ok(Value::from_bool(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::from_int(self.vm.gc_mut(), i))
                } else {
                    Ok(Value::from_float(n.as_f64().unwrap_or(0.0)))
                }
            }
            serde_json::Value::String(s) => Ok(Value::from_string(self.vm.gc_mut(), s.clone())),
            serde_json::Value::Object(o) if o.len() == 1 && o.contains_key("entity") => {
                let name = o["entity"]
                    .as_str()
                    .ok_or("session_emit: {\"entity\": ...} must name an entity")?;
                let eid = self
                    .vm
                    .world
                    .get_entity_by_name(name)
                    .ok_or_else(|| format!("session_emit: no entity named '{}'", name))?;
                Ok(Value::from_entity_id(self.vm.gc_mut(), eid))
            }
            serde_json::Value::Array(items) => {
                let mut vals = Vec::with_capacity(items.len());
                for item in items {
                    vals.push(self.json_to_value(item)?);
                }
                Ok(Value::list(self.vm.gc_mut(), vals))
            }
            serde_json::Value::Object(_) => Err(
                "session_emit: nested objects are not supported in event fields \
                     (use {\"entity\": \"name\"} for entity references)"
                    .to_string(),
            ),
        }
    }

    fn compile_and_run_seeded(&mut self, source: &str, seed: u64) -> Result<String, String> {
        self.vm = VM::new();
        self.vm.set_random_seed(seed);
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
            return Err("Module imports are not supported in browser sessions yet".to_string());
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
}

impl Default for RadRuntime {
    fn default() -> Self {
        Self::new()
    }
}

fn component_field<'a>(
    components: &'a [ComponentData],
    component_type: &str,
    field_name: &str,
) -> Option<&'a Value> {
    let component = components.iter().find(|c| c.type_name == component_type)?;
    let index = component
        .layout
        .iter()
        .position(|name| name == field_name)?;
    component.values.get(index)
}

fn component_float_field(
    components: &[ComponentData],
    component_type: &str,
    field_name: &str,
) -> Option<f64> {
    component_field(components, component_type, field_name)?.as_float()
}

fn component_int_field(
    components: &[ComponentData],
    component_type: &str,
    field_name: &str,
) -> Option<i64> {
    component_field(components, component_type, field_name)?.as_int()
}

fn component_bool_field(
    components: &[ComponentData],
    component_type: &str,
    field_name: &str,
) -> Option<bool> {
    component_field(components, component_type, field_name)?.as_bool()
}

fn component_str_field<'a>(
    components: &'a [ComponentData],
    component_type: &str,
    field_name: &str,
) -> Option<&'a str> {
    component_field(components, component_type, field_name)?.as_str()
}

fn render_model_code(model: &str) -> f32 {
    match model {
        "clockwork_mage" => 1.0,
        _ => 0.0,
    }
}

/// Bytecode chunk plus a scratch [`GcHeap`] holding any heap constants (`from_int` / `from_string`,
/// etc.). On [`RadRuntime::load_and_run`], that heap is **merged** into the VM heap before the
/// chunk is installed, so constant pointers stay valid.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct WasmChunk {
    inner: Chunk,
    gc: GcHeap,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl WasmChunk {
    pub fn add_int(&mut self, value: i64) -> u16 {
        self.inner
            .add_constant(Value::from_int(&mut self.gc, value))
    }

    pub fn add_float(&mut self, value: f64) -> u16 {
        self.inner.add_constant(Value::from_float(value))
    }

    pub fn add_string(&mut self, value: &str) -> u16 {
        self.inner
            .add_constant(Value::from_string(&mut self.gc, value.to_string()))
    }

    pub fn add_bool(&mut self, value: bool) -> u16 {
        self.inner.add_constant(Value::from_bool(value))
    }

    pub fn add_nil(&mut self) -> u16 {
        self.inner.add_constant(Value::NIL)
    }

    pub fn write_const_int(&mut self, value: i64, line: u32) {
        self.inner
            .write_const(Value::from_int(&mut self.gc, value), line);
    }

    pub fn write_const_float(&mut self, value: f64, line: u32) {
        self.inner.write_const(Value::from_float(value), line);
    }

    pub fn write_const_str(&mut self, value: &str, line: u32) {
        self.inner
            .write_const(Value::from_string(&mut self.gc, value.to_string()), line);
    }

    pub fn write_const_bool(&mut self, value: bool, line: u32) {
        self.inner.write_const(Value::from_bool(value), line);
    }

    pub fn emit_op(&mut self, op_name: &str, line: u32) -> Result<(), String> {
        let op = parse_op_name(op_name).ok_or_else(|| format!("Unknown opcode: {}", op_name))?;
        self.inner.write_op(op, line);
        Ok(())
    }

    pub fn emit_u16(&mut self, val: u16, line: u32) {
        self.inner.write_u16(val, line);
    }

    pub fn emit_byte(&mut self, val: u8, line: u32) {
        self.inner.write(val, line);
    }
}

fn parse_op_name(name: &str) -> Option<Op> {
    match name {
        "Const" => Some(Op::Const),
        "Pop" => Some(Op::Pop),
        "Dup" => Some(Op::Dup),
        "Add" => Some(Op::Add),
        "Sub" => Some(Op::Sub),
        "Mul" => Some(Op::Mul),
        "Div" => Some(Op::Div),
        "Mod" => Some(Op::Mod),
        "BitAnd" => Some(Op::BitAnd),
        "BitOr" => Some(Op::BitOr),
        "BitXor" => Some(Op::BitXor),
        "Shl" => Some(Op::Shl),
        "Shr" => Some(Op::Shr),
        "BitNot" => Some(Op::BitNot),
        "Neg" => Some(Op::Neg),
        "Not" => Some(Op::Not),
        "Eq" => Some(Op::Eq),
        "Neq" => Some(Op::Neq),
        "Lt" => Some(Op::Lt),
        "Lte" => Some(Op::Lte),
        "Gt" => Some(Op::Gt),
        "Gte" => Some(Op::Gte),
        // Logical and/or are compiled as short-circuit jumps, not direct opcodes.
        "DefGlobal" => Some(Op::DefGlobal),
        "GetGlobal" => Some(Op::GetGlobal),
        "SetGlobal" => Some(Op::SetGlobal),
        "GetLocal" => Some(Op::GetLocal),
        "SetLocal" => Some(Op::SetLocal),
        "MoveLocal" => Some(Op::MoveLocal),
        "Jump" => Some(Op::Jump),
        "JumpIfFalse" => Some(Op::JumpIfFalse),
        "JumpBack" => Some(Op::JumpBack),
        "Call" => Some(Op::Call),
        "AsyncCall" => Some(Op::AsyncCall),
        "Await" => Some(Op::Await),
        "Yield" => Some(Op::Yield),
        "Return" => Some(Op::Return),
        "Try" => Some(Op::Try),
        "MakeList" => Some(Op::MakeList),
        "MakeComp" => Some(Op::MakeComp),
        "MakeState" => Some(Op::MakeState),
        "GetField" => Some(Op::GetField),
        "SetField" => Some(Op::SetField),
        "GetIndex" => Some(Op::GetIndex),
        "SetIndex" => Some(Op::SetIndex),
        "EcsSpawn" => Some(Op::EcsSpawn),
        "EcsGet" => Some(Op::EcsGet),
        "EcsSet" => Some(Op::EcsSet),
        "EcsHas" => Some(Op::EcsHas),
        "EcsQuery" => Some(Op::EcsQuery),
        "Transition" => Some(Op::Transition),
        "MakeVariant" => Some(Op::MakeVariant),
        "Emit" => Some(Op::Emit),
        "RunSystem" => Some(Op::RunSystem),
        "RunSchedule" => Some(Op::RunSchedule),
        "RunScheduleSerial" => Some(Op::RunScheduleSerial),
        "MatchState" => Some(Op::MatchState),
        "IsVariant" => Some(Op::IsVariant),
        "Print" => Some(Op::Print),
        "Len" => Some(Op::Len),
        "TypeOf" => Some(Op::TypeOf),
        "Closure" => Some(Op::Closure),
        "GetUpvalue" => Some(Op::GetUpvalue),
        "SetUpvalue" => Some(Op::SetUpvalue),
        "MakeMap" => Some(Op::MakeMap),
        "GetFieldSlot" => Some(Op::GetFieldSlot),
        "SetFieldSlot" => Some(Op::SetFieldSlot),
        "MakeCompSlot" => Some(Op::MakeCompSlot),
        "LogicalLoad" => Some(Op::LogicalLoad),
        "LogicalStore" => Some(Op::LogicalStore),
        "MaterializeAoS" => Some(Op::MaterializeAoS),
        "QueryFilter" => Some(Op::QueryFilter),
        "Snapshot" => Some(Op::Snapshot),
        "Rollback" => Some(Op::Rollback),
        "VecBroadcast" => Some(Op::VecBroadcast),
        "ConcatN" => Some(Op::ConcatN),
        "Halt" => Some(Op::Halt),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_basic_execution() {
        let mut rt = RadRuntime::new();
        let mut chunk = rt.create_chunk("test");
        chunk.write_const_str("Hello WASM", 1);
        chunk.emit_op("Print", 1).unwrap();
        chunk.emit_byte(1, 1);
        chunk.emit_op("Halt", 2).unwrap();

        let result = rt.load_and_run(chunk).unwrap();
        assert_eq!(result, "Hello WASM");
    }

    #[test]
    fn add_state_transition_with_guard_sets_guard_chunk() {
        let mut rt = RadRuntime::new();
        rt.add_state_transition_with_guard("Door", "Closed", "open", "Open", Some(7));
        let transitions = rt
            .vm
            .state_machines
            .get("Door")
            .and_then(|m| m.get("Closed"))
            .expect("transition list should exist");
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].guard_chunk_id, Some(7));
    }

    #[test]
    fn runtime_arithmetic() {
        let mut rt = RadRuntime::new();
        let mut chunk = rt.create_chunk("math");
        chunk.write_const_int(10, 1);
        chunk.write_const_int(20, 1);
        chunk.emit_op("Add", 1).unwrap();
        chunk.emit_op("Print", 1).unwrap();
        chunk.emit_byte(1, 1);
        chunk.emit_op("Halt", 2).unwrap();

        let result = rt.load_and_run(chunk).unwrap();
        assert_eq!(result, "30");
    }

    #[test]
    fn compile_and_run_source() {
        let mut rt = RadRuntime::new();
        let result = rt.compile_and_run("print(2 + 3)").unwrap();
        assert_eq!(result, "5");
    }

    #[test]
    fn compile_and_run_causal_settlement_when_capability_is_advertised() {
        let mut rt = RadRuntime::new();
        let source = r#"
component Health { hp: int = 100 }
intent Damage { key target: entity, amount: int }
law Hit(target: entity) { propose Damage { target: target, amount: 20 } }
resolver ResolveDamage for Damage(target, proposals) {
    next(target, Health { hp: require(target, Health).hp - 20 })
}
entity hero { Health {} }
settle { Hit(hero) }
print(require(hero, Health).hp)
"#;
        assert_eq!(rt.compile_and_run(source).unwrap(), "80");
        let features: serde_json::Value =
            serde_json::from_str(&rt.runtime_features()).expect("feature JSON");
        assert_eq!(features["causal_laws"], 1);
    }

    #[test]
    fn compile_and_run_autocalls_main() {
        let mut rt = RadRuntime::new();
        let source = r#"
fn main() -> nil {
    print("hello wasm")
}
"#;
        let result = rt.compile_and_run(source).unwrap();
        assert_eq!(result, "hello wasm");
    }

    #[test]
    fn compile_and_run_playground_types_snippet() {
        let mut rt = RadRuntime::new();
        let source = r#"
fn add(a: int, b: int) -> int {
    return a + b
}

fn main() -> nil {
    print(add(20, 22))
}
"#;
        let result = rt.compile_and_run(source).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn compile_and_run_playground_match_snippet() {
        let mut rt = RadRuntime::new();
        let source = r#"
type Packet {
    Ping { id: 0 }
    Data { id: 0 }
}

let p = Packet::Data { id: 7 }
match p {
    Ping { id } => { print("ping") print(id) }
    Data { id } => { print("data") print(id) }
}
"#;
        let result = rt.compile_and_run(source).unwrap();
        assert_eq!(result, "data\n7");
    }

    #[test]
    fn compile_and_run_with_state_machine() {
        let mut rt = RadRuntime::new();
        let source = r#"
state Door {
    Closed { on open -> Open }
    Open { on close -> Closed }
}

let d = Door::Closed
let r = transition(d, "open") |> unwrap
print(r)
"#;
        let result = rt.compile_and_run(source).unwrap();
        assert_eq!(result, "Door::Open");
    }

    #[test]
    fn compile_and_run_syntax_error() {
        let mut rt = RadRuntime::new();
        let result = rt.compile_and_run("let = bad");
        assert!(result.is_err());
    }

    #[test]
    fn compile_and_run_reports_use_not_supported() {
        let mut rt = RadRuntime::new();
        let result = rt.compile_and_run(r#"use "shared.rad""#);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("not supported in browser playground"));
    }

    #[test]
    fn check_source_ok() {
        let rt = RadRuntime::new();
        let result = rt.check_source("let x = 42\nprint(x)");
        assert!(result.is_ok());
    }

    #[test]
    fn check_source_error() {
        let rt = RadRuntime::new();
        let result = rt.check_source("let x = 42\nx = 10");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }

    #[test]
    fn check_source_reports_use_not_supported() {
        let rt = RadRuntime::new();
        let result = rt.check_source(r#"use "shared.rad""#);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("not supported in browser playground"));
    }

    #[test]
    fn compile_only_counts_chunks_systems_handlers() {
        let rt = RadRuntime::new();
        let src = r#"
component Pos { x: int }

fn main() -> nil {
    print(1)
}

system step(p: mut Pos) {
}

event Tap { }

on Tap(_t) {
    print("tap")
}
"#;
        let msg = rt.compile_only(src).unwrap();
        assert!(
            msg.contains("chunks") && msg.contains("systems") && msg.contains("handlers"),
            "got: {msg}"
        );
    }

    #[test]
    fn get_world_snapshot_json_like() {
        let rt = RadRuntime::new();
        // resources ride along since RADGUI (renderers read UiConfig etc.)
        assert_eq!(
            rt.get_world_snapshot(),
            "{\"entities\":[],\"resources\":{}}"
        );
    }

    #[test]
    fn emit_op_accepts_query_snapshot_rollback() {
        let rt = RadRuntime::new();
        let mut chunk = rt.create_chunk("snap_ops");
        chunk.emit_op("QueryFilter", 1).unwrap();
        chunk.emit_op("Snapshot", 1).unwrap();
        chunk.emit_op("Rollback", 1).unwrap();
    }

    // -- D4: streaming sessions ---------------------------------------------

    const COLLAB_SRC: &str = r#"
component Note { text: "", by: "" }
resource Board { count: 0 }
event AddNote { text: str, by: str }
event ClearNotes { by: str }

on AddNote(e) {
    let b = get_resource(Board) |> unwrap
    let n = b.count + 1
    let _ = spawn(f"note-{n}", Note { text: e.text, by: e.by })
    set_resource(Board, Board { count: n })
    print(f"{e.by} added note {n}")
}

on ClearNotes(e) {
    for ent in entities(Note) { despawn(ent) }
    print(f"{e.by} cleared the board")
}
"#;

    /// One session, many frames, zero recompiles: emit -> pump mutates the
    /// world through declared handlers, and output is per-frame.
    #[test]
    fn session_emits_pump_frames_without_recompiling() {
        let mut rt = RadRuntime::new();
        rt.session_start(COLLAB_SRC).unwrap();

        rt.session_emit("AddNote", r#"{"text": "ship D4", "by": "alice"}"#)
            .unwrap();
        let out1 = rt.session_pump().unwrap();
        assert_eq!(out1, "alice added note 1");

        rt.session_emit("AddNote", r#"{"text": "λ unicode", "by": "bob"}"#)
            .unwrap();
        rt.session_emit("AddNote", r#"{"text": "third", "by": "bob"}"#)
            .unwrap();
        let out2 = rt.session_pump().unwrap();
        assert_eq!(out2, "bob added note 2\nbob added note 3");

        let snap = rt.get_world_snapshot();
        assert!(
            snap.contains("ship D4") && snap.contains("λ unicode"),
            "got: {snap}"
        );
    }

    /// Bad pushes are session-fatal for the *input*, not the session:
    /// unknown events and missing fields Err, then the session keeps going.
    #[test]
    fn session_emit_validates_and_survives() {
        let mut rt = RadRuntime::new();
        rt.session_start(COLLAB_SRC).unwrap();
        assert!(rt.session_emit("NoSuchEvent", "{}").is_err());
        assert!(rt.session_emit("AddNote", r#"{"text": "no by"}"#).is_err());
        assert!(rt.session_emit("AddNote", "not json").is_err());
        rt.session_emit("AddNote", r#"{"text": "still alive", "by": "carol"}"#)
            .unwrap();
        assert_eq!(rt.session_pump().unwrap(), "carol added note 1");
    }

    /// The D4 PASS shape, natively provable: three tabs (one authority, two
    /// replicas) start the same source; the authority's edits stream as one
    /// fork_delta per flush; replicas apply in order; world_digest agrees on
    /// ALL THREE after every flush. Replicas never recompile and never run
    /// the handlers — they converge on state alone.
    #[test]
    fn session_three_tabs_converge_via_deltas() {
        let mut host = RadRuntime::new();
        let mut tab2 = RadRuntime::new();
        let mut tab3 = RadRuntime::new();
        host.session_start(COLLAB_SRC).unwrap();
        tab2.session_start(COLLAB_SRC).unwrap();
        tab3.session_start(COLLAB_SRC).unwrap();
        assert_eq!(
            host.session_digest().unwrap(),
            tab2.session_digest().unwrap(),
            "deterministic start"
        );

        // Frame 1: one edit.
        host.session_emit("AddNote", r#"{"text": "from the host", "by": "alice"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d1 = host.session_delta().unwrap();
        tab2.session_apply(&d1).unwrap();
        tab3.session_apply(&d1).unwrap();
        let h = host.session_digest().unwrap();
        assert_eq!(h, tab2.session_digest().unwrap(), "tab2 after frame 1");
        assert_eq!(h, tab3.session_digest().unwrap(), "tab3 after frame 1");
        assert!(tab3.get_world_snapshot().contains("from the host"));

        // Frame 2: two edits batched in one flush -> still ONE delta.
        host.session_emit("AddNote", r#"{"text": "two", "by": "bob"}"#)
            .unwrap();
        host.session_emit("AddNote", r#"{"text": "three", "by": "bob"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d2 = host.session_delta().unwrap();
        tab2.session_apply(&d2).unwrap();
        tab3.session_apply(&d2).unwrap();
        let h = host.session_digest().unwrap();
        assert_eq!(h, tab2.session_digest().unwrap(), "tab2 after frame 2");
        assert_eq!(h, tab3.session_digest().unwrap(), "tab3 after frame 2");

        // Frame 3: a despawning edit (clear) converges too.
        host.session_emit("ClearNotes", r#"{"by": "alice"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d3 = host.session_delta().unwrap();
        tab2.session_apply(&d3).unwrap();
        tab3.session_apply(&d3).unwrap();
        let h = host.session_digest().unwrap();
        assert_eq!(h, tab2.session_digest().unwrap(), "tab2 after clear");
        assert_eq!(h, tab3.session_digest().unwrap(), "tab3 after clear");
        assert!(!tab2.get_world_snapshot().contains("from the host"));
    }

    /// Late join: a fourth tab arrives mid-session, adopts the full state,
    /// then rides the same delta stream as everyone else.
    #[test]
    fn session_late_joiner_loads_state_then_streams() {
        let mut host = RadRuntime::new();
        host.session_start(COLLAB_SRC).unwrap();
        host.session_emit("AddNote", r#"{"text": "early", "by": "a"}"#)
            .unwrap();
        host.session_pump().unwrap();
        host.session_delta().unwrap(); // broadcast nobody hears; state moves on

        // Late tab: no source run needed beyond decls, adopt state directly.
        let mut late = RadRuntime::new();
        late.session_start(COLLAB_SRC).unwrap();
        let state = host.session_state().unwrap();
        late.session_load(&state).unwrap();
        assert_eq!(
            host.session_digest().unwrap(),
            late.session_digest().unwrap(),
            "late joiner adopts the full state"
        );

        // And the stream continues for both.
        host.session_emit("AddNote", r#"{"text": "after join", "by": "b"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d = host.session_delta().unwrap();
        late.session_apply(&d).unwrap();
        assert_eq!(
            host.session_digest().unwrap(),
            late.session_digest().unwrap()
        );
        assert!(late.get_world_snapshot().contains("after join"));
    }

    /// Deltas chain and order matters: applying frame 2's delta before
    /// frame 1's is a refusal (fingerprint mismatch), not a corrupt world.
    #[test]
    fn session_apply_refuses_out_of_order_deltas() {
        let mut host = RadRuntime::new();
        let mut replica = RadRuntime::new();
        host.session_start(COLLAB_SRC).unwrap();
        replica.session_start(COLLAB_SRC).unwrap();

        host.session_emit("AddNote", r#"{"text": "one", "by": "a"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d1 = host.session_delta().unwrap();
        host.session_emit("AddNote", r#"{"text": "two", "by": "a"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d2 = host.session_delta().unwrap();

        let err = replica.session_apply(&d2).unwrap_err();
        assert!(err.contains("different base"), "got: {err}");
        // In order, both apply and the worlds agree.
        replica.session_apply(&d1).unwrap();
        replica.session_apply(&d2).unwrap();
        assert_eq!(
            host.session_digest().unwrap(),
            replica.session_digest().unwrap()
        );
    }

    /// Causality survives the host boundary: a host-pushed event answers
    /// why() with the handler chain, like any rad-emitted event.
    #[test]
    fn session_pushed_events_have_causality() {
        let mut rt = RadRuntime::new();
        rt.session_start(COLLAB_SRC).unwrap();
        rt.session_emit("AddNote", r#"{"text": "traced", "by": "dana"}"#)
            .unwrap();
        rt.session_pump().unwrap();
        let snap = rt.get_world_snapshot();
        assert!(snap.contains("traced"), "got: {snap}");
        let recorded = rt.vm.ledger.emits.iter().any(|e| e.event == "AddNote");
        assert!(recorded, "host emit must land in the causality ledger");
    }

    #[test]
    fn emit_op_rejects_legacy_pipe_and_break() {
        let rt = RadRuntime::new();
        let mut chunk = rt.create_chunk("legacy");
        let pipe_err = chunk.emit_op("Pipe", 1).unwrap_err();
        assert!(pipe_err.contains("Unknown opcode"), "got: {pipe_err}");
        let break_err = chunk.emit_op("Break", 1).unwrap_err();
        assert!(break_err.contains("Unknown opcode"), "got: {break_err}");
    }
}
