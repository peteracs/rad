impl VM {

    /// Borrow this VM’s [`GcHeap`] to build [`Value`]s from host/embedder code (e.g. arguments to
    /// [`Self::call_value`]). All such values must live on this heap before entering the VM.
    #[inline]
    pub(crate) fn gc_mut(&mut self) -> &mut GcHeap {
        &mut self.gc
    }

    pub fn load_compile_result(&mut self, mut result: CompileResult) {
        // Verify the complete compiler product before mutating VM registries
        // or merging its heap. Compiler corruption is an internal fault; host
        // supplied chunks use the fallible load_verified_chunk API above.
        let sealed_chunks = std::mem::take(&mut result.chunks)
            .into_iter()
            .map(|chunk| {
                chunk
                    .verify_and_seal()
                    .unwrap_or_else(|error| panic!("compiler produced invalid bytecode: {error}"))
            })
            .collect::<Vec<_>>();
        self.gc.merge(result.gc);
        {
            let intents = Arc::make_mut(&mut self.intent_registry);
            for intent in &result.intents {
                intents.insert(
                    intent.name.clone(),
                    IntentRuntimeInfo {
                        name: intent.name.clone(),
                        key_field: intent.key_field.clone(),
                        fields: Arc::new(intent.fields.clone()),
                    },
                );
            }
            let resolvers = Arc::make_mut(&mut self.resolver_registry);
            for resolver in &result.resolvers {
                resolvers.insert(
                    resolver.intent.clone(),
                    ResolverRuntimeInfo {
                        name: resolver.name.clone(),
                        intent: resolver.intent.clone(),
                        global_slot: resolver.global_slot,
                    },
                );
            }
            let constraints = Arc::make_mut(&mut self.constraint_registry);
            constraints.extend(
                result
                    .constraints
                    .iter()
                    .map(|constraint| ConstraintRuntimeInfo {
                        name: constraint.name.clone(),
                        attached_component: constraint.attached_component.clone(),
                        watches: Arc::new(constraint.watches.clone()),
                        global_slot: constraint.global_slot,
                    }),
            );
            constraints.sort_by(|left, right| {
                (&left.name, &left.attached_component)
                    .cmp(&(&right.name, &right.attached_component))
            });
        }
        {
            let sm = Arc::make_mut(&mut self.state_machines);
            for machine in result.state_machines {
                sm.insert(machine.name, machine.states);
            }
        }
        {
            let systems = Arc::make_mut(&mut self.systems);
            for sys in result.systems {
                let mut params: Vec<(String, bool, String)> = Vec::new();
                let mut resource_params: Vec<(String, bool, String)> = Vec::new();
                let mut accum_resources = std::collections::HashSet::new();
                for p in sys.params {
                    if p.is_accum && p.is_resource {
                        accum_resources.insert(p.comp_type.clone());
                    }
                    if p.is_resource {
                        resource_params.push((p.name, p.is_mut, p.comp_type));
                    } else {
                        params.push((p.name, p.is_mut, p.comp_type));
                    }
                }
                systems.insert(
                    sys.name,
                    SystemRuntimeInfo {
                        params,
                        resource_params,
                        chunk_id: sys.chunk_id,
                        after: sys.after,
                        before: sys.before,
                        serial_group: sys.serial_group,
                        accum_resources,
                    },
                );
            }
        }
        for m in &result.migrations {
            self.migrations.insert(
                m.component.clone(),
                MigrationEntry {
                    chunk_id: m.chunk_id,
                    param_slot: m.param_slot,
                    version_slot: m.version_slot,
                },
            );
        }
        {
            let eh = Arc::make_mut(&mut self.event_handlers);
            for h in result.handlers {
                eh.entry(h.event_name).or_default().push(HandlerEntry {
                    chunk_id: h.chunk_id,
                    param_slot: h.param_slot,
                    once: h.once,
                    fired: false,
                    has_guard: h.has_guard,
                });
            }
        }
        {
            let cl = Arc::make_mut(&mut self.component_layouts);
            for (name, layout) in result.component_layouts {
                cl.insert(name, Arc::new(layout));
            }
        }
        {
            let ft = Arc::make_mut(&mut self.component_field_types);
            for (name, fields) in result.component_field_types {
                ft.insert(name, Arc::new(fields));
            }
        }
        {
            let cv = Arc::make_mut(&mut self.component_versions);
            for (name, version) in result.component_versions {
                cv.insert(name, version);
            }
        }
        {
            let mut indexed_fields: HashMap<String, HashSet<String>> = (*self.indexed_decl).clone();
            for (name, fields) in result.indexed_component_fields {
                indexed_fields.insert(name, fields.into_iter().collect());
            }
            self.indexed_decl = Arc::new(indexed_fields);
            self.world
                .set_indexed_fields_arc(Arc::clone(&self.indexed_decl));
        }
        {
            let vl = Arc::make_mut(&mut self.variant_layouts);
            for (key, layout) in result.variant_layouts {
                vl.insert(key, layout);
            }
        }
        {
            let tr = Arc::make_mut(&mut self.transient_resources);
            tr.extend(result.transient_resources);
        }
        self.global_names = Arc::new(result.global_names);
        self.program_source_identity = result.program_source_identity.map(Arc::from);
        if self.globals.len() < self.global_names.len() {
            self.globals.resize(self.global_names.len(), Value::NIL);
        }
        for chunk in sealed_chunks {
            self.load_chunk(chunk);
        }
    }

    /// Execute a chunk and preserve structured settlement rejection data.
    pub fn run_detailed(
        &mut self,
        chunk_id: usize,
    ) -> Result<(), crate::constraint_types::VmFailure> {
        if chunk_id >= self.chunks.len() {
            return Err(format!("Invalid chunk id {}", chunk_id).into());
        }
        // A previous public boundary must already have enforced this
        // invariant. Restore transaction-local state defensively instead of
        // merely dropping a stale context if an internal caller violated it.
        self.abort_settlement();
        let next_frame_id_before = self.next_frame_id;
        crate::value::set_profile_copy_context(self.profile_copies, 0);
        self.frames.clear();
        self.print_buffer.clear();
        self.eprint_buffer.clear();
        self.events_current.clear();
        self.events_next.clear();
        self.events_processing.clear();
        self.emit_ids_current.clear();
        self.emit_ids_next.clear();
        self.tasks.clear();
        self.next_task_id = 1;
        self.pending_io.clear();
        self.in_async_context = false;
        let stack_base = self.stack.len();
        let frame_id = self.allocate_frame_id();
        self.frames.push(CallFrame {
            frame_id,
            chunk_id,
            ip: 0,
            stack_base,
            captures: None,
            system_writeback: None,
        });
        let result = self.run_frames(0).map_err(|failure| match failure {
            crate::constraint_types::VmFailure::Runtime(mut error) => {
                if !error.message.starts_with("[line") {
                    error.message = self.runtime_error(error.message);
                }
                crate::constraint_types::VmFailure::Runtime(error)
            }
            other => other,
        });
        let result = self.enforce_settlement_balance(result);
        if result.is_err() {
            // BeginSettlement and EndSettlement are separate bytecodes. A
            // body/law failure can bypass EndSettlement, so every public run
            // return must explicitly discard an unfinished transaction.
            self.abort_settlement();
            self.frames.clear();
            self.stack.truncate(stack_base);
            self.next_frame_id = next_frame_id_before;
        }
        crate::value::set_profile_copy_context(false, 0);
        if self.op_profile {
            let total: u64 = self.op_counts.iter().sum();
            eprintln!("== op profile: {} dispatches ==", total);
            let mut counted: Vec<(usize, u64)> = self
                .op_counts
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, n)| *n > 0)
                .collect();
            counted.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
            for (byte, n) in counted.into_iter().take(24) {
                let name = crate::opcode::Op::from_byte(byte as u8)
                    .map(|o| format!("{:?}", o))
                    .unwrap_or_else(|_| format!("op#{}", byte));
                eprintln!(
                    "  {:<18} {:>12}  {:>5.1}%",
                    name,
                    n,
                    n as f64 * 100.0 / total as f64
                );
            }
        }
        result
    }

    /// Compatibility execution API. New embedders should prefer
    /// [`Self::run_detailed`] so rejected settlements remain structured.
    pub fn run(&mut self, chunk_id: usize) -> Result<(), String> {
        self.run_detailed(chunk_id)
            .map_err(|failure| failure.render_compat())
    }

    pub(crate) fn allocate_task_id(&mut self) -> u64 {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        task_id
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn spawn_io_task<F>(&mut self, work: F) -> Value
    where
        F: FnOnce() -> Result<IoTaskPayload, String> + Send + 'static,
    {
        let task_id = self.allocate_task_id();
        self.tasks.insert(
            task_id,
            TaskRecord {
                id: task_id,
                status: TaskStatus::Ready,
            },
        );
        let rx = self.io_pool.submit(work);
        self.pending_io.insert(task_id, rx);
        Value::from_task(&mut self.gc, task_id)
    }
}