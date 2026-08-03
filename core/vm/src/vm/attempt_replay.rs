//! Exact pre-attempt checkpoints for observational RFC-0002 replay.
//!
//! A portable failed-attempt recipe contains no VM pointers. In-process
//! replay additionally retains one detached, graph-isolated VM captured
//! before the attempted host call begins. Replays fork that seed rather than
//! cloning the authoritative VM's later state.

use super::{TaskStatus, VM};
use crate::value::Value;
use sha2::{Digest, Sha256};

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
        let digest = source.attempt_checkpoint_digest();
        let seed = source.detached_attempt_replay_vm()?;
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
        Ok(())
    }

    /// Canonical identity of all pre-attempt state that observational replay
    /// may read. Heap topology uses deterministic graph numbering, never raw
    /// allocator addresses.
    pub(crate) fn attempt_checkpoint_digest(&self) -> String {
        fn bytes(digest: &mut Sha256, value: &[u8]) {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value);
        }
        fn text(digest: &mut Sha256, value: &str) {
            bytes(digest, value.as_bytes());
        }

        let layout = self.replay_root_layout();
        let mut digest = Sha256::new();
        digest.update(b"rad-attempt-checkpoint/v1\0");
        text(&mut digest, &self.world.content_digest());
        text(&mut digest, &self.program_digest());
        text(&mut digest, &self.runtime_feature_fingerprint());
        text(&mut digest, &self.constraint_registry_digest());
        text(&mut digest, &self.constraint_limit_profile.fingerprint());
        text(
            &mut digest,
            &crate::vm::replay_clone::fingerprint_roots(&layout.roots),
        );
        digest.update(self.rng_state.to_le_bytes());
        digest.update(self.fuel.to_le_bytes());
        digest.update((self.mem_limit as u64).to_le_bytes());
        digest.update(self.next_frame_id.to_le_bytes());
        digest.update(self.next_settlement_id.to_le_bytes());
        digest.update(self.next_task_id.to_le_bytes());
        digest.update(self.next_trace_id.to_le_bytes());
        digest.update(self.causality_frame.to_le_bytes());
        digest.update([self.once_guard_passed as u8]);
        for argument in &self.sys_args {
            text(&mut digest, argument);
        }
        for line in &self.print_buffer {
            text(&mut digest, line);
        }
        for line in &self.eprint_buffer {
            text(&mut digest, line);
        }
        for snapshot in &self.timeline {
            text(&mut digest, &snapshot.snapshot_json_like());
        }
        text(&mut digest, &format!("{:?}", self.ledger));
        text(&mut digest, &format!("{:?}", self.current_cause));
        text(&mut digest, &format!("{:?}", self.trace_patch));
        text(&mut digest, &format!("{:?}", self.sandbox_input_json));
        text(&mut digest, &format!("{:?}", self.sandbox_output_json));
        text(&mut digest, &format!("{:?}", self.last_sandbox_output_json));
        digest.update(self.last_sandbox_fuel_spent.to_le_bytes());
        for (name, _, id) in &self.events_current {
            text(&mut digest, name);
            digest.update(id.to_le_bytes());
        }
        for (name, _, id) in &self.events_next {
            text(&mut digest, name);
            digest.update(id.to_le_bytes());
        }
        for (name, _, id) in &self.events_processing {
            text(&mut digest, name);
            digest.update(id.to_le_bytes());
        }
        for (delay, name, _, id) in &self.delayed_events {
            digest.update(delay.to_le_bytes());
            text(&mut digest, name);
            digest.update(id.to_le_bytes());
        }
        for entry in &self.event_log {
            digest.update(entry.tick.to_le_bytes());
            text(&mut digest, &entry.event_name);
        }
        let mut tasks = self.tasks.iter().collect::<Vec<_>>();
        tasks.sort_by_key(|(id, _)| **id);
        for (id, task) in tasks {
            digest.update(id.to_le_bytes());
            match &task.status {
                TaskStatus::Ready => digest.update(b"ready"),
                TaskStatus::Completed(_) => digest.update(b"completed"),
                TaskStatus::Failed(message) => {
                    digest.update(b"failed");
                    text(&mut digest, message);
                }
            }
        }
        hex::encode(digest.finalize())
    }
}
