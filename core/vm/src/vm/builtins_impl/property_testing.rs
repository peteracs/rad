#[derive(Clone, Copy)]
struct ModelTrialSpec<'a> {
    base: &'a crate::world::WorldSnapshot,
    commands: &'a [Value],
    invariants: &'a [Value],
    temporal: &'a [ModelTemporal],
    observation_plan: &'a ModelObservationPlan,
}

struct ModelFailureContext<'a> {
    model: &'a str,
    campaign_seed: u64,
    run_seed: u64,
    run_index: u32,
    histories_requested: u32,
    commands_executed: u64,
    max_commands: u32,
    history_ns: &'a [u64],
    trial: ModelTrialSpec<'a>,
    trace: &'a [ModelTraceStep],
}

impl VM {
    fn run_model_trial(
        &mut self,
        spec: ModelTrialSpec<'_>,
        labels: Option<&[String]>,
        trace: &[ModelTraceStep],
        seed: u64,
    ) -> Result<(), String> {
        let native_plan = self.prepare_nested_native_model_trial(spec.base, trace, seed)?;
        let mut worker = VM::from_shared_state(self.shared_state());
        worker.nested_native_tape = native_plan.tape_for_lane(0);
        worker.indexed_decl = Arc::clone(&self.indexed_decl);
        worker.ordered_decl = Arc::clone(&self.ordered_decl);
        worker.migrations = self.migrations.clone();
        worker.restore_events_from(spec.base);
        worker.world.restore(spec.base.clone());
        worker.is_worker = false;
        worker.in_simulation_fork = 1;
        // Model trials are private simulation forks, but temporal clauses must
        // observe events emitted by that trial. This enables only the worker's
        // bounded observation log; it does not re-enable main-timeline causal
        // recording or effects forbidden in simulation.
        worker.capture_isolated_event_log = true;
        worker.suppress_output = true;
        worker.set_random_seed(seed);

        let execution = Self::execute_model_trial(
            &mut worker,
            spec.commands,
            labels,
            spec.invariants,
            spec.temporal,
            spec.observation_plan,
            trace,
        );
        let native_calls = worker.finish_worker_native_tape()?;
        if native_plan.active() {
            self.finish_nested_native(native_plan, &[native_calls])?;
        }
        execution
    }

    fn execute_model_trial(
        worker: &mut VM,
        commands: &[Value],
        labels: Option<&[String]>,
        invariants: &[Value],
        temporal: &[ModelTemporal],
        observation_plan: &ModelObservationPlan,
        trace: &[ModelTraceStep],
    ) -> Result<(), String> {
        let observation_capacity = trace.len().saturating_add(1);
        let mut signals = observation_plan
            .names
            .iter()
            .map(|_| Vec::with_capacity(observation_capacity))
            .collect::<Vec<_>>();
        let mut occurrences = vec![0usize; observation_plan.names.len()];
        let mut previous_membership = vec![false; observation_plan.names.len()];
        let mut event_counts = vec![0usize; observation_plan.names.len()];
        let mut prior_event_count = 0usize;
        let mut invariant_dependencies = ModelAccessSet::default();

        let outcome = (|| {
            Self::model_invariants_hold_traced(
                worker,
                invariants,
                ModelInvariantPoint::Initial,
                &mut invariant_dependencies,
                true,
            )?;
            Self::model_sample(
                worker,
                observation_plan,
                &mut prior_event_count,
                &mut signals,
                &mut occurrences,
                &mut previous_membership,
                &mut event_counts,
            );
            for (step_index, step) in trace.iter().enumerate() {
                let step_number = step_index + 1;
                let label = if step.label.is_empty() {
                    labels
                        .and_then(|labels| labels.get(step.command))
                        .map(String::as_str)
                        .ok_or_else(|| {
                            format!("trace selects missing command label index {}", step.command)
                        })?
                } else {
                    step.label.as_str()
                };
                let mut temporal_dirty = false;
                if step.flush_before {
                    Self::begin_model_access_trace(worker);
                    let flush = worker.bi_flush_events(Vec::new()).map_err(|error| {
                        format!(
                            "flush before step {} ({}) failed: {error}",
                            step_number, label
                        )
                    });
                    let (observation_changed, invariant_changed) =
                        Self::finish_model_access_trace(worker, |trace| {
                            (
                                trace.writes.intersects(&observation_plan.dependencies),
                                trace.writes.intersects(&invariant_dependencies),
                            )
                        });
                    flush?;
                    temporal_dirty |= observation_changed;
                    Self::model_invariants_hold_traced(
                        worker,
                        invariants,
                        ModelInvariantPoint::FlushBefore(step_number),
                        &mut invariant_dependencies,
                        invariant_changed,
                    )?;
                }
                let command = commands.get(step.command).ok_or_else(|| {
                    format!("trace selects missing command index {}", step.command)
                })?;
                Self::begin_model_access_trace(worker);
                let command_result = worker.call_value(command, Vec::new()).map_err(|error| {
                    format!(
                        "command '{}' failed at step {}: {error}",
                        label, step_number
                    )
                });
                let (observation_changed, invariant_changed) =
                    Self::finish_model_access_trace(worker, |trace| {
                        (
                            trace.writes.intersects(&observation_plan.dependencies),
                            trace.writes.intersects(&invariant_dependencies),
                        )
                    });
                command_result?;
                temporal_dirty |= observation_changed;
                Self::model_invariants_hold_traced(
                    worker,
                    invariants,
                    ModelInvariantPoint::Command {
                        label,
                        step: step_number,
                    },
                    &mut invariant_dependencies,
                    invariant_changed,
                )?;
                if step.flush_after {
                    Self::begin_model_access_trace(worker);
                    let flush = worker.bi_flush_events(Vec::new()).map_err(|error| {
                        format!(
                            "flush after step {} ({}) failed: {error}",
                            step_number, label
                        )
                    });
                    let (observation_changed, invariant_changed) =
                        Self::finish_model_access_trace(worker, |trace| {
                            (
                                trace.writes.intersects(&observation_plan.dependencies),
                                trace.writes.intersects(&invariant_dependencies),
                            )
                        });
                    flush?;
                    temporal_dirty |= observation_changed;
                    Self::model_invariants_hold_traced(
                        worker,
                        invariants,
                        ModelInvariantPoint::FlushAfter(step_number),
                        &mut invariant_dependencies,
                        invariant_changed,
                    )?;
                }
                if worker.event_log.len() != prior_event_count
                    || temporal_dirty
                {
                    Self::model_sample(
                        worker,
                        observation_plan,
                        &mut prior_event_count,
                        &mut signals,
                        &mut occurrences,
                        &mut previous_membership,
                        &mut event_counts,
                    );
                } else {
                    Self::model_repeat_unchanged_sample(&mut signals, &previous_membership);
                }
            }
            Self::model_temporal_holds(temporal, observation_plan, &signals, &occurrences)
        })();
        worker.model_access_trace = None;
        outcome
    }

    /// Return a pooled simulation VM to the exact observable state of one
    /// freshly constructed model-trial VM.
    ///
    /// Generated campaigns can contain tens of thousands of independent
    /// histories. Constructing a coordinator and trial VM per history made VM
    /// setup—not the model—dominate the campaign. The reset is intentionally
    /// exhaustive: no event, causal record, task, effect region, random state,
    /// or diagnostic counter may cross a history boundary.
    #[cfg(not(target_arch = "wasm32"))]
    fn reset_pooled_model_trial(
        worker: &mut VM,
        shared: &crate::vm::VmSharedState,
        base: &crate::world::WorldSnapshot,
        seed: u64,
    ) {
        worker.abort_effect_regions();
        worker.sync_from_shared(shared);
        worker.world.restore(base.clone());
        worker.restore_events_from(base);
        worker.events_processing.clear();
        worker.print_buffer.clear();
        worker.eprint_buffer.clear();
        worker.timeline.clear();
        worker.event_log.clear();
        worker.trace_patch = None;
        worker.trace_timeline = false;
        worker.model_check_reports.clear();
        worker.model_access_trace = None;
        worker.metered_instruction_count = 0;
        worker.current_trace_id = None;
        worker.next_trace_id = 1;
        worker.next_frame_id = 1;
        worker.next_settlement_id = 1;
        worker.tasks.clear();
        worker.next_task_id = 1;
        worker.pending_io.clear();
        worker.in_async_context = false;
        worker.once_guard_passed = false;
        worker.current_cause = crate::causality::Cause::Main;
        worker.pending_host_cause = None;
        worker.causality_frame = 0;
        worker.ledger = crate::causality::CausalityLedger::default();
        worker.nested_native_tape = None;
        worker.observational_attempt_replay = false;
        worker.command_buffer.clear();
        worker.arena.reset();
        if let Some(metrics) = worker.system_metrics.as_mut() {
            metrics.clear();
        }
        worker.is_worker = false;
        worker.in_simulation_fork = 1;
        worker.capture_isolated_event_log = true;
        worker.suppress_output = true;
        worker.set_random_seed(seed);
    }

    /// Stable property identity used by the shrinker. A smaller trace is a
    /// valid counterexample only when it fails the same command, invariant,
    /// flush boundary, or temporal clause as the generated history.
    fn model_failure_identity(reason: &str) -> String {
        let first_line = reason.lines().next().unwrap_or(reason);
        if let Some(command) = first_line.strip_prefix("command '") {
            if let Some((label, failure)) = command.split_once("' failed at step ") {
                let detail = failure
                    .split_once(": ")
                    .map_or("", |(_, detail)| detail);
                return format!("command|{label}|{detail}");
            }
        }
        if first_line.starts_with("invariant ") {
            return first_line
                .split_once(" after ")
                .map_or_else(|| first_line.to_string(), |(identity, _)| identity.to_string());
        }
        if first_line.starts_with("flush before step ")
            || first_line.starts_with("flush after step ")
        {
            let label = first_line
                .split_once('(')
                .and_then(|(_, rest)| rest.split_once(')'))
                .map_or("<unknown>", |(label, _)| label);
            let detail = first_line
                .split_once(" failed: ")
                .map_or("", |(_, detail)| detail);
            let boundary = if first_line.starts_with("flush before") {
                "before"
            } else {
                "after"
            };
            return format!("flush|{boundary}|{label}|{detail}");
        }
        // Temporal diagnostics contain the complete clause in backticks.
        // Unknown failures retain their complete first line and therefore
        // also fail closed against property drift.
        first_line.to_string()
    }

    fn shrink_model_trace(
        &mut self,
        spec: ModelTrialSpec<'_>,
        original: &[ModelTraceStep],
        seed: u64,
    ) -> (Vec<ModelTraceStep>, String) {
        let mut trace = original.to_vec();
        let mut reason = self
            .run_model_trial(spec, None, &trace, seed)
            .expect_err("shrink is entered only for a failing trace");
        let failure_identity = Self::model_failure_identity(&reason);
        let mut partitions = 2usize;
        while !trace.is_empty() {
            let chunk = trace.len().div_ceil(partitions);
            let mut reduced = false;
            let mut start = 0usize;
            while start < trace.len() {
                let end = (start + chunk).min(trace.len());
                let mut candidate = Vec::with_capacity(trace.len() - (end - start));
                candidate.extend_from_slice(&trace[..start]);
                candidate.extend_from_slice(&trace[end..]);
                if let Err(candidate_reason) = self.run_model_trial(spec, None, &candidate, seed) {
                    if Self::model_failure_identity(&candidate_reason) == failure_identity {
                        trace = candidate;
                        reason = candidate_reason;
                        partitions = partitions.saturating_sub(1).max(2);
                        reduced = true;
                        break;
                    }
                }
                start = end;
            }
            if !reduced {
                if partitions >= trace.len() {
                    break;
                }
                partitions = (partitions * 2).min(trace.len());
            }
        }
        (trace, reason)
    }

    #[inline]
    fn model_run_seed(campaign_seed: u64, run_index: u32) -> u64 {
        let mut value = campaign_seed
            .wrapping_add(u64::from(run_index).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn generate_model_trace(
        command_count: usize,
        max_commands: u32,
        run_seed: u64,
    ) -> Vec<ModelTraceStep> {
        let mut random = run_seed;
        let mut trace = Vec::with_capacity(max_commands as usize);
        for _ in 0..max_commands {
            random ^= random >> 12;
            random ^= random << 25;
            random ^= random >> 27;
            let selection = random.wrapping_mul(0x2545_F491_4F6C_DD1D);
            let command = selection as usize % command_count;
            trace.push(ModelTraceStep {
                command,
                // Generated campaigns keep labels in one immutable table.
                // Materialize them only if a failure must become a replayable
                // artifact; copying one String per successful command made
                // large campaigns allocate millions of short-lived objects.
                label: String::new(),
                flush_before: selection & (1 << 8) != 0,
                flush_after: selection & (1 << 17) != 0,
            });
        }
        trace
    }

    fn materialize_model_trace_labels(
        trace: &mut [ModelTraceStep],
        labels: &[String],
    ) -> Result<(), String> {
        for step in trace {
            if step.label.is_empty() {
                step.label = labels
                    .get(step.command)
                    .cloned()
                    .ok_or_else(|| {
                        format!("trace selects missing command label index {}", step.command)
                    })?;
            }
        }
        Ok(())
    }

    fn report_model_failure(
        &mut self,
        context: ModelFailureContext<'_>,
    ) -> Result<Value, String> {
        let ModelFailureContext {
            model,
            campaign_seed,
            run_seed,
            run_index,
            histories_requested,
            commands_executed,
            max_commands,
            history_ns,
            trial,
            trace,
        } = context;
        let shrink_started = std::time::Instant::now();
        let (minimal, reason) = self.shrink_model_trace(trial, trace, run_seed);
        let shrink_ns = shrink_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let (median_history_ns, p95_history_ns) = Self::model_history_percentiles(history_ns);
        self.model_check_reports.push(ModelCheckReport {
            model: model.to_string(),
            seed: campaign_seed,
            histories_requested,
            histories_executed: run_index + 1,
            commands_executed,
            counterexamples: 1,
            minimum_trace_length: Some(minimal.len()),
            shrink_ns,
            median_history_ns,
            p95_history_ns,
        });
        let marker = serde_json::json!({
            "kind": "rad_model_failure_v1",
            "model": model,
            "campaign_seed": campaign_seed,
            "seed": run_seed,
            "run_index": run_index,
            "histories_requested": histories_requested,
            "max_commands": max_commands,
            "generated_trace": trace,
            "minimal_trace": minimal,
            "reason": reason,
        });
        Err(format!(
            "Stateful model '{}' failed in history {} after shrinking {} generated command(s) to {}:\n{}\nRAD_MODEL_FAILURE={}",
            model,
            run_index + 1,
            marker["generated_trace"].as_array().map_or(0, Vec::len),
            marker["minimal_trace"].as_array().map_or(0, Vec::len),
            marker["reason"].as_str().unwrap_or("model failure"),
            marker
        ))
    }

    fn bi_model_check(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 8 {
            return Err(format!("model_check() expects 8 arguments, got {}", args.len()));
        }
        let model = args[0]
            .as_str()
            .ok_or_else(|| "model_check() model name must be a string".to_string())?
            .to_string();
        let commands = args[1]
            .as_list()
            .ok_or_else(|| "model_check() commands must be a list".to_string())?
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let labels = args[2]
            .as_list()
            .ok_or_else(|| "model_check() command labels must be a list".to_string())?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "model_check() command labels must be strings".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let invariants = args[3]
            .as_list()
            .ok_or_else(|| "model_check() invariants must be a list".to_string())?
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let temporal = args[4]
            .as_list()
            .ok_or_else(|| "model_check() temporal clauses must be a list".to_string())?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| "model_check() temporal clauses must be strings".to_string())
                    .and_then(ModelTemporal::decode)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let observation_plan = ModelObservationPlan::new(&temporal);
        let declared_runs = args[5]
            .as_int()
            .ok_or_else(|| "model_check() run count must be an integer".to_string())?;
        let declared_max_commands = args[6]
            .as_int()
            .ok_or_else(|| "model_check() command bound must be an integer".to_string())?;
        let raw_seed = args[7]
            .as_int()
            .ok_or_else(|| "model_check() seed must be an integer".to_string())?;
        if commands.is_empty()
            || commands.len() != labels.len()
            || declared_runs <= 0
            || declared_max_commands <= 0
        {
            return Err("model_check() received an invalid compiled model".to_string());
        }

        let config = self.model_check_config.clone();
        if config
            .model
            .as_deref()
            .is_some_and(|selected| selected != model)
        {
            return Ok(Value::NIL);
        }
        let declared_runs = u32::try_from(declared_runs)
            .map_err(|_| "model_check() run count exceeds u32".to_string())?;
        let declared_max_commands = u32::try_from(declared_max_commands)
            .map_err(|_| "model_check() command bound exceeds u32".to_string())?;
        let runs = config.runs.unwrap_or(declared_runs);
        let max_commands = config.max_commands.unwrap_or(declared_max_commands);
        if runs == 0 || max_commands == 0 {
            return Err("model_check() overrides must be positive".to_string());
        }
        let campaign_seed = config.seed.unwrap_or(raw_seed as u64);
        let campaign_seed = if campaign_seed == 0 {
            0xD1B5_4A32_D192_ED03
        } else {
            campaign_seed
        };
        let base = self.snapshot_with_events();
        let trial_spec = ModelTrialSpec {
            base: &base,
            commands: &commands,
            invariants: &invariants,
            temporal: &temporal,
            observation_plan: &observation_plan,
        };
        let exact_trace = config.trace;
        let histories_requested = if exact_trace.is_some() { 1 } else { runs };

        // Independent generated histories have no observable ordering: each
        // starts from `base`, owns its VM/world, suppresses output, and returns
        // only a result. Execute the complete campaign in parallel, then scan
        // outcomes in run-index order so the first counterexample, report
        // counts, seed, shrink input, and replay artifact remain deterministic.
        // Record/replay stays serial because its nested-native boundary is an
        // ordered external trace. Exact-trace replay is one history and gains
        // nothing from parallel setup.
        #[cfg(not(target_arch = "wasm32"))]
        if exact_trace.is_none()
            && self.recorder.is_none()
            && self.replayer.is_none()
            && histories_requested > 1
        {
            use rayon::prelude::*;

            let shared = self.shared_state();
            let indexed_decl = Arc::clone(&self.indexed_decl);
            let ordered_decl = Arc::clone(&self.ordered_decl);
            let migrations = self.migrations.clone();
            let outcomes = (0..histories_requested)
                .into_par_iter()
                .map_init(
                    || {
                        let mut worker = VM::from_shared_state(shared.clone());
                        worker.indexed_decl = Arc::clone(&indexed_decl);
                        worker.ordered_decl = Arc::clone(&ordered_decl);
                        worker.migrations = migrations.clone();
                        worker
                    },
                    |worker, run_index| {
                    let run_seed = Self::model_run_seed(campaign_seed, run_index);
                    let trace = Self::generate_model_trace(commands.len(), max_commands, run_seed);

                    let started = std::time::Instant::now();
                    Self::reset_pooled_model_trial(worker, &shared, &base, run_seed);
                    let result = Self::execute_model_trial(
                        worker,
                        &commands,
                        Some(&labels),
                        &invariants,
                        &temporal,
                        &observation_plan,
                        &trace,
                    );
                    let elapsed_ns = started
                        .elapsed()
                        .as_nanos()
                        .min(u128::from(u64::MAX)) as u64;
                    (run_seed, elapsed_ns, result)
                },
                )
                .collect::<Vec<_>>();

            let first_failure = outcomes.iter().position(|(_, _, result)| result.is_err());
            let histories_executed = first_failure.map_or(histories_requested, |index| {
                u32::try_from(index + 1).expect("history index is bounded by u32 run count")
            });
            let history_ns = outcomes
                .iter()
                .take(histories_executed as usize)
                .map(|(_, elapsed_ns, _)| *elapsed_ns)
                .collect::<Vec<_>>();
            let commands_executed =
                u64::from(histories_executed).saturating_mul(u64::from(max_commands));

            if let Some(failure_index) = first_failure {
                let run_index = u32::try_from(failure_index)
                    .expect("history index is bounded by u32 run count");
                let run_seed = outcomes[failure_index].0;
                let mut trace =
                    Self::generate_model_trace(commands.len(), max_commands, run_seed);
                Self::materialize_model_trace_labels(&mut trace, &labels)?;
                return self.report_model_failure(ModelFailureContext {
                    model: &model,
                    campaign_seed,
                    run_seed,
                    run_index,
                    histories_requested,
                    commands_executed,
                    max_commands,
                    history_ns: &history_ns,
                    trial: trial_spec,
                    trace: &trace,
                });
            }

            let (median_history_ns, p95_history_ns) =
                Self::model_history_percentiles(&history_ns);
            self.model_check_reports.push(ModelCheckReport {
                model,
                seed: campaign_seed,
                histories_requested,
                histories_executed,
                commands_executed,
                counterexamples: 0,
                minimum_trace_length: None,
                shrink_ns: 0,
                median_history_ns,
                p95_history_ns,
            });
            return Ok(Value::NIL);
        }

        let mut commands_executed = 0_u64;
        let mut history_ns = Vec::with_capacity(histories_requested as usize);

        for run_index in 0..histories_requested {
            let run_seed = exact_trace
                .as_ref()
                .map_or_else(|| Self::model_run_seed(campaign_seed, run_index), |_| campaign_seed);
            let mut generated_trace = false;
            let mut trace = if let Some(trace) = exact_trace.as_ref() {
                for step in trace {
                    let expected = labels.get(step.command).ok_or_else(|| {
                        format!(
                            "model replay selects missing command index {}",
                            step.command
                        )
                    })?;
                    if expected != &step.label {
                        return Err(format!(
                            "model replay command {} changed from '{}' to '{}'",
                            step.command, step.label, expected
                        ));
                    }
                }
                trace.clone()
            } else {
                generated_trace = true;
                Self::generate_model_trace(commands.len(), max_commands, run_seed)
            };
            commands_executed = commands_executed.saturating_add(trace.len() as u64);
            let history_started = std::time::Instant::now();
            let trial = self.run_model_trial(
                trial_spec,
                Some(&labels),
                &trace,
                run_seed,
            );
            history_ns.push(
                history_started
                    .elapsed()
                    .as_nanos()
                    .min(u128::from(u64::MAX)) as u64,
            );
            if trial.is_ok() {
                continue;
            }

            if generated_trace {
                Self::materialize_model_trace_labels(&mut trace, &labels)?;
            }

            return self.report_model_failure(ModelFailureContext {
                model: &model,
                campaign_seed,
                run_seed,
                run_index,
                histories_requested,
                commands_executed,
                max_commands,
                history_ns: &history_ns,
                trial: trial_spec,
                trace: &trace,
            });
        }

        let (median_history_ns, p95_history_ns) = Self::model_history_percentiles(&history_ns);
        self.model_check_reports.push(ModelCheckReport {
            model,
            seed: campaign_seed,
            histories_requested,
            histories_executed: histories_requested,
            commands_executed,
            counterexamples: 0,
            minimum_trace_length: None,
            shrink_ns: 0,
            median_history_ns,
            p95_history_ns,
        });
        Ok(Value::NIL)
    }
}
