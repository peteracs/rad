pub(crate) fn bi_gen_int(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    let mut out = Vec::with_capacity(100);
    out.push(Value::from_int(gc, 0));
    for k in 1..=48 {
        out.push(Value::from_int(gc, k));
        out.push(Value::from_int(gc, -k));
    }
    out.push(Value::from_int(gc, 49));
    out.push(Value::from_int(gc, i64::MAX / 2));
    out.push(Value::from_int(gc, i64::MIN / 2));
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_gen_float(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    let mut out = Vec::with_capacity(100);
    out.push(Value::from_float(0.0));
    for k in 1..=48 {
        out.push(Value::from_float(k as f64));
        out.push(Value::from_float(-(k as f64)));
    }
    out.push(Value::from_float(f64::INFINITY));
    out.push(Value::from_float(f64::NEG_INFINITY));
    out.push(Value::from_float(f64::NAN));
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_gen_str(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    let mut out = Vec::with_capacity(21);
    out.push(Value::from_string(gc, String::new()));
    for len in 1..=20 {
        out.push(Value::from_string(gc, "a".repeat(len)));
    }
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_gen_bool(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    Ok(Value::list(gc, vec![Value::TRUE, Value::FALSE]))
}

pub(crate) fn bi_gen_list(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("gen_list() requires 1 argument".into());
    }
    let items = args[0]
        .as_list()
        .ok_or_else(|| format!("gen_list() expects list, got {}", args[0].type_name()))?;
    let n = items.len();
    let mut out = Vec::with_capacity(n + 1);
    for end in 0..=n {
        out.push(Value::list(gc, items.slice(0, end)));
    }
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_assert(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("assert() requires 2 arguments".into());
    }
    if !args[0].is_truthy() {
        return Err(args[1].print_display());
    }
    Ok(Value::NIL)
}

pub(crate) fn bi_assert_eq(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("assert_eq() requires 2 arguments".into());
    }
    if args[0] != args[1] {
        return Err(format!(
            "assert_eq failed: {} != {}",
            args[0].print_display(),
            args[1].print_display()
        ));
    }
    Ok(Value::NIL)
}

#[derive(Clone, Debug)]
enum ModelTemporal {
    Always(String),
    Eventually(String),
    Until(String, String),
    LeadsTo(String, String),
    ExactlyOnce(String),
    NeverAfter(String, String),
    EventuallyWithin(String, String, usize),
}

impl ModelTemporal {
    fn decode(raw: &str) -> Result<Self, String> {
        let parts = raw.split('|').collect::<Vec<_>>();
        match parts.as_slice() {
            ["always", name] if !name.is_empty() => Ok(Self::Always((*name).to_string())),
            ["eventually", name] if !name.is_empty() => {
                Ok(Self::Eventually((*name).to_string()))
            }
            ["until", condition, terminal] if !condition.is_empty() && !terminal.is_empty() => {
                Ok(Self::Until((*condition).to_string(), (*terminal).to_string()))
            }
            ["leads_to", trigger, consequence]
                if !trigger.is_empty() && !consequence.is_empty() =>
            {
                Ok(Self::LeadsTo(
                    (*trigger).to_string(),
                    (*consequence).to_string(),
                ))
            }
            ["exactly_once", name] if !name.is_empty() => {
                Ok(Self::ExactlyOnce((*name).to_string()))
            }
            ["never_after", prohibited, terminal]
                if !prohibited.is_empty() && !terminal.is_empty() =>
            {
                Ok(Self::NeverAfter(
                    (*prohibited).to_string(),
                    (*terminal).to_string(),
                ))
            }
            ["eventually_within", trigger, consequence, bound]
                if !trigger.is_empty() && !consequence.is_empty() =>
            {
                let bound = bound.parse::<usize>().map_err(|_| {
                    format!("Invalid eventually_within bound in compiled clause '{raw}'")
                })?;
                if bound == 0 {
                    return Err(format!(
                        "Invalid eventually_within bound in compiled clause '{raw}'"
                    ));
                }
                Ok(Self::EventuallyWithin(
                    (*trigger).to_string(),
                    (*consequence).to_string(),
                    bound,
                ))
            }
            _ => Err(format!("Invalid compiled temporal clause '{raw}'")),
        }
    }

    fn names(&self) -> Vec<&str> {
        match self {
            Self::Always(name) | Self::Eventually(name) | Self::ExactlyOnce(name) => {
                vec![name]
            }
            Self::Until(left, right)
            | Self::LeadsTo(left, right)
            | Self::NeverAfter(left, right)
            | Self::EventuallyWithin(left, right, _) => vec![left, right],
        }
    }
}

impl VM {
    fn model_history_percentiles(samples: &[u64]) -> (u64, u64) {
        if samples.is_empty() {
            return (0, 0);
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let nearest_rank = |percent: usize| {
            sorted[(sorted.len().saturating_mul(percent).saturating_add(99) / 100)
                .saturating_sub(1)
                .min(sorted.len() - 1)]
        };
        (nearest_rank(50), nearest_rank(95))
    }

    fn model_invariants_hold(
        worker: &mut VM,
        invariants: &[Value],
        point: &str,
    ) -> Result<(), String> {
        for (index, invariant) in invariants.iter().enumerate() {
            let result = worker
                .call_value(invariant, Vec::new())
                .map_err(|error| format!("invariant {} errored {point}: {error}", index + 1))?;
            let valid = result.as_bool().ok_or_else(|| {
                format!(
                    "invariant {} returned {}, expected bool {point}",
                    index + 1,
                    result.type_name()
                )
            })?;
            if !valid {
                return Err(format!("invariant {} was false {point}", index + 1));
            }
        }
        Ok(())
    }

    fn model_sample(
        worker: &VM,
        names: &[String],
        prior_event_count: &mut usize,
        signals: &mut std::collections::HashMap<String, Vec<bool>>,
        occurrences: &mut std::collections::HashMap<String, usize>,
        previous_membership: &mut std::collections::HashMap<String, bool>,
    ) {
        let new_events = worker.event_log.iter().skip(*prior_event_count);
        for name in names {
            let event_count = new_events
                .clone()
                .filter(|event| event.event_name == *name)
                .count();
            let membership = worker.world.any_with_component(name);
            let previous = previous_membership.insert(name.clone(), membership).unwrap_or(false);
            if event_count > 0 {
                *occurrences.entry(name.clone()).or_default() += event_count;
            } else if membership && !previous {
                *occurrences.entry(name.clone()).or_default() += 1;
            }
            signals
                .entry(name.clone())
                .or_default()
                .push(event_count > 0 || membership);
        }
        *prior_event_count = worker.event_log.len();
    }

    fn model_temporal_holds(
        temporal: &[ModelTemporal],
        signals: &std::collections::HashMap<String, Vec<bool>>,
        occurrences: &std::collections::HashMap<String, usize>,
    ) -> Result<(), String> {
        for clause in temporal {
            match clause {
                ModelTemporal::Always(name) => {
                    if signals.get(name).is_none_or(|values| values.iter().any(|value| !value)) {
                        return Err(format!("temporal clause `always {name}` failed"));
                    }
                }
                ModelTemporal::Eventually(name) => {
                    if signals.get(name).is_none_or(|values| !values.iter().any(|value| *value)) {
                        return Err(format!("temporal clause `eventually {name}` failed"));
                    }
                }
                ModelTemporal::Until(condition, terminal) => {
                    let left = signals.get(condition).map(Vec::as_slice).unwrap_or(&[]);
                    let right = signals.get(terminal).map(Vec::as_slice).unwrap_or(&[]);
                    let Some(end) = right.iter().position(|value| *value) else {
                        return Err(format!(
                            "temporal clause `{condition} until {terminal}` failed: terminal was never observed"
                        ));
                    };
                    if left.iter().take(end).any(|value| !value) || left.len() <= end {
                        return Err(format!(
                            "temporal clause `{condition} until {terminal}` failed before step {end}"
                        ));
                    }
                }
                ModelTemporal::LeadsTo(trigger, consequence) => {
                    let left = signals.get(trigger).map(Vec::as_slice).unwrap_or(&[]);
                    let right = signals.get(consequence).map(Vec::as_slice).unwrap_or(&[]);
                    for (index, active) in left.iter().enumerate() {
                        if *active && !right.iter().skip(index).any(|value| *value) {
                            return Err(format!(
                                "temporal clause `{trigger} eventually {consequence}` failed after observation {index}"
                            ));
                        }
                    }
                }
                ModelTemporal::ExactlyOnce(name) => {
                    let count = occurrences.get(name).copied().unwrap_or(0);
                    if count != 1 {
                        return Err(format!(
                            "temporal clause `exactly_once {name}` observed {count} occurrence(s)"
                        ));
                    }
                }
                ModelTemporal::NeverAfter(prohibited, terminal) => {
                    let prohibited_values = signals
                        .get(prohibited)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let terminal_values =
                        signals.get(terminal).map(Vec::as_slice).unwrap_or(&[]);
                    if let Some(terminal_index) = terminal_values.iter().position(|value| *value) {
                        if let Some(offset) = prohibited_values
                            .iter()
                            .skip(terminal_index + 1)
                            .position(|value| *value)
                        {
                            return Err(format!(
                                "temporal clause `{prohibited} never_after {terminal}` failed at observation {}",
                                terminal_index + 1 + offset
                            ));
                        }
                    }
                }
                ModelTemporal::EventuallyWithin(trigger, consequence, bound) => {
                    let triggers = signals.get(trigger).map(Vec::as_slice).unwrap_or(&[]);
                    let consequences = signals
                        .get(consequence)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    for (index, active) in triggers.iter().enumerate() {
                        if !active {
                            continue;
                        }
                        let end = index
                            .saturating_add(*bound)
                            .saturating_add(1)
                            .min(consequences.len());
                        if !consequences
                            .get(index..end)
                            .is_some_and(|window| window.iter().any(|value| *value))
                        {
                            return Err(format!(
                                "temporal clause `{trigger} eventually_within {consequence} {bound}` failed after observation {index}"
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn run_model_trial(
        &self,
        base: &crate::world::WorldSnapshot,
        commands: &[Value],
        invariants: &[Value],
        temporal: &[ModelTemporal],
        trace: &[ModelTraceStep],
        seed: u64,
    ) -> Result<(), String> {
        let mut worker = VM::from_shared_state(self.shared_state());
        worker.indexed_decl = Arc::clone(&self.indexed_decl);
        worker.ordered_decl = Arc::clone(&self.ordered_decl);
        worker.migrations = self.migrations.clone();
        worker.restore_events_from(base);
        worker.world.restore(base.clone());
        worker.is_worker = false;
        worker.in_simulation_fork = 1;
        worker.suppress_output = true;
        worker.set_random_seed(seed);

        let mut observation_names = temporal
            .iter()
            .flat_map(ModelTemporal::names)
            .map(str::to_string)
            .collect::<Vec<_>>();
        observation_names.sort();
        observation_names.dedup();
        let mut signals = std::collections::HashMap::new();
        let mut occurrences = std::collections::HashMap::new();
        let mut previous_membership = std::collections::HashMap::new();
        let mut prior_event_count = 0usize;

        Self::model_invariants_hold(&mut worker, invariants, "in the initial state")?;
        Self::model_sample(
            &worker,
            &observation_names,
            &mut prior_event_count,
            &mut signals,
            &mut occurrences,
            &mut previous_membership,
        );
        for (step_index, step) in trace.iter().enumerate() {
            if step.flush_before {
                worker.bi_flush_events(Vec::new()).map_err(|error| {
                    format!("flush before step {} ({}) failed: {error}", step_index + 1, step.label)
                })?;
                Self::model_invariants_hold(
                    &mut worker,
                    invariants,
                    &format!("after the flush before step {}", step_index + 1),
                )?;
            }
            let command = commands.get(step.command).ok_or_else(|| {
                format!("trace selects missing command index {}", step.command)
            })?;
            worker.call_value(command, Vec::new()).map_err(|error| {
                format!("command '{}' failed at step {}: {error}", step.label, step_index + 1)
            })?;
            Self::model_invariants_hold(
                &mut worker,
                invariants,
                &format!("after command '{}' at step {}", step.label, step_index + 1),
            )?;
            if step.flush_after {
                worker.bi_flush_events(Vec::new()).map_err(|error| {
                    format!("flush after step {} ({}) failed: {error}", step_index + 1, step.label)
                })?;
                Self::model_invariants_hold(
                    &mut worker,
                    invariants,
                    &format!("after the flush following step {}", step_index + 1),
                )?;
            }
            Self::model_sample(
                &worker,
                &observation_names,
                &mut prior_event_count,
                &mut signals,
                &mut occurrences,
                &mut previous_membership,
            );
        }
        Self::model_temporal_holds(temporal, &signals, &occurrences)
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
        &self,
        base: &crate::world::WorldSnapshot,
        commands: &[Value],
        invariants: &[Value],
        temporal: &[ModelTemporal],
        original: &[ModelTraceStep],
        seed: u64,
    ) -> (Vec<ModelTraceStep>, String) {
        let mut trace = original.to_vec();
        let mut reason = self
            .run_model_trial(base, commands, invariants, temporal, &trace, seed)
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
                if let Err(candidate_reason) =
                    self.run_model_trial(base, commands, invariants, temporal, &candidate, seed)
                {
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
        let exact_trace = config.trace;
        let histories_requested = if exact_trace.is_some() { 1 } else { runs };
        let mut commands_executed = 0_u64;
        let mut history_ns = Vec::with_capacity(histories_requested as usize);

        for run_index in 0..histories_requested {
            let run_seed = if exact_trace.is_some() {
                campaign_seed
            } else {
                let mut value = campaign_seed
                    .wrapping_add(u64::from(run_index).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                value ^ (value >> 31)
            };
            let trace = if let Some(trace) = exact_trace.as_ref() {
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
                let mut random = run_seed;
                let mut trace = Vec::with_capacity(max_commands as usize);
                for _ in 0..max_commands {
                    random ^= random >> 12;
                    random ^= random << 25;
                    random ^= random >> 27;
                    let selection = random.wrapping_mul(0x2545_F491_4F6C_DD1D);
                    let command = selection as usize % commands.len();
                    trace.push(ModelTraceStep {
                        command,
                        label: labels[command].clone(),
                        flush_before: selection & (1 << 8) != 0,
                        flush_after: selection & (1 << 17) != 0,
                    });
                }
                trace
            };
            commands_executed = commands_executed.saturating_add(trace.len() as u64);
            let history_started = std::time::Instant::now();
            let trial =
                self.run_model_trial(&base, &commands, &invariants, &temporal, &trace, run_seed);
            history_ns.push(
                history_started
                    .elapsed()
                    .as_nanos()
                    .min(u128::from(u64::MAX)) as u64,
            );
            if trial.is_ok() {
                continue;
            }

            let shrink_started = std::time::Instant::now();
            let (minimal, reason) = self.shrink_model_trace(
                &base,
                &commands,
                &invariants,
                &temporal,
                &trace,
                run_seed,
            );
            let shrink_ns = shrink_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            let (median_history_ns, p95_history_ns) =
                Self::model_history_percentiles(&history_ns);
            self.model_check_reports.push(ModelCheckReport {
                model: model.clone(),
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
            return Err(format!(
                "Stateful model '{}' failed in history {} after shrinking {} generated command(s) to {}:\n{}\nRAD_MODEL_FAILURE={}",
                model,
                run_index + 1,
                marker["generated_trace"]
                    .as_array()
                    .map_or(0, Vec::len),
                marker["minimal_trace"].as_array().map_or(0, Vec::len),
                marker["reason"].as_str().unwrap_or("model failure"),
                marker
            ));
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
