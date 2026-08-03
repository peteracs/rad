//! Exact pre-attempt checkpoints for observational RFC-0002 replay.
//!
//! A portable failed-attempt recipe contains no VM pointers. In-process
//! replay additionally retains one detached, graph-isolated VM captured
//! before the attempted host call begins. Replays fork that seed rather than
//! cloning the authoritative VM's later state.

use super::{EventLogEntry, HandlerEntry, MigrationEntry, TaskRecord, TaskStatus, VM};
use crate::opcode::SealedChunk;
use crate::value::Value;
use crate::world::WorldSnapshot;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionRole {
    Main,
    Worker,
    SimulationFork,
}

impl ExecutionRole {
    fn of(source: &VM) -> Self {
        if source.is_worker {
            Self::Worker
        } else if source.in_simulation_fork > 0 {
            Self::SimulationFork
        } else {
            Self::Main
        }
    }
}

/// The mutable execution context that must be identical between the original
/// attempt and its observational replay. This one inventory drives both the
/// checkpoint digest and child-VM construction; observational safety flags
/// (`suppress_output` and `observational_attempt_replay`) are deliberately
/// applied afterward because they do not change gameplay semantics.
#[derive(Clone)]
pub(crate) struct AttemptReplayState {
    execution_role: ExecutionRole,
    is_worker: bool,
    world: WorldSnapshot,
    pub(crate) root_layout: ReplayRootLayout,
    pub(crate) global_count: usize,
    pub(crate) chunks: Arc<Vec<SealedChunk>>,
    pub(crate) events_current: Vec<(String, u64)>,
    pub(crate) events_next: Vec<(String, u64)>,
    pub(crate) events_processing: Vec<(String, u64)>,
    pub(crate) delayed_events: Vec<(i64, String, u64)>,
    pub(crate) event_log: Vec<(u64, String)>,
    pub(crate) tasks: HashMap<u64, TaskRecord>,
    event_handlers: Arc<HashMap<String, Vec<HandlerEntry>>>,
    indexed_decl: Arc<HashMap<String, HashSet<String>>>,
    migrations: HashMap<String, MigrationEntry>,
    sandbox_caps: Option<Arc<crate::sandbox::SandboxCaps>>,
    sys_args: Vec<String>,
    print_buffer: Vec<String>,
    eprint_buffer: Vec<String>,
    trace_timeline: bool,
    trace_patch: Option<(u64, String, String, String, String)>,
    timeline: Vec<WorldSnapshot>,
    fuel: u64,
    mem_limit: usize,
    rng_state: u64,
    current_cause: crate::causality::Cause,
    causality_frame: u64,
    ledger: crate::causality::CausalityLedger,
    emit_ids_current: Vec<u64>,
    emit_ids_next: Vec<u64>,
    next_frame_id: u64,
    next_settlement_id: u64,
    next_task_id: u64,
    current_trace_id: Option<u64>,
    next_trace_id: u64,
    in_simulation_fork: u32,
    sandbox_input_json: Option<String>,
    sandbox_output_json: Option<String>,
    last_sandbox_output_json: Option<String>,
    last_sandbox_fuel_spent: u64,
    once_guard_passed: bool,
    serial_schedule: bool,
}

impl AttemptReplayState {
    pub(crate) fn capture(source: &VM) -> Self {
        let execution_role = ExecutionRole::of(source);
        Self {
            execution_role,
            is_worker: source.is_worker,
            world: source.world.snapshot(),
            root_layout: source.replay_root_layout(),
            global_count: source.globals.len(),
            chunks: Arc::clone(&source.chunks),
            events_current: source
                .events_current
                .iter()
                .map(|(name, _, id)| (name.clone(), *id))
                .collect(),
            events_next: source
                .events_next
                .iter()
                .map(|(name, _, id)| (name.clone(), *id))
                .collect(),
            events_processing: source
                .events_processing
                .iter()
                .map(|(name, _, id)| (name.clone(), *id))
                .collect(),
            delayed_events: source
                .delayed_events
                .iter()
                .map(|(delay, name, _, id)| (*delay, name.clone(), *id))
                .collect(),
            event_log: source
                .event_log
                .iter()
                .map(|entry| (entry.tick, entry.event_name.clone()))
                .collect(),
            tasks: source.tasks.clone(),
            event_handlers: Arc::clone(&source.event_handlers),
            indexed_decl: Arc::clone(&source.indexed_decl),
            migrations: source.migrations.clone(),
            sandbox_caps: source.sandbox_caps.clone(),
            sys_args: source.sys_args.clone(),
            print_buffer: source.print_buffer.clone(),
            eprint_buffer: source.eprint_buffer.clone(),
            trace_timeline: source.trace_timeline,
            trace_patch: source.trace_patch.clone(),
            timeline: source.timeline.clone(),
            fuel: source.fuel,
            mem_limit: source.mem_limit,
            rng_state: source.rng_state,
            current_cause: source.current_cause.clone(),
            causality_frame: source.causality_frame,
            ledger: source.ledger.clone(),
            emit_ids_current: source.emit_ids_current.clone(),
            emit_ids_next: source.emit_ids_next.clone(),
            next_frame_id: source.next_frame_id,
            next_settlement_id: source.next_settlement_id,
            next_task_id: source.next_task_id,
            current_trace_id: source.current_trace_id,
            next_trace_id: source.next_trace_id,
            in_simulation_fork: source.in_simulation_fork,
            sandbox_input_json: source.sandbox_input_json.clone(),
            sandbox_output_json: source.sandbox_output_json.clone(),
            last_sandbox_output_json: source.last_sandbox_output_json.clone(),
            last_sandbox_fuel_spent: source.last_sandbox_fuel_spent,
            once_guard_passed: source.once_guard_passed,
            serial_schedule: source.serial_schedule,
        }
    }

    pub(crate) fn apply_to(&self, replay: &mut VM) {
        replay.is_worker = self.is_worker;
        replay.world.restore(self.world.clone());
        replay.event_handlers = Arc::clone(&self.event_handlers);
        replay.indexed_decl = Arc::clone(&self.indexed_decl);
        replay.migrations = self.migrations.clone();
        replay.sandbox_caps = self.sandbox_caps.clone();
        replay.sys_args = self.sys_args.clone();
        replay.print_buffer = self.print_buffer.clone();
        replay.eprint_buffer = self.eprint_buffer.clone();
        replay.trace_timeline = self.trace_timeline;
        replay.trace_patch = self.trace_patch.clone();
        replay.timeline = self.timeline.clone();
        replay.fuel = self.fuel;
        replay.mem_limit = self.mem_limit;
        replay.rng_state = self.rng_state;
        replay.current_cause = self.current_cause.clone();
        replay.causality_frame = self.causality_frame;
        replay.ledger = self.ledger.clone();
        replay.emit_ids_current = self.emit_ids_current.clone();
        replay.emit_ids_next = self.emit_ids_next.clone();
        replay.next_frame_id = self.next_frame_id;
        replay.next_settlement_id = self.next_settlement_id;
        replay.next_task_id = self.next_task_id;
        replay.current_trace_id = self.current_trace_id;
        replay.next_trace_id = self.next_trace_id;
        replay.in_simulation_fork = self.in_simulation_fork;
        replay.sandbox_input_json = self.sandbox_input_json.clone();
        replay.sandbox_output_json = self.sandbox_output_json.clone();
        replay.last_sandbox_output_json = self.last_sandbox_output_json.clone();
        replay.last_sandbox_fuel_spent = self.last_sandbox_fuel_spent;
        replay.once_guard_passed = self.once_guard_passed;
        replay.serial_schedule = self.serial_schedule;
    }

    pub(crate) fn rebuild_event_log(
        &self,
        mut next_value: impl FnMut() -> Result<Value, String>,
    ) -> Result<Vec<EventLogEntry>, String> {
        self.event_log
            .iter()
            .map(|(tick, event_name)| {
                Ok(EventLogEntry {
                    tick: *tick,
                    event_name: event_name.clone(),
                    payload: next_value()?,
                })
            })
            .collect()
    }

    fn encode_checkpoint(&self, out: &mut crate::canonical::CanonicalWriter) {
        out.byte(match self.execution_role {
            ExecutionRole::Main => 0,
            ExecutionRole::Worker => 1,
            ExecutionRole::SimulationFork => 2,
        });
        out.u32(self.in_simulation_fork);
        out.bool(self.serial_schedule);
        out.bool(self.trace_timeline);
        out.bool(self.once_guard_passed);
        out.optional_u64(self.current_trace_id);
        out.u64(self.next_trace_id);
        out.usize(self.emit_ids_current.len());
        for emit_id in &self.emit_ids_current {
            out.u64(*emit_id);
        }
        out.usize(self.emit_ids_next.len());
        for emit_id in &self.emit_ids_next {
            out.u64(*emit_id);
        }

        let mut handler_names = self.event_handlers.keys().collect::<Vec<_>>();
        handler_names.sort();
        out.usize(handler_names.len());
        for name in handler_names {
            out.text(name);
            out.usize(self.event_handlers[name].len());
            for handler in &self.event_handlers[name] {
                out.usize(handler.chunk_id);
                out.u16(handler.param_slot);
                out.bool(handler.once);
                out.bool(handler.fired);
                out.bool(handler.has_guard);
            }
        }

        out.usize(self.events_current.len());
        for (name, id) in &self.events_current {
            out.text(name);
            out.u64(*id);
        }
        out.usize(self.events_next.len());
        for (name, id) in &self.events_next {
            out.text(name);
            out.u64(*id);
        }
        out.usize(self.events_processing.len());
        for (name, id) in &self.events_processing {
            out.text(name);
            out.u64(*id);
        }
        out.usize(self.delayed_events.len());
        for (delay, name, id) in &self.delayed_events {
            out.i64(*delay);
            out.text(name);
            out.u64(*id);
        }
        out.usize(self.event_log.len());
        for (tick, event_name) in &self.event_log {
            out.u64(*tick);
            out.text(event_name);
        }
        let mut tasks = self.tasks.iter().collect::<Vec<_>>();
        tasks.sort_by_key(|(id, _)| **id);
        out.usize(tasks.len());
        for (id, task) in tasks {
            out.u64(*id);
            out.u64(task.id);
            match &task.status {
                TaskStatus::Ready => out.byte(0),
                TaskStatus::Completed(_) => out.byte(1),
                TaskStatus::Failed(message) => {
                    out.byte(2);
                    out.text(message);
                }
            }
        }

        let mut indexed_components = self.indexed_decl.keys().collect::<Vec<_>>();
        indexed_components.sort();
        out.usize(indexed_components.len());
        for component in indexed_components {
            out.text(component);
            let mut fields = self.indexed_decl[component].iter().collect::<Vec<_>>();
            fields.sort();
            out.usize(fields.len());
            for field in fields {
                out.text(field);
            }
        }
        let mut migration_names = self.migrations.keys().collect::<Vec<_>>();
        migration_names.sort();
        out.usize(migration_names.len());
        for name in migration_names {
            let migration = &self.migrations[name];
            out.text(name);
            out.usize(migration.chunk_id);
            out.u16(migration.param_slot);
            out.bool(migration.version_slot.is_some());
            if let Some(slot) = migration.version_slot {
                out.u16(slot);
            }
        }

        out.text(&self.world.snapshot_json_like());
        out.u64(self.rng_state);
        out.u64(self.fuel);
        out.usize(self.mem_limit);
        out.u64(self.next_frame_id);
        out.u64(self.next_settlement_id);
        out.u64(self.next_task_id);
        out.u64(self.causality_frame);
        out.usize(self.sys_args.len());
        for argument in &self.sys_args {
            out.text(argument);
        }
        out.usize(self.print_buffer.len());
        for line in &self.print_buffer {
            out.text(line);
        }
        out.usize(self.eprint_buffer.len());
        for line in &self.eprint_buffer {
            out.text(line);
        }
        out.usize(self.timeline.len());
        for snapshot in &self.timeline {
            out.text(&snapshot.snapshot_json_like());
        }
        self.ledger.encode_checkpoint(out);
        crate::causality::CausalityLedger::encode_cause_checkpoint(&self.current_cause, out);
        out.bool(self.trace_patch.is_some());
        if let Some((frame, entity, component, field, value)) = &self.trace_patch {
            out.u64(*frame);
            out.text(entity);
            out.text(component);
            out.text(field);
            out.text(value);
        }
        out.optional_text(self.sandbox_input_json.as_deref());
        out.optional_text(self.sandbox_output_json.as_deref());
        out.optional_text(self.last_sandbox_output_json.as_deref());
        out.u64(self.last_sandbox_fuel_spent);

        if let Some(caps) = &self.sandbox_caps {
            out.bool(true);
            let mut readable = caps.readable_components.iter().collect::<Vec<_>>();
            let mut writable = caps.writable_components.iter().collect::<Vec<_>>();
            readable.sort();
            writable.sort();
            out.usize(readable.len());
            for component in readable {
                out.text(component);
            }
            out.usize(writable.len());
            for component in writable {
                out.text(component);
            }
            out.u64(caps.fuel);
            out.usize(caps.mem_limit);
        } else {
            out.bool(false);
        }
    }
}

#[derive(Clone)]
pub(crate) struct ReplayRootLayout {
    pub(crate) roots: Vec<Value>,
    pub(crate) chunk_constant_counts: Vec<usize>,
    pub(crate) completed_tasks: Vec<(u64, Value)>,
}

pub(crate) struct AttemptReplayCheckpoint {
    seed: VM,
    digest: String,
}

impl AttemptReplayCheckpoint {
    pub(crate) fn capture(source: &VM) -> Result<Self, String> {
        source.ensure_attempt_checkpoint_boundary()?;
        let state = AttemptReplayState::capture(source);
        let digest = source.attempt_checkpoint_digest_from(&state);
        let seed = source.detached_attempt_replay_vm_from(&state)?;
        let cloned_digest = seed.attempt_checkpoint_digest();
        if cloned_digest != digest {
            return Err(format!(
                "attempt replay checkpoint changed while detaching ({digest} != {cloned_digest})"
            ));
        }
        Ok(Self { seed, digest })
    }

    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }

    pub(crate) fn fork(&self) -> Result<VM, String> {
        let actual = self.seed.attempt_checkpoint_digest();
        if actual != self.digest {
            return Err("stored attempt replay checkpoint no longer matches its digest".into());
        }
        self.seed.detached_attempt_replay_vm()
    }
}

impl VM {
    pub(crate) fn replay_root_layout(&self) -> ReplayRootLayout {
        let mut roots = self.globals.clone();
        let chunk_constant_counts = self
            .chunks
            .iter()
            .map(|chunk| {
                roots.extend_from_slice(chunk.constants());
                chunk.constants().len()
            })
            .collect::<Vec<_>>();
        for (_, payload, _) in &self.events_current {
            roots.push(*payload);
        }
        for (_, payload, _) in &self.events_next {
            roots.push(*payload);
        }
        for (_, payload, _) in &self.events_processing {
            roots.push(*payload);
        }
        for (_, _, payload, _) in &self.delayed_events {
            roots.push(*payload);
        }
        for entry in &self.event_log {
            roots.push(entry.payload);
        }
        let mut completed_tasks = self
            .tasks
            .iter()
            .filter_map(|(id, task)| match task.status {
                TaskStatus::Completed(value) => Some((*id, value)),
                TaskStatus::Ready | TaskStatus::Failed(_) => None,
            })
            .collect::<Vec<_>>();
        completed_tasks.sort_by_key(|(id, _)| *id);
        roots.extend(completed_tasks.iter().map(|(_, value)| *value));
        ReplayRootLayout {
            roots,
            chunk_constant_counts,
            completed_tasks,
        }
    }

    fn ensure_attempt_checkpoint_boundary(&self) -> Result<(), String> {
        if self.settlement.is_some() || !self.frames.is_empty() || !self.stack.is_empty() {
            return Err("attempt recording requires a quiescent public VM boundary".into());
        }
        if self.in_async_context || !self.pending_io.is_empty() {
            return Err("attempt recording cannot checkpoint active asynchronous I/O".into());
        }
        if !self.command_buffer.is_empty() {
            return Err("attempt recording cannot checkpoint buffered ECS commands".into());
        }
        if self.recorder.is_some() || self.replayer.is_some() {
            return Err("attempt recording cannot nest inside ledger record/replay".into());
        }
        let role = ExecutionRole::of(self);
        if role != ExecutionRole::Main || self.observational_attempt_replay {
            return Err("attempt recording requires an authoritative main-timeline VM".into());
        }
        Ok(())
    }

    /// Canonical identity of all pre-attempt state that observational replay
    /// may read. Heap topology uses deterministic graph numbering, never raw
    /// allocator addresses.
    pub(crate) fn attempt_checkpoint_digest(&self) -> String {
        let state = AttemptReplayState::capture(self);
        self.attempt_checkpoint_digest_from(&state)
    }

    pub(crate) fn attempt_checkpoint_digest_from(&self, state: &AttemptReplayState) -> String {
        hex::encode(Sha256::digest(
            self.attempt_checkpoint_canonical_bytes_from(state),
        ))
    }

    /// The versioned bytes hashed by portable replay. Persisting a checkpoint
    /// uses this same representation; no diagnostic formatting participates.
    pub(crate) fn attempt_checkpoint_canonical_bytes_from(
        &self,
        state: &AttemptReplayState,
    ) -> Vec<u8> {
        let mut out = crate::canonical::CanonicalWriter::with_domain("rad-attempt-checkpoint/v3");
        state.encode_checkpoint(&mut out);
        out.text(&self.program_digest());
        out.text(&self.runtime_feature_fingerprint());
        out.text(&self.constraint_registry_digest());
        out.text(&self.constraint_limit_profile.fingerprint());
        out.text(&crate::vm::replay_clone::fingerprint_roots(
            &state.root_layout.roots,
        ));
        out.finish()
    }
}
