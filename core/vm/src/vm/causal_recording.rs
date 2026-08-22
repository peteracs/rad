// Recording provenance for authoritative writes, and serialising the causal
// ledger for inspection, wire transport, and replay.

impl VM {
    fn take_mutation_cause(&mut self) -> crate::causality::Cause {
        self.pending_host_cause
            .take()
            .unwrap_or_else(|| self.current_cause.clone())
    }

    pub fn causality_ledger(&self) -> &crate::causality::CausalityLedger {
        &self.ledger
    }

    /// Bound retained causal history for long-running embedded VMs.
    /// Settlement proposal fan-in, writes, and event ancestry share this
    /// retention policy.
    pub fn set_causality_retention_cap(&mut self, cap: usize) {
        self.ledger.set_retention_cap(cap);
    }

    /// Take the ledger out of the VM (the time-travel server keeps it to
    /// answer `why` at any frame).
    pub fn take_causality_ledger(&mut self) -> crate::causality::CausalityLedger {
        std::mem::take(&mut self.ledger)
    }

    /// Record a main-timeline world write in the causality ledger. No-op in
    /// simulation forks and worker VMs — speculative or staged writes get
    /// their provenance when (and if) they land on the main world.
    pub(crate) fn record_causal_write(
        &mut self,
        entity: Option<u32>,
        component: &str,
        kind: crate::causality::WriteKind,
        value: String,
    ) {
        let entity_name = entity.and_then(|id| self.world.entity_name(id));
        self.record_causal_write_named(entity, entity_name, component, kind, value);
    }

    pub(crate) fn record_causal_write_named(
        &mut self,
        entity: Option<u32>,
        entity_name: Option<String>,
        component: &str,
        kind: crate::causality::WriteKind,
        value: String,
    ) {
        let fields = match kind {
            crate::causality::WriteKind::Set | crate::causality::WriteKind::Spawn => entity
                .and_then(|entity| self.world.get_component(entity, component))
                .map(|data| Self::component_field_summaries(&data))
                .unwrap_or_default(),
            crate::causality::WriteKind::Resource => self
                .world
                .get_resource(component)
                .map(|data| Self::component_field_summaries(&data))
                .unwrap_or_default(),
            crate::causality::WriteKind::Despawn | crate::causality::WriteKind::Remove => {
                Vec::new()
            }
        };
        if let Some(transaction) = &mut self.transaction {
            transaction.pending_writes.push(PendingTransactionWrite {
                entity,
                entity_name,
                component: component.to_string(),
                value,
                kind,
                fields,
                view_refresh: crate::vm::transaction::MaterializedViewRefresh::Component,
            });
            return;
        }
        if let Some(entity) = entity {
            self.world.refresh_materialized_views_for(component, entity);
        }
        if self.in_simulation_fork > 0 || self.is_worker {
            return;
        }
        let cause = self.take_mutation_cause();
        self.ledger
            .record_write(crate::causality::WriteRecord::local(
                self.causality_frame,
                entity,
                entity_name,
                component,
                crate::causality::WriteSummary::text_fields(value, fields),
                kind,
                cause,
            ));
    }

    /// Record one exact field mutation without formatting the untouched
    /// component or reevaluating views that depend only on component presence.
    pub(crate) fn record_causal_field_write(
        &mut self,
        entity: u32,
        component: &str,
        field: &str,
        field_value: Value,
    ) {
        let entity_name = self.world.entity_name(entity);
        self.record_causal_field_write_resolved(
            entity,
            entity_name,
            component,
            field,
            field_value,
            true,
        );
    }

    pub(crate) fn record_causal_field_write_resolved(
        &mut self,
        entity: u32,
        entity_name: Option<String>,
        component: &str,
        field: &str,
        field_value: Value,
        refresh_view: bool,
    ) {
        if let Some(transaction) = &mut self.transaction {
            let field_value = field_value.to_string();
            let value = format!("{{ {field}: {field_value} }}");
            let fields = vec![(field.to_string(), crate::causality::summarize(&field_value))];
            transaction.pending_writes.push(PendingTransactionWrite {
                entity: Some(entity),
                entity_name,
                component: component.to_string(),
                value,
                kind: crate::causality::WriteKind::Set,
                fields,
                view_refresh: crate::vm::transaction::MaterializedViewRefresh::Field(
                    field.to_string(),
                ),
            });
            return;
        }
        if refresh_view {
            self.world
                .refresh_materialized_views_for_field(component, field, entity);
        }
        if self.in_simulation_fork > 0 || self.is_worker {
            return;
        }
        let cause = self.take_mutation_cause();
        self.ledger
            .record_write(crate::causality::WriteRecord::local(
                self.causality_frame,
                Some(entity),
                entity_name,
                component,
                crate::causality::WriteSummary::Field((
                    field.into(),
                    crate::causality::CausalScalar::from_value(&field_value),
                )),
                crate::causality::WriteKind::Set,
                cause,
            ));
    }

    /// Bounded display summary of a component's fields, for ledger records.
    pub(crate) fn component_summary(data: &crate::value::ComponentData) -> String {
        let mut s = String::from("{ ");
        for (i, (k, v)) in data.layout.iter().zip(data.values.iter()).enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(k);
            s.push_str(": ");
            s.push_str(&v.to_string());
        }
        s.push_str(" }");
        crate::causality::summarize(&s)
    }

    fn component_field_summaries(data: &crate::value::ComponentData) -> Vec<(String, String)> {
        data.layout
            .iter()
            .zip(data.values.iter())
            .map(|(field, value)| {
                (
                    field.clone(),
                    crate::causality::summarize(&value.to_string()),
                )
            })
            .collect()
    }

    /// Render an event payload for the causality ledger with entity fields
    /// resolved to their spawn names: `Clawed { monster: ghoul-1, dmg: 2 }`
    /// instead of `Clawed { monster: 1, dmg: 2 }`. The ledger already names
    /// the *written* entity ("Vital of you"); the emitter's payload deserves
    /// the same courtesy. Unnamed entities keep their bare id. Only the
    /// shapes an event payload can carry are walked; everything else
    /// falls back to the plain `Display`.
    pub(crate) fn ledger_payload(&self, v: &Value) -> String {
        use crate::value::Object;
        use std::fmt::Write as _;
        fn walk(world: &crate::world::World, v: &Value, out: &mut String, depth: usize) {
            if depth > 6 {
                out.push('…');
                return;
            }
            if let Some(id) = v.as_entity_id() {
                match world.entity_name(id) {
                    Some(name) => {
                        let _ = write!(out, "{}", name);
                    }
                    None => {
                        let _ = write!(out, "{}", id);
                    }
                }
                return;
            }
            match v.as_object() {
                Some(Object::Component(c)) => {
                    let _ = write!(out, "{} {{", crate::value::display_type_name(&c.type_name));
                    if !c.layout.is_empty() {
                        out.push(' ');
                        let mut first = true;
                        for (k, fv) in c.layout.iter().zip(c.values.iter()) {
                            if !first {
                                out.push_str(", ");
                            }
                            first = false;
                            let _ = write!(out, "{}: ", k);
                            walk(world, fv, out, depth + 1);
                        }
                        out.push(' ');
                    }
                    out.push('}');
                }
                Some(Object::List(list)) => {
                    out.push('[');
                    for (i, item) in list.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        walk(world, item, out, depth + 1);
                    }
                    out.push(']');
                }
                Some(Object::Tuple(items)) => {
                    out.push('(');
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        walk(world, item, out, depth + 1);
                    }
                    if items.len() == 1 {
                        out.push(',');
                    }
                    out.push(')');
                }
                _ => {
                    let _ = write!(out, "{}", v);
                }
            }
        }
        let mut out = String::new();
        walk(&self.world, v, &mut out, 0);
        out
    }

    /// Enqueue an event from the host (streaming sessions, D4): the
    /// embedder-side mirror of `Op::Emit`. The payload joins `events_next`
    /// with a fresh trace id and a causality emit record, exactly as if a
    /// rad program had executed `emit Name { .. }` — so `why()` answers for
    /// host-pushed events too, and the next `flush_events()` dispatches it
    /// on the same frame clock.
    pub(crate) fn enqueue_event(&mut self, payload: Value) -> Result<(), String> {
        let event_name = payload.type_name().to_string();
        if !self.event_handlers.contains_key(&event_name)
            && !self.component_layouts.contains_key(&event_name)
        {
            return Err(format!(
                "enqueue_event: '{}' is not a declared event",
                event_name
            ));
        }
        let trace_id = self.next_trace_id;
        self.next_trace_id += 1;
        let emit_id = {
            let summary = crate::causality::summarize(&self.ledger_payload(&payload));
            // Host pushes are top-level-equivalent: there is no rad frame
            // above them to attribute to.
            self.ledger.record_emit(
                self.causality_frame,
                &event_name,
                summary,
                crate::causality::Cause::Main,
            )
        };
        self.emit_ids_next.push(emit_id);
        self.events_next.push((event_name, payload, trace_id));
        Ok(())
    }

    /// Restore the in-flight event queue from a snapshot (the other half of
    /// `snapshot_with_events`): pending events and their causality ids come
    /// back exactly as captured.
    pub(crate) fn restore_events_from(&mut self, snap: &crate::world::WorldSnapshot) {
        self.events_current.clear();
        self.emit_ids_current.clear();
        self.events_next = snap.events.as_ref().clone();
        self.emit_ids_next = snap.emit_ids.as_ref().clone();
        // delayed timers travel with the snapshot: a rewind must not keep
        // the abandoned timeline's timers ticking, nor lose the target's
        self.delayed_events = snap.delayed.as_ref().clone();
    }

    /// Content digest of the live world (pub for the CLI).
    pub fn world_digest(&self) -> String {
        self.world.content_digest()
    }

    /// Finish a time-travel replay session: appends the program-end world as
    /// the final timeline entry and returns (timeline, report).
    pub fn finish_replay_session(
        &mut self,
    ) -> Option<(
        Vec<std::sync::Arc<crate::world::WorldSnapshot>>,
        crate::replay::ReplayReport,
    )> {
        self.finish_replay_session_with_outcome(None)
    }

    pub fn finish_replay_session_with_outcome(
        &mut self,
        error: Option<&str>,
    ) -> Option<(
        Vec<std::sync::Arc<crate::world::WorldSnapshot>>,
        crate::replay::ReplayReport,
    )> {
        let end_snap = self.world.snapshot();
        let digest = self.world.content_digest();
        self.replayer.take().map(|mut replayer| {
            replayer.push_timeline_snapshot(end_snap);
            let timeline = replayer.take_timeline();
            (timeline, replayer.report_with_outcome(&digest, error))
        })
    }

    pub fn suppress_output(&mut self) {
        self.suppress_output = true;
    }

    /// Force `schedule`/phases to run serially in topological order (the
    /// `--serial-schedule` lever). No effect on explicit `simulate_par`.
    pub fn set_serial_schedule(&mut self, serial: bool) {
        self.serial_schedule = serial;
    }

    pub fn set_profile_copies(&mut self, enabled: bool) {
        self.profile_copies = enabled;
        crate::value::set_profile_copy_context(enabled, 0);
    }

    /// Enable exact per-system instruction, guest-heap, VM-runtime, managed
    /// backing, and metered host-boundary allocation metrics.
    /// Collection is opt-in so normal execution pays no counter updates.
    pub fn enable_system_metrics(&mut self) {
        self.system_metrics = Some(BTreeMap::new());
        self.metered_instruction_count = 0;
    }

    pub fn system_execution_metrics(&self) -> Option<&BTreeMap<String, SystemExecutionMetrics>> {
        self.system_metrics.as_ref()
    }

    pub fn configure_model_check(&mut self, config: ModelCheckConfig) {
        self.model_check_config = config;
        self.model_check_reports.clear();
    }

    pub fn model_check_reports(&self) -> &[ModelCheckReport] {
        &self.model_check_reports
    }

    fn initial_rng_seed() -> u64 {
        #[cfg(target_arch = "wasm32")]
        {
            0xA5A5_5A5A_C3C3_3C3C
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xA5A5_5A5A_C3C3_3C3C);
            let stack_addr = (&nanos as *const u64 as usize) as u64;
            let seed = nanos ^ stack_addr.rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15;
            if seed == 0 {
                0xD1B5_4A32_D192_ED03
            } else {
                seed
            }
        }
    }

    fn normalize_random_seed(seed: u64) -> u64 {
        if seed == 0 {
            0xD1B5_4A32_D192_ED03
        } else {
            seed
        }
    }

    pub(crate) fn set_random_seed(&mut self, seed: u64) {
        self.rng_state = Self::normalize_random_seed(seed);
    }

    pub(crate) fn next_random_u64(&mut self) -> u64 {
        if self.rng_state == 0 {
            self.rng_state = 0xD1B5_4A32_D192_ED03;
        }
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub(crate) fn next_random_f64(&mut self) -> f64 {
        let n = self.next_random_u64() >> 11;
        (n as f64) * (1.0 / ((1u64 << 53) as f64))
    }

    pub(crate) fn random_bounded_u64(&mut self, bound: u64) -> u64 {
        if bound <= 1 {
            return 0;
        }
        let threshold = u64::MAX - (u64::MAX % bound);
        loop {
            let n = self.next_random_u64();
            if n < threshold {
                return n % bound;
            }
        }
    }

    /// Verify and append host-supplied bytecode. No instruction from an
    /// invalid chunk can execute.
    ///
    /// This safe raw-builder ingress accepts only immediate constants. Heap
    /// values carry untyped GC pointers, so accepting them here could not
    /// prove their ownership or lifetime. Compiler/WASM bundles use the
    /// explicit owning-heap path below.
    pub fn load_verified_chunk(&mut self, chunk: Chunk) -> Result<usize, crate::VerificationError> {
        if let Some(index) = chunk.constants().iter().position(Value::is_heap_object_tag) {
            return Err(crate::VerificationError::at(
                &chunk,
                0,
                format!(
                    "heap-backed constant {index} requires an owning GC bundle; raw chunk loading accepts immediate constants only"
                ),
            ));
        }
        let chunk = chunk.verify_and_seal()?;
        Ok(self.load_chunk(chunk))
    }

    /// Internal ingress for chunks whose constants were allocated directly
    /// in this VM's heap (tests and VM-owned construction only).
    #[cfg(test)]
    pub(crate) fn load_vm_owned_chunk(
        &mut self,
        chunk: Chunk,
    ) -> Result<usize, crate::VerificationError> {
        let chunk = chunk.verify_and_seal()?;
        Ok(self.load_chunk(chunk))
    }

    /// Append an immutable proof-bearing artifact. The bytes and proof cannot
    /// be separated or mutated through the public type.
    pub(crate) fn load_chunk(&mut self, chunk: SealedChunk) -> usize {
        let v = Arc::make_mut(&mut self.chunks);
        let id = v.len();
        v.push(chunk);
        debug_assert!(v[id].instruction_count() > 0);
        id
    }

    /// Verify bytecode before merging its separate constant heap, then append
    /// both atomically from the embedder's perspective.
    /// # Safety
    ///
    /// Every heap-backed value reachable from `chunk.constants` must be owned
    /// by `chunk_gc`. Passing unrelated storage can leave dangling raw object
    /// pointers after the source allocator is dropped.
    pub unsafe fn load_verified_chunk_with_gc(
        &mut self,
        chunk: Chunk,
        chunk_gc: GcHeap,
    ) -> Result<usize, crate::VerificationError> {
        let chunk = chunk.verify_and_seal()?;
        self.gc.merge(chunk_gc);
        Ok(self.load_chunk(chunk))
    }

    /// Runtime-defense tests deliberately bypass the host verifier. This is
    /// never compiled into a production library.
    #[cfg(test)]
    pub(crate) fn load_unchecked_chunk(&mut self, chunk: Chunk) -> usize {
        let chunks = Arc::make_mut(&mut self.chunks);
        let id = chunks.len();
        chunks.push(SealedChunk::from_unchecked_for_test(chunk));
        id
    }
}
