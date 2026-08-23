impl VM {
    /// Immutable global symbol table in exact slot order.
    pub fn global_symbols(&self) -> &[String] {
        self.global_names.as_slice()
    }

    /// Capture the canonical immutable identity of the currently installed
    /// executable program. Runtime values are intentionally excluded.
    pub fn compiled_program_manifest(&self) -> Result<CompiledProgramManifest, String> {
        CompiledProgramManifest::capture(self).map_err(|error| error.to_string())
    }

    /// Content-addressed manifests for native implementations installed in
    /// this executable program. The returned slice is immutable; loading or
    /// replacing an implementation produces a new program identity.
    pub fn native_extension_manifests(&self) -> &[Arc<crate::ffi::NativeExtensionManifest>] {
        &self.native_extension_manifests
    }

    /// Install the immutable RFC-0003 schema artifact into both executable
    /// identity and operational world state. Reinstalling an identical
    /// manifest is idempotent; replacing one is rejected.
    pub fn install_relation_frontend(
        &mut self,
        artifacts: &crate::relation::frontend::FrontendArtifacts,
    ) -> crate::relation::runtime::RelationRuntimeResult<()> {
        let manifest =
            Arc::new(crate::relation::runtime::RelationRuntimeManifest::from_frontend(artifacts)?);
        if let Some(installed) = &self.relation_runtime_manifest {
            if installed.digest() != manifest.digest() {
                return Err(crate::relation::runtime::RelationRuntimeError {
                    code: "relation.manifest_already_installed",
                    detail: "a different relation program is already installed".into(),
                });
            }
        }
        self.world
            .install_relation_manifest(Arc::clone(&manifest), artifacts.manifest_digest)?;
        self.relation_runtime_manifest = Some(manifest);
        Ok(())
    }

    pub fn relation_runtime_manifest(
        &self,
    ) -> Option<&Arc<crate::relation::runtime::RelationRuntimeManifest>> {
        self.relation_runtime_manifest.as_ref()
    }

    /// Execute the authoritative operation section emitted by the bounded
    /// front end. This is an embedding boundary, not derived evaluation: all
    /// symbolic entity operands are ground names in the current world and
    /// the complete operation batch commits atomically.
    pub fn apply_frontend_relation_operations(
        &mut self,
        artifacts: &crate::relation::frontend::FrontendArtifacts,
    ) -> crate::relation::runtime::RelationRuntimeResult<Vec<crate::relation::runtime::FactChange>>
    {
        self.install_relation_frontend(artifacts)?;
        let transaction =
            crate::relation::runtime::RelationTransaction::from_frontend(artifacts, |name| {
                self.world
                    .get_entity_by_name(name)
                    .and_then(|id| self.world.entity_ref(id))
            })?;
        let transaction = crate::relation::runtime::BoundedRelationTransaction::try_new(
            transaction,
            &crate::relation::runtime::RelationTransactionProfile::default(),
        )?;
        self.world.apply_bounded_relation_transaction(&transaction)
    }

    #[inline(always)]
    pub fn get_world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    #[inline(always)]
    pub fn get_world(&self) -> &World {
        &self.world
    }

    /// Walk VM-heap roots and sweep unreachable GC allocations.
    ///
    /// ECS component/resource payloads live in the persistent store and do
    /// not need tracing. VM-owned roots include stacks, globals, queued and
    /// recorded events, tasks, settlement payloads, closures, and constants.
    pub fn collect_cycles(&mut self) -> usize {
        for val in &self.stack {
            val.trace(&mut self.gc);
        }
        for val in &self.globals {
            val.trace(&mut self.gc);
        }
        // In-flight event payloads are live state (latent-bug fix found
        // during the #7 composition pass: queued payloads were not roots).
        for (_, payload, _) in self
            .events_current
            .iter()
            .chain(self.events_next.iter())
            .chain(self.events_processing.iter())
        {
            payload.trace(&mut self.gc);
        }
        for (_, _, payload, _) in &self.delayed_events {
            payload.trace(&mut self.gc);
        }
        for frame in &self.frames {
            if let Some(captures) = &frame.captures {
                for &cell in captures.as_ref() {
                    if unsafe { self.gc.mark(cell) } {
                        unsafe { (*cell).get().trace(&mut self.gc) };
                    }
                }
            }
        }
        // Completed-but-unawaited task results and recorded event payloads
        // are reachable from rad code, so they are roots too.
        for task in self.tasks.values() {
            if let TaskStatus::Completed(val) = &task.status {
                val.trace(&mut self.gc);
            }
        }
        for entry in &self.event_log {
            entry.payload.trace(&mut self.gc);
        }
        if let Some(settlement) = &self.settlement {
            for proposal in &settlement.proposals {
                proposal.payload.trace(&mut self.gc);
            }
            for patch in &settlement.patches {
                for write in &patch.writes {
                    for value in &write.component.values {
                        value.trace(&mut self.gc);
                    }
                }
            }
            if let Some(active) = &settlement.active {
                for write in &active.writes {
                    for value in &write.component.values {
                        value.trace(&mut self.gc);
                    }
                }
            }
        }
        for chunk in self.chunks.iter() {
            for val in &chunk.constants {
                val.trace(&mut self.gc);
            }
        }
        unsafe { self.gc.sweep() }
    }

    #[inline]
    pub(crate) fn allocate_frame_id(&mut self) -> u64 {
        let id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.wrapping_add(1).max(1);
        id
    }

    #[inline]
    pub(crate) fn allocate_settlement_id(&mut self) -> u64 {
        let id = self.next_settlement_id;
        self.next_settlement_id = self.next_settlement_id.wrapping_add(1).max(1);
        id
    }

    pub(crate) fn shared_state(&self) -> VmSharedState {
        VmSharedState {
            chunks: self.chunks.clone(),
            view_kernels: self.view_kernels.clone(),
            globals: self.globals.clone(),
            global_names: self.global_names.clone(),
            program_source_identity: self.program_source_identity.clone(),
            relation_runtime_manifest: self.relation_runtime_manifest.clone(),
            state_machines: self.state_machines.clone(),
            event_handlers: self.event_handlers.clone(),
            systems: self.systems.clone(),
            intent_registry: self.intent_registry.clone(),
            resolver_registry: self.resolver_registry.clone(),
            constraint_registry: self.constraint_registry.clone(),
            native_extension_manifests: self.native_extension_manifests.clone(),
            component_layouts: self.component_layouts.clone(),
            native_layouts: self.native_layouts.clone(),
            component_field_types: self.component_field_types.clone(),
            component_versions: self.component_versions.clone(),
            variant_layouts: self.variant_layouts.clone(),
            transient_resources: self.transient_resources.clone(),
            rng_state: self.rng_state,
            suppress_output: self.suppress_output,
            profile_copies: self.profile_copies,
            collect_system_metrics: self.system_metrics.is_some(),
            causal_value_limits: self.causal_value_limits,
            constraint_limit_profile: self.constraint_limit_profile.clone(),
        }
    }

    pub(crate) fn from_shared_state(shared: VmSharedState) -> Self {
        VM {
            chunks: shared.chunks,
            view_kernels: shared.view_kernels,
            stack: Vec::with_capacity(1024),
            globals: shared.globals,
            global_names: shared.global_names,
            program_source_identity: shared.program_source_identity,
            relation_runtime_manifest: shared.relation_runtime_manifest.clone(),
            frames: Vec::with_capacity(256),
            next_frame_id: 1,
            world: {
                let mut world = World::new();
                if let Some(manifest) = &shared.relation_runtime_manifest {
                    let _ = world.install_relation_manifest(
                        Arc::clone(manifest),
                        manifest.frontend_digest(),
                    );
                }
                world
            },
            state_machines: shared.state_machines,
            event_handlers: shared.event_handlers,
            systems: shared.systems,
            intent_registry: shared.intent_registry,
            resolver_registry: shared.resolver_registry,
            constraint_registry: shared.constraint_registry,
            native_extension_manifests: shared.native_extension_manifests,
            settlement: None,
            transaction: None,
            post_commit: None,
            next_settlement_id: 1,
            causal_value_limits: shared.causal_value_limits,
            constraint_limit_profile: shared.constraint_limit_profile,
            migrations: HashMap::new(),
            events_current: Vec::new(),
            events_next: Vec::new(),
            events_processing: Vec::new(),
            delayed_events: Vec::new(),
            print_buffer: Vec::new(),
            eprint_buffer: Vec::new(),
            suppress_output: shared.suppress_output,
            profile_copies: shared.profile_copies,
            op_profile: false, // worker VMs don't profile (histogram is per main VM)
            op_counts: Vec::new(),
            system_metrics: shared.collect_system_metrics.then(BTreeMap::new),
            model_check_config: ModelCheckConfig::default(),
            model_check_reports: Vec::new(),
            model_access_trace: None,
            metered_instruction_count: 0,
            trace_timeline: false,
            trace_patch: None,
            component_layouts: shared.component_layouts,
            native_layouts: shared.native_layouts,
            component_field_types: shared.component_field_types,
            component_versions: shared.component_versions,
            variant_layouts: shared.variant_layouts,
            transient_resources: shared.transient_resources,
            // Worker VMs execute systems, never `test` declarations.
            shared_world_tests: Arc::new(HashSet::new()),
            indexed_decl: Arc::new(HashMap::new()),
            ordered_decl: Arc::new(HashMap::new()),
            timeline: Vec::new(),
            event_log: VecDeque::new(),
            capture_isolated_event_log: false,
            rng_state: shared.rng_state,
            tasks: HashMap::new(),
            next_task_id: 1,
            pending_io: HashMap::new(),
            in_async_context: false,
            #[cfg(not(target_arch = "wasm32"))]
            // Worker VMs are effect-isolated and may not perform I/O. Keeping
            // this disabled also matters on Windows: WORKER_VM is thread-local
            // to a Rayon thread, and joining a nested I/O thread from that
            // TLS destructor can race process teardown and abort after an
            // otherwise successful simulate_par() run.
            io_pool: IoPool::disabled(),
            #[cfg(not(target_arch = "wasm32"))]
            net_handles: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            next_net_handle_id: 1,
            current_trace_id: None,
            next_trace_id: 1,
            in_simulation_fork: 0,
            gc_pause: 0,
            is_worker: true,
            sys_args: Vec::new(),
            command_buffer: Vec::new(),
            once_guard_passed: false,
            fuel: u64::MAX,
            mem_limit: usize::MAX,
            constraint_meter: None,
            sandbox_caps: None,
            sandbox_input_json: None,
            sandbox_output_json: None,
            last_sandbox_output_json: None,
            last_sandbox_fuel_spent: 0,
            serial_schedule: false,
            recorder: None,
            replayer: None,
            nested_native_tape: None,
            observational_attempt_replay: false,
            ledger: crate::causality::CausalityLedger::default(),
            current_cause: crate::causality::Cause::Main,
            pending_host_cause: None,
            causality_frame: 0,
            emit_ids_current: Vec::new(),
            emit_ids_next: Vec::new(),
            gc: GcHeap::new(),
            arena: BumpArena::new(),
        }
    }

    pub(crate) fn sync_from_shared(&mut self, shared: &VmSharedState) {
        // Arc identity, not length: two different programs can have the same
        // chunk count, and a pooled worker VM must never run one program's
        // schedule against another program's system table.
        if !Arc::ptr_eq(&self.chunks, &shared.chunks) {
            self.chunks = Arc::clone(&shared.chunks);
            self.view_kernels = Arc::clone(&shared.view_kernels);
            self.global_names = Arc::clone(&shared.global_names);
            self.program_source_identity = shared.program_source_identity.clone();
            self.relation_runtime_manifest = shared.relation_runtime_manifest.clone();
            self.state_machines = Arc::clone(&shared.state_machines);
            self.event_handlers = Arc::clone(&shared.event_handlers);
            self.systems = Arc::clone(&shared.systems);
            self.intent_registry = Arc::clone(&shared.intent_registry);
            self.resolver_registry = Arc::clone(&shared.resolver_registry);
            self.constraint_registry = Arc::clone(&shared.constraint_registry);
            self.component_layouts = Arc::clone(&shared.component_layouts);
            self.native_layouts = Arc::clone(&shared.native_layouts);
            self.component_field_types = Arc::clone(&shared.component_field_types);
            self.component_versions = Arc::clone(&shared.component_versions);
            self.variant_layouts = Arc::clone(&shared.variant_layouts);
            self.transient_resources = Arc::clone(&shared.transient_resources);
        }
        // Globals refresh EVERY sync, not only on program change: global
        // values are handles into the MAIN VM's GC heap, and top-level
        // `let mut` rebinding makes the old objects garbage that the main
        // collector frees between parallel calls. A pooled worker that kept
        // its creation-time copy would then hold dangling pointers — its own
        // collector TRACES every global as a root, so the very next worker
        // GC dereferenced freed memory (A2's simulate_par 0xC0000005, ~1 in
        // 3 runs; allocation-shape dependent, which is why a str field
        // modulated it).
        self.globals = shared.globals.clone();
        // Relation artifacts may be installed after a worker pool has already
        // cached the same bytecode Arc. Their executable identity must refresh
        // independently of chunk identity just like globals do.
        self.relation_runtime_manifest = shared.relation_runtime_manifest.clone();
        self.native_extension_manifests = Arc::clone(&shared.native_extension_manifests);
        self.suppress_output = shared.suppress_output;
        self.profile_copies = shared.profile_copies;
        if shared.collect_system_metrics && self.system_metrics.is_none() {
            self.system_metrics = Some(BTreeMap::new());
        } else if !shared.collect_system_metrics {
            self.system_metrics = None;
        }
        self.constraint_limit_profile = shared.constraint_limit_profile.clone();
        self.stack.clear();
        self.frames.clear();
        self.events_next.clear();
        self.emit_ids_next.clear();
        for cmd in &self.command_buffer {
            cmd.release_payload();
        }
        self.command_buffer.clear();
    }

    pub fn new() -> Self {
        Self::new_with_seed(Self::initial_rng_seed())
    }

    /// Construct a VM without consulting host time for its initial RNG seed.
    ///
    /// This is the deterministic embedding entry point for tests, replay
    /// harnesses, and isolated interpreters such as Miri. A zero seed is
    /// normalized to the same deterministic non-zero seed used by `set_random_seed`.
    pub fn new_with_seed(seed: u64) -> Self {
        // Runtime arity validation is a hot-path table lookup. Derive that
        // table before any system allocation scope can begin, including for
        // checker-less bytecode embedders.
        crate::builtins::initialize_builtin_call_shapes();
        let mut gc = GcHeap::new();
        let mut globals = Vec::new();
        let mut global_names = Vec::new();
        for builtin in Builtin::ALL {
            global_names.push(builtin.name().to_string());
            globals.push(Value::from_builtin(&mut gc, builtin));
        }
        VM {
            chunks: Arc::new(Vec::new()),
            view_kernels: Arc::new(Vec::new()),
            stack: Vec::new(),
            globals,
            global_names: Arc::new(global_names),
            program_source_identity: None,
            relation_runtime_manifest: None,
            frames: Vec::new(),
            next_frame_id: 1,
            world: World::new(),
            state_machines: Arc::new(HashMap::new()),
            event_handlers: Arc::new(HashMap::new()),
            systems: Arc::new(HashMap::new()),
            intent_registry: Arc::new(HashMap::new()),
            resolver_registry: Arc::new(HashMap::new()),
            constraint_registry: Arc::new(Vec::new()),
            native_extension_manifests: Arc::new(Vec::new()),
            settlement: None,
            transaction: None,
            post_commit: None,
            next_settlement_id: 1,
            causal_value_limits: crate::CausalValueLimits::default(),
            constraint_limit_profile: crate::constraint_types::ConstraintLimitProfile::default(),
            migrations: HashMap::new(),
            events_current: Vec::new(),
            events_next: Vec::new(),
            events_processing: Vec::new(),
            delayed_events: Vec::new(),
            print_buffer: Vec::new(),
            eprint_buffer: Vec::new(),
            suppress_output: false,
            profile_copies: false,
            op_profile: std::env::var("RAD_OP_PROFILE").is_ok_and(|v| v == "1"),
            op_counts: vec![0u64; 256],
            system_metrics: None,
            model_check_config: ModelCheckConfig::default(),
            model_check_reports: Vec::new(),
            model_access_trace: None,
            metered_instruction_count: 0,
            trace_timeline: false,
            trace_patch: None,
            component_layouts: Arc::new(HashMap::new()),
            native_layouts: Arc::new(HashMap::new()),
            shared_world_tests: Arc::new(HashSet::new()),
            component_field_types: Arc::new(HashMap::new()),
            component_versions: Arc::new(HashMap::new()),
            variant_layouts: Arc::new(HashMap::new()),
            transient_resources: Arc::new(HashSet::new()),
            indexed_decl: Arc::new(HashMap::new()),
            ordered_decl: Arc::new(HashMap::new()),
            timeline: Vec::new(),
            event_log: VecDeque::new(),
            capture_isolated_event_log: false,
            rng_state: Self::normalize_random_seed(seed),
            tasks: HashMap::new(),
            next_task_id: 1,
            pending_io: HashMap::new(),
            in_async_context: false,
            #[cfg(not(target_arch = "wasm32"))]
            io_pool: IoPool::new(4),
            #[cfg(not(target_arch = "wasm32"))]
            net_handles: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            next_net_handle_id: 1,
            current_trace_id: None,
            next_trace_id: 1,
            in_simulation_fork: 0,
            gc_pause: 0,
            is_worker: false,
            sys_args: Vec::new(),
            command_buffer: Vec::new(),
            once_guard_passed: false,
            fuel: u64::MAX,
            mem_limit: usize::MAX,
            constraint_meter: None,
            sandbox_caps: None,
            sandbox_input_json: None,
            sandbox_output_json: None,
            last_sandbox_output_json: None,
            last_sandbox_fuel_spent: 0,
            serial_schedule: false,
            recorder: None,
            replayer: None,
            nested_native_tape: None,
            observational_attempt_replay: false,
            ledger: crate::causality::CausalityLedger::default(),
            current_cause: crate::causality::Cause::Main,
            pending_host_cause: None,
            causality_frame: 0,
            emit_ids_current: Vec::new(),
            emit_ids_next: Vec::new(),
            gc,
            arena: BumpArena::new(),
        }
    }

    /// Start recording a replay trace. Captures the current RNG state as the
    /// trace seed, so call this before `run` and after any seed override.
    pub fn enable_recording(&mut self, source: &str) {
        self.recorder = Some(crate::replay::TraceRecorder::new(source, self.rng_state));
    }

    /// Record the exact language-feature contract needed to compile the
    /// embedded source again. The feature list is canonicalized and protected
    /// by its own trace-header digest.
    pub fn enable_recording_with_features(&mut self, source: &str, features: &[String]) {
        self.recorder = Some(crate::replay::TraceRecorder::new_with_features(
            source,
            self.rng_state,
            features,
        ));
    }

    /// Record a self-contained multi-module source bundle. The authenticated
    /// layout is data, not a lexer convention, so arbitrary comments cannot
    /// alter replay locations.
    pub fn enable_recording_with_source_layout(
        &mut self,
        source: &str,
        features: &[String],
        source_layout: &crate::source_bundle::SourceLayout,
    ) {
        self.recorder = Some(crate::replay::TraceRecorder::new_with_features_and_layout(
            source,
            self.rng_state,
            features,
            source_layout,
        ));
    }

    /// Stream a canonical compressed trace directly to `output_path`.
    /// Recording memory is O(one serialized record), independent of session
    /// length; publication is atomic and happens only when recording finishes.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_recording_to_file(
        &mut self,
        source: &str,
        features: &[String],
        source_layout: &crate::source_bundle::SourceLayout,
        output_path: &std::path::Path,
    ) -> Result<(), String> {
        self.recorder = Some(
            crate::replay::TraceRecorder::new_file_with_features_and_layout(
                source,
                self.rng_state,
                features,
                source_layout,
                output_path,
            )?,
        );
        Ok(())
    }

    /// Finish recording and return the trace as JSONL, if recording was on.
    /// Appends the end record (world digest at exit or crash point) that
    /// replay verifies itself against.
    pub fn take_trace(&mut self) -> Option<String> {
        self.take_trace_with_outcome(None)
    }

    pub fn take_trace_with_outcome(&mut self, error: Option<&str>) -> Option<String> {
        let digest = self.world.content_digest();
        self.recorder.take().map(|mut r| {
            r.record_end_with_outcome(&digest, error);
            r.to_jsonl()
        })
    }

    /// Finish either an in-memory embedding trace or a file-backed CLI trace.
    pub fn finish_recording_with_outcome(
        &mut self,
        error: Option<&str>,
    ) -> Result<Option<crate::replay::FinishedTrace>, String> {
        let digest = self.world.content_digest();
        let Some(mut recorder) = self.recorder.take() else {
            return Ok(None);
        };
        recorder.record_end_with_outcome(&digest, error);
        recorder.finish().map(Some)
    }

    /// Enter replay mode: managed builtins are served from the trace, and
    /// the RNG is rewound to the recorded seed. When timeline capture is on,
    /// the (empty) pre-run world becomes `timeline[0]`.
    pub fn enable_replay(&mut self, mut replayer: crate::replay::TraceReplayer) {
        self.set_random_seed(replayer.seed());
        if replayer.capturing_timeline() {
            replayer.push_timeline_snapshot(self.world.snapshot());
        }
        self.replayer = Some(replayer);
    }

    /// Finish replay and report how faithfully the trace was consumed.
    pub fn finish_replay(&mut self) -> Option<crate::replay::ReplayReport> {
        self.finish_replay_with_outcome(None)
    }

    pub fn finish_replay_with_outcome(
        &mut self,
        error: Option<&str>,
    ) -> Option<crate::replay::ReplayReport> {
        let digest = self.world.content_digest();
        self.replayer
            .take()
            .map(|r| r.report_with_outcome(&digest, error))
    }

    /// The causality ledger (#4): provenance of every main-timeline write.
    /// Reset the world, pending events and causal ledger to a captured
    /// fixture. The test runner calls this before each non-`shared` test so
    /// that execution order cannot become an unstated dependency.
    ///
    /// The causal ledger travels with the world: `why_removed` and friends
    /// answer questions about the writes that produced the current state, and
    /// leaving a previous test's writes in place would explain entities this
    /// world no longer contains.
    pub(crate) fn restore_fixture(
        &mut self,
        world: &crate::world::WorldSnapshot,
        ledger: &crate::causality::CausalityLedger,
    ) {
        self.world.restore(world.clone());
        self.restore_events_from(world);
        self.ledger = ledger.clone();
        self.causality_frame = 0;
    }

    /// CoW snapshot of the live world (pub for the CLI's retroactive-edit
    /// diff, which compares the original and edited timelines).
    pub fn world_snapshot(&self) -> crate::world::WorldSnapshot {
        self.world.snapshot()
    }

    /// Snapshot the world **plus the in-flight event queue** — the full
    /// program state at this instant. Payloads are persisted so the snapshot
    /// outlives any GC epoch. This is what `fork()` hands out: a snapshot
    /// that drops pending events is not a snapshot (composition pass, #7).
    pub(crate) fn snapshot_with_events(&self) -> crate::world::WorldSnapshot {
        let mut snap = self.world.snapshot();
        if !self.events_next.is_empty() {
            let events: Vec<(String, Value, u64)> = self
                .events_next
                .iter()
                .map(|(name, payload, tid)| {
                    let persisted = payload.deep_copy(&mut crate::value::PersistentStore);
                    (name.clone(), persisted, *tid)
                })
                .collect();
            snap.events = std::sync::Arc::new(events);
            snap.emit_ids = std::sync::Arc::new(self.emit_ids_next.clone());
        }
        if !self.delayed_events.is_empty() {
            let delayed: Vec<(i64, String, Value, u64)> = self
                .delayed_events
                .iter()
                .map(|(left, name, payload, emit_id)| {
                    let persisted = payload.deep_copy(&mut crate::value::PersistentStore);
                    (*left, name.clone(), persisted, *emit_id)
                })
                .collect();
            snap.delayed = std::sync::Arc::new(delayed);
        }
        snap
    }
}
