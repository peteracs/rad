impl VM {

    /// `simulate_many(forks, schedule, ticks, seed) -> [world_fork]`
    ///
    /// The heterogeneous sibling of `simulate_par`: instead of `n` rollouts of
    /// ONE fork, it runs each of the PROVIDED forks in parallel under the same
    /// schedule for `ticks`. This is the axis a search wants — B×K distinct
    /// candidate worlds evaluated at once — where `simulate_par`'s single-fork
    /// fan-out is the wrong dimension (dogfood feature seq 150). Per-fork RNG
    /// seeds derive from `(seed, index)` exactly like `simulate_par`, so a
    /// result is bit-identical for the same inputs regardless of thread count.
    fn bi_simulate_many(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(format!(
                "simulate_many() expects 4 arguments (forks, schedule, ticks, seed), got {}",
                args.len()
            ));
        }
        let bases: Vec<crate::world::WorldSnapshot> = {
            let list = args[0].as_list().ok_or_else(|| {
                "simulate_many() first argument must be a list of world_fork".to_string()
            })?;
            list.iter()
                .enumerate()
                .map(|(i, v)| {
                    v.as_world_fork()
                        .map(|f| f.as_ref().clone())
                        .ok_or_else(|| {
                            format!(
                                "simulate_many() element {} must be a world_fork, got {}",
                                i,
                                v.type_name()
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let system_names = Self::schedule_from_value(&args[1], "simulate_many")?;
        let ticks = args[2].as_int().ok_or_else(|| {
            "simulate_many() third argument (ticks) must be an integer".to_string()
        })?;
        if ticks < 0 {
            return Err("simulate_many() tick count must be non-negative".to_string());
        }
        let seed = args[3].as_int().ok_or_else(|| {
            "simulate_many() fourth argument (seed) must be an integer".to_string()
        })? as u64;

        for name in &system_names {
            if !self.systems.contains_key(name) {
                return Err(format!("simulate_many(): unknown system '{}'", name));
            }
        }

        let shared = self.shared_state();
        let bases_ref = &bases;
        let run_fork = |i: usize| {
            super::exec::with_worker_vm(&shared, |worker| {
                let base = bases_ref[i].clone();
                worker.restore_events_from(&base);
                worker.get_world_mut().restore(base);
                worker.set_random_seed(crate::sandbox::fork_seed(seed, i as u64));
                let was_worker = worker.is_worker;
                worker.is_worker = false;
                worker.in_simulation_fork += 1;

                let sim_result = (|| -> Result<(), String> {
                    for _ in 0..ticks {
                        for name in &system_names {
                            worker.run_system_by_name(name)?;
                        }
                        worker.bi_flush_events(vec![])?;
                    }
                    Ok(())
                })();

                worker.in_simulation_fork -= 1;
                worker.is_worker = was_worker;

                let snap = worker.snapshot_with_events();
                *worker.get_world_mut() = crate::world::World::new();
                worker.events_current.clear();
                worker.events_next.clear();
                worker.emit_ids_current.clear();
                worker.emit_ids_next.clear();
                worker.delayed_events.clear();
                sim_result.map(|_| snap)
            })
        };
        #[cfg(target_arch = "wasm32")]
        let snapshots: Vec<Result<crate::world::WorldSnapshot, String>> =
            (0..bases.len()).map(run_fork).collect();
        #[cfg(not(target_arch = "wasm32"))]
        let snapshots: Vec<Result<crate::world::WorldSnapshot, String>> = {
            use rayon::prelude::*;
            (0..bases.len()).into_par_iter().map(run_fork).collect()
        };

        let mut forks = Vec::with_capacity(snapshots.len());
        for (i, snap) in snapshots.into_iter().enumerate() {
            let mut snap = snap.map_err(|e| format!("simulate_many() fork {}: {}", i, e))?;
            snap.rollout_seed = Some(crate::sandbox::fork_seed(seed, i as u64));
            forks.push(Value::world_fork(&mut self.gc, std::sync::Arc::new(snap)));
        }
        Ok(Value::list(&mut self.gc, forks))
    }

    /// `fork_with(fork, resource_value) -> world_fork`
    ///
    /// Returns a copy of `fork` with one resource overridden — a speculative
    /// candidate seeded WITHOUT `commit()`ing to the live world (dogfood
    /// feature seq 150). `resource_value` is a resource/component instance
    /// (e.g. `Policy { tax: 8 }`); its type name selects the resource. Events,
    /// timers, and entities ride through untouched, so the result composes
    /// straight into `simulate`/`simulate_par`/`simulate_many`.
    fn bi_fork_with(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "fork_with() expects 2 arguments (fork, resource value), got {}",
                args.len()
            ));
        }
        let base = args[0]
            .as_world_fork()
            .ok_or_else(|| "fork_with() first argument must be a world_fork".to_string())?;
        let data = args[1].as_component().ok_or_else(|| {
            format!(
                "fork_with() second argument must be a component/resource value, got {}",
                args[1].type_name()
            )
        })?;
        let name = data.type_name.clone();
        let new_snap = base.as_ref().with_resource(&name, data.clone());
        Ok(Value::world_fork(
            &mut self.gc,
            std::sync::Arc::new(new_snap),
        ))
    }

    /// `simulate_seeded(fork, schedule, ticks, raw_seed) -> world_fork`
    ///
    /// ONE rollout under an EXACT rng seed — no per-index derivation. This is
    /// the reproduction half of `fork_seed()` (dogfood feature seq 150): when
    /// rollout `i` of a `simulate_par`/`simulate_many` call is the outlier,
    /// `simulate_seeded(base, systems, ticks, fork_seed(outs[i]))` re-runs
    /// exactly that future in isolation, bit-identically, without paying for
    /// the other rollouts. Purity rules match `simulate_par` (rand_* allowed —
    /// the explicit seed keeps it deterministic).
    fn bi_simulate_seeded(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(format!(
                "simulate_seeded() expects 4 arguments (fork, systems, ticks, raw_seed), got {}",
                args.len()
            ));
        }
        let fork_snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "simulate_seeded() first argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        let system_names = Self::schedule_from_value(&args[1], "simulate_seeded")?;
        let ticks = args[2].as_int().ok_or_else(|| {
            "simulate_seeded() third argument (ticks) must be an integer".to_string()
        })?;
        if ticks < 0 {
            return Err("simulate_seeded() tick count must be non-negative".to_string());
        }
        let raw_seed = args[3].as_int().ok_or_else(|| {
            "simulate_seeded() fourth argument (raw_seed) must be an integer".to_string()
        })? as u64;

        for name in &system_names {
            if !self.systems.contains_key(name) {
                return Err(format!("simulate_seeded(): unknown system '{}'", name));
            }
        }

        let shared = self.shared_state();
        // Run the one rollout on a rayon POOL thread, never the caller (a
        // one-item par_iter would execute on the calling thread): the pooled
        // worker VM must live in a pool thread's TLS, which outlives any
        // caller. Parking a worker VM in a short-lived caller thread's TLS
        // (e.g. a test thread) wedges that thread's teardown when the TLS
        // destructor tears the whole VM down.
        let run_rollout = move || {
            super::exec::with_worker_vm(&shared, |worker| {
                worker.restore_events_from(&fork_snap);
                worker.get_world_mut().restore(fork_snap.clone());
                // The seed is used AS GIVEN — that is the whole point.
                worker.set_random_seed(raw_seed);
                let was_worker = worker.is_worker;
                worker.is_worker = false;
                worker.in_simulation_fork += 1;

                let sim_result = (|| -> Result<(), String> {
                    for _ in 0..ticks {
                        for name in &system_names {
                            worker.run_system_by_name(name)?;
                        }
                        worker.bi_flush_events(vec![])?;
                    }
                    Ok(())
                })();

                worker.in_simulation_fork -= 1;
                worker.is_worker = was_worker;

                let snap = worker.snapshot_with_events();
                *worker.get_world_mut() = crate::world::World::new();
                worker.events_current.clear();
                worker.events_next.clear();
                worker.emit_ids_current.clear();
                worker.emit_ids_next.clear();
                worker.delayed_events.clear();
                sim_result.map(|_| snap)
            })
        };
        // wasm32 has no threads: run on the (only) thread, like simulate_par.
        #[cfg(target_arch = "wasm32")]
        let snap = run_rollout();
        #[cfg(not(target_arch = "wasm32"))]
        let snap = {
            let (tx, rx) = std::sync::mpsc::channel();
            rayon::spawn(move || {
                let _ = tx.send(run_rollout());
            });
            rx.recv()
                .map_err(|_| "simulate_seeded(): rollout worker disappeared".to_string())?
        };
        let mut snap = snap.map_err(|e| format!("simulate_seeded(): {}", e))?;
        snap.rollout_seed = Some(raw_seed);
        Ok(Value::world_fork(&mut self.gc, std::sync::Arc::new(snap)))
    }

    /// `fork_seed(fork) -> int`
    ///
    /// The effective rng seed the simulate-family rollout that produced this
    /// fork ran under, or 0 for any other fork (`fork()`, `fork_with`,
    /// `merge_forks`, wire decodes — the seed is local debug metadata and is
    /// deliberately not serialized). Derived seeds are never 0 (the SplitMix64
    /// finalizer clamps 0 to a sentinel), so 0 is unambiguous.
    fn bi_fork_seed(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "fork_seed() expects 1 argument, got {}",
                args.len()
            ));
        }
        let fork = args[0]
            .as_world_fork()
            .ok_or_else(|| "fork_seed() argument must be a world_fork".to_string())?;
        let seed = fork.as_ref().rollout_seed.unwrap_or(0);
        Ok(Value::from_int(&mut self.gc, seed as i64))
    }

    /// Compile untrusted RAD source for sandboxed execution.
    ///
    /// Module imports are rejected outright: resolving them would touch the
    /// filesystem, which a sandboxed guest must never do.
    fn compile_sandbox_source(source: &str) -> Result<crate::compiler::CompileResult, String> {
        let mut lexer = crate::lexer::Lexer::new(source);
        let (tokens, lex_errors) = lexer.tokenize();

        let mut parser = crate::parser::Parser::new(tokens);
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
            .any(|d| matches!(d, crate::ast::Decl::Use(_)))
        {
            all_errors
                .push("sandbox: module imports are not permitted in sandboxed code".to_string());
        }

        let mut checker = crate::checker::Checker::new();
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

        crate::compiler::Compiler::new()
            .with_checker_output(checker_output)
            .compile(&program)
            .map_err(|e| format!("Compile error: {}", e.message))
    }

    /// `sandbox_run(source, fork, caps_json) -> Result`
    ///
    /// Compiles and runs untrusted RAD source against a forked world inside a
    /// fresh, capability-bounded guest VM. The guest never sees the live
    /// world; it gets a copy-on-write fork and must return it for the host to
    /// inspect (`peek`/`diff`) and optionally `commit`.
    ///
    /// Enforcement layers:
    /// 1. builtin mask — IO/network/clock/speculation builtins are denied
    ///    (`call_builtin` checks `sandbox_caps`),
    /// 2. component-write ACL — `set`/`spawn`/`despawn`/system writebacks are
    ///    checked against the caps allowlist,
    /// 3. budgets — fuel and memory limits bound all execution.
    ///
    /// Unlike `simulate()`, events emitted by the guest are *not* dropped:
    /// the guest VM owns private double-buffered queues, so its handlers run
    /// normally inside the closed world (captured-events mode).
    ///
    /// Failure of any kind — malformed capability grant, guest compile
    /// error, runtime error, budget exhaustion, capability denial — returns
    /// `Err(message)` rather than aborting the host.
    fn bi_sandbox_run(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 && args.len() != 4 {
            return Err(format!(
                "sandbox_run() expects 3 or 4 arguments, got {}",
                args.len()
            ));
        }
        let source = args[0]
            .as_str()
            .ok_or_else(|| "sandbox_run() first argument must be a source string".to_string())?
            .to_string();
        let fork_snap = args[1]
            .as_world_fork()
            .ok_or_else(|| "sandbox_run() second argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        let caps_text = args[2]
            .as_str()
            .ok_or_else(|| "sandbox_run() third argument must be a caps JSON string".to_string())?;
        // A malformed grant comes back through the Err arm like every other
        // sandbox_run failure (dogfood bug seq 59). "Malformed caps are a
        // host-side bug: hard error" held only while caps were literals in
        // host source; a real plugin host computes the grant from the
        // plugin's own manifest, so a bad manifest ("fuel": -1) is
        // attacker-influenced input and must not take the host down before
        // the ACL or budgets ever run. The parser's messages stay strict and
        // specific — only the failure mode changes.
        let (caps, seed) = match crate::sandbox::SandboxCaps::from_json(caps_text) {
            Ok(parsed) => parsed,
            Err(e) => {
                let v = Value::from_string(&mut self.gc, e);
                return Ok(self.make_result(false, v));
            }
        };

        // Optional 4th argument: data-only input for the guest's
        // sandbox_input(). Serialized to JSON immediately — values are parsed
        // onto the guest heap, never shared. Replaces the anti-pattern of
        // splicing host data into guest source text.
        let input_json = match args.get(3) {
            Some(v) => Some(
                value_to_json(v, 0)
                    .map_err(|e| format!("sandbox_run() input is not data-only: {}", e))?
                    .to_string(),
            ),
            None => None,
        };

        let outcome = Self::run_sandbox_guest(
            &source,
            fork_snap,
            caps,
            seed,
            input_json,
            self.component_field_types.clone(),
        );

        // Retain the guest's structured output and fuel spend for
        // `sandbox_last_output()` / `sandbox_last_fuel()` (dogfood feature
        // seq 62): both were computed and discarded before, leaving a Rad
        // host with only Result<world_fork, str>. Recorded on every run,
        // including failures — a partial run still spends fuel, and the fuel
        // number is exactly the telemetry a plugin host meters on.
        self.last_sandbox_output_json = outcome.output_json.clone();
        self.last_sandbox_fuel_spent = outcome.fuel_spent;

        // Surface buffered guest output to the host, tagged for provenance.
        for line in outcome.prints {
            let tagged = format!("[sandbox] {}", line);
            if self.suppress_output {
                self.print_buffer.push(tagged);
            } else {
                println!("{}", tagged);
            }
        }

        match outcome.result {
            Ok(snap) => {
                let v = Value::world_fork(&mut self.gc, std::sync::Arc::new(snap));
                Ok(self.make_result(true, v))
            }
            Err(e) => {
                let v = Value::from_string(&mut self.gc, e);
                Ok(self.make_result(false, v))
            }
        }
    }

    /// Run untrusted source in a capability-bounded guest VM against a world
    /// snapshot. This is the shared core behind the `sandbox_run` builtin and
    /// the `rad sandbox serve` JSON-RPC protocol.
    ///
    /// `input_json` crosses the data-only boundary: the guest reads it via
    /// `sandbox_input()` (parsed onto the guest's own heap) and reports
    /// structured results via `sandbox_output(v)` (serialized to JSON before
    /// the guest VM is dropped). No heap values ever cross between VMs.
    pub fn run_sandbox_guest(
        source: &str,
        base: crate::world::WorldSnapshot,
        caps: crate::sandbox::SandboxCaps,
        seed: u64,
        input_json: Option<String>,
        host_field_types: crate::vm::ComponentFieldTypes,
    ) -> crate::sandbox::SandboxOutcome {
        let initial_fuel = caps.fuel;
        let compile_result = match Self::compile_sandbox_source(source) {
            Ok(r) => r,
            Err(msg) => {
                return crate::sandbox::SandboxOutcome {
                    result: Err(msg),
                    prints: Vec::new(),
                    fuel_spent: 0,
                    output_json: None,
                }
            }
        };

        let mem_limit = caps.mem_limit;
        let mut guest = VM::new();
        guest.suppress_output();
        guest.load_compile_result(compile_result);
        // The write-shape ACL binds guest writes to the HOST's declared
        // schema, so the guest must be validated against the host's field
        // types, not its own (a malicious guest declares the wrong shape on
        // purpose). Overlay the host schema onto the guest's: host-declared
        // components become authoritative; guest-only components keep theirs.
        if !host_field_types.is_empty() {
            let ft = std::sync::Arc::make_mut(&mut guest.component_field_types);
            for (name, fields) in host_field_types.iter() {
                ft.insert(name.clone(), fields.clone());
            }
        }
        // The fork's pending events are part of the state handed to the
        // guest. `guest.run` resets the queues, so they are spliced in after
        // the main chunk, ahead of the guest's own emissions (FIFO from the
        // forked timeline), and drained below.
        let inherited_events: Vec<(String, Value, u64)> = base.events.as_ref().clone();
        let inherited_emit_ids: Vec<u64> = base.emit_ids.as_ref().clone();
        guest.get_world_mut().restore(base);
        guest.sandbox_caps = Some(std::sync::Arc::new(caps));
        guest.fuel = initial_fuel;
        guest.mem_limit = mem_limit;
        guest.set_random_seed(seed);
        guest.sandbox_input_json = input_json;

        let run_result = guest.run(0).and_then(|_| {
            if !inherited_events.is_empty() {
                let mut q = inherited_events;
                q.extend(std::mem::take(&mut guest.events_next));
                guest.events_next = q;
                let mut ids = inherited_emit_ids;
                ids.extend(std::mem::take(&mut guest.emit_ids_next));
                guest.emit_ids_next = ids;
            }
            // Drain any events still in flight after the guest's main chunk so
            // emitted events take effect inside the closed world.
            //
            // Every generation is charged. This loop is Rust, not bytecode, and
            // a handler body carrying no call and no loop back-edge crosses no
            // charge point of its own (`emit` is a single opcode), so a
            // self-re-emitting handler would otherwise drain forever — unmetered
            // by fuel, and unmetered by `mem_bytes` too, since the allocation
            // ceiling is only enforced inside `charge_fuel`.
            while !guest.events_next.is_empty() {
                guest.charge_fuel()?;
                guest.bi_flush_events(vec![])?;
            }
            Ok(())
        });

        crate::sandbox::SandboxOutcome {
            result: run_result.map(|_| guest.snapshot_with_events()),
            prints: std::mem::take(&mut guest.print_buffer),
            fuel_spent: initial_fuel.saturating_sub(guest.fuel),
            output_json: guest.sandbox_output_json.take(),
        }
    }

    /// `fork_to_bytes(fork) -> str` — the fork wire codec, encode half.
    /// Serializes a fork's **full program state** — entities (with their
    /// runtime ids and the id-allocator), names, components, resources,
    /// in-flight events with causality ids, and the schema — as canonical
    /// JSON with an integrity digest. Deterministic: the same fork encodes
    /// to the same bytes on every machine.
    ///
    /// Format: `RADFORK2 <blake3-hex> <body-json>`, written directly into a
    /// string buffer (no intermediate JSON tree) with the compact value
    /// codec in `crate::wire` — measured ~30x faster and ~4x smaller than
    /// the v1 tagged-tree format.
    pub(crate) fn bi_fork_to_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        use std::fmt::Write as _;
        if args.len() != 1 {
            return Err(format!(
                "fork_to_bytes() expects 1 argument, got {}",
                args.len()
            ));
        }
        let snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "fork_to_bytes() argument must be a world_fork".to_string())?
            .as_ref()
            .clone();

        let mut w = crate::world::World::new();
        let events = snap.events.clone();
        let emit_ids = snap.emit_ids.clone();
        let delayed = snap.delayed.clone();
        let provenance = snap.provenance.clone();
        w.restore(snap);

        // First occurrence of a type pins its wire layout; later instances
        // (which can only differ in order, not field set) remap into it.
        let mut schema: std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>> =
            std::collections::BTreeMap::new();
        fn write_data(
            schema: &mut std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>>,
            data: &crate::value::ComponentData,
            out: &mut String,
        ) -> Result<(), String> {
            let wire_layout = schema
                .entry(data.type_name.clone())
                .or_insert_with(|| data.layout.clone())
                .clone();
            crate::wire::escape_json_into(out, &data.type_name);
            out.push_str(",[");
            let aligned =
                std::sync::Arc::ptr_eq(&wire_layout, &data.layout) || *wire_layout == *data.layout;
            for i in 0..wire_layout.len() {
                if i > 0 {
                    out.push(',');
                }
                let v = if aligned {
                    &data.values[i]
                } else {
                    let f = &wire_layout[i];
                    let pos = data.layout.iter().position(|n| n == f).ok_or_else(|| {
                        format!(
                            "fork_to_bytes: instances of '{}' disagree on field '{}'",
                            data.type_name, f
                        )
                    })?;
                    &data.values[pos]
                };
                crate::wire::encode_value_into(v, out)?;
            }
            out.push(']');
            Ok(())
        }

        let mut body = String::with_capacity(64 * 1024);
        body.push_str("{\"entities\":[");
        for (i, eid) in w.all_entity_ids().into_iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "[{},", eid);
            match w.entity_name(eid) {
                Some(name) => crate::wire::escape_json_into(&mut body, &name),
                None => body.push_str("null"),
            }
            body.push_str(",[");
            let mut comps = w.components_on_entity(eid);
            comps.sort_by(|a, b| a.type_name.cmp(&b.type_name));
            for (j, data) in comps.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                body.push('[');
                write_data(&mut schema, data, &mut body)?;
                body.push(']');
            }
            body.push_str("]]");
        }

        body.push_str("],\"events\":[");
        for (i, (name, payload, tid)) in events.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, name);
            // Emit ids cross into the foreign namespace here: the receiver's
            // ledger must never confuse them with its own sequential ids.
            let _ = write!(
                body,
                ",{},{},",
                tid,
                crate::causality::foreign_emit_id(emit_ids.get(i).copied().unwrap_or(0))
            );
            crate::wire::encode_value_into(payload, &mut body)?;
            body.push(']');
        }

        body.push_str("],\"delayed\":[");
        for (i, (left, name, payload, emit_id)) in delayed.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            let _ = write!(body, "{},", left);
            crate::wire::escape_json_into(&mut body, name);
            let _ = write!(body, ",{},", crate::causality::foreign_emit_id(*emit_id));
            crate::wire::encode_value_into(payload, &mut body)?;
            body.push(']');
        }

        body.push_str("],\"resources\":[");

        let mut rnames = w.resource_names();
        rnames.sort();
        for (i, rname) in rnames.iter().enumerate() {
            if let Some(data) = w.get_resource(rname) {
                if i > 0 {
                    body.push(',');
                }
                body.push('[');
                write_data(&mut schema, &data, &mut body)?;
                body.push(']');
            }
        }

        body.push_str("],\"schema\":[");
        for (i, (tname, layout)) in schema.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, tname);
            body.push_str(",[");
            for (j, f) in layout.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                crate::wire::escape_json_into(&mut body, f);
            }
            body.push_str("]]");
        }

        // Provenance rides last (tools that only want state can stop at it).
        // A decoded fork re-encodes its carried records verbatim — that is
        // what keeps re-encoding byte-identical across machines. A local
        // fork ships this VM's ledger closure for everything alive in it.
        body.push(']');
        Self::append_authoritative_world_transport(&w, &mut body)?;
        body.push_str(",\"prov\":");
        match &provenance {
            Some(p) => crate::wire::encode_prov_into(p, &mut body),
            None => {
                let resource_names: std::collections::HashSet<String> =
                    w.resource_names().into_iter().collect();
                let closure = self.ledger.provenance_closure(
                    |rec| match rec.entity {
                        Some(eid) => w.contains_entity(eid),
                        None => resource_names.contains(&rec.component),
                    },
                    |record| {
                        w.relation_state()
                            .assertions()
                            .get(&record.fact_key)
                            .is_some_and(|assertion| {
                                assertion.assertion_id == record.assertion_id
                            })
                    },
                    &emit_ids
                        .iter()
                        .copied()
                        .chain(delayed.iter().map(|(_, _, _, id)| *id))
                        .collect::<Vec<_>>(),
                );
                crate::wire::encode_prov_into(&closure, &mut body);
            }
        }
        body.push('}');

        // Large bodies use the compressed RADPACK representation; small
        // bodies use the plain current envelope. Both name the same world.
        let out = crate::radpack::seal("RADFORK2", &body);
        Ok(Value::from_string(&mut self.gc, out))
    }

    /// `fork_from_bytes(str) -> Result<world_fork, str>` — decode half.
    /// Verifies the integrity digest, reconstructs the world **id-faithfully**
    /// (entity ids, names, allocator state), revives in-flight events, and
    /// validates every component/resource against the local schema — running
    /// declared `migrate` blocks on shape drift, exactly like `load_world`.
    /// Malformed or mismatched bytes are an `Err`, not a crash: network input
    /// is a system boundary.
    pub(crate) fn bi_fork_from_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "fork_from_bytes() expects 1 argument, got {}",
                args.len()
            ));
        }
        let text = args[0]
            .as_str()
            .ok_or_else(|| format!("fork_from_bytes() expects str, got {}", args[0].type_name()))?
            .to_string();

        match self.decode_fork_wire(&text) {
            Ok(snap) => {
                let v = Value::world_fork(&mut self.gc, std::sync::Arc::new(snap));
                Ok(self.make_result(true, v))
            }
            Err(msg) => {
                let e = Value::from_string(&mut self.gc, msg);
                Ok(self.make_result(false, e))
            }
        }
    }}
