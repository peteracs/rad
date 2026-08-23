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

struct ModelObservationPlan {
    names: Vec<String>,
    slots: std::collections::HashMap<String, usize>,
    dependencies: ModelAccessSet,
}

impl ModelObservationPlan {
    fn new(temporal: &[ModelTemporal]) -> Self {
        let mut names = temporal
            .iter()
            .flat_map(ModelTemporal::names)
            .map(str::to_string)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        let slots = names
            .iter()
            .enumerate()
            .map(|(slot, name)| (name.clone(), slot))
            .collect();
        let mut dependencies = ModelAccessSet::default();
        for name in &names {
            dependencies.insert(name);
        }
        Self {
            names,
            slots,
            dependencies,
        }
    }

    #[inline]
    fn slot(&self, name: &str) -> Option<usize> {
        self.slots.get(name).copied()
    }
}

#[derive(Clone, Copy)]
enum ModelInvariantPoint<'a> {
    Initial,
    FlushBefore(usize),
    Command { label: &'a str, step: usize },
    FlushAfter(usize),
}

impl std::fmt::Display for ModelInvariantPoint<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initial => formatter.write_str("in the initial state"),
            Self::FlushBefore(step) => write!(formatter, "after the flush before step {step}"),
            Self::Command { label, step } => {
                write!(formatter, "after command '{label}' at step {step}")
            }
            Self::FlushAfter(step) => write!(formatter, "after the flush following step {step}"),
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
        point: ModelInvariantPoint<'_>,
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

    fn begin_model_access_trace(worker: &mut VM) {
        if let Some(trace) = &worker.model_access_trace {
            trace.borrow_mut().clear();
        } else {
            worker.model_access_trace = Some(std::cell::RefCell::new(ModelAccessTrace::default()));
        }
    }

    fn finish_model_access_trace<T>(
        worker: &VM,
        consume: impl FnOnce(&ModelAccessTrace) -> T,
    ) -> T {
        let mut trace = worker
            .model_access_trace
            .as_ref()
            .expect("model access trace is paired")
            .borrow_mut();
        let result = consume(&trace);
        trace.clear();
        result
    }

    fn model_invariants_hold_traced(
        worker: &mut VM,
        invariants: &[Value],
        point: ModelInvariantPoint<'_>,
        dependencies: &mut ModelAccessSet,
        should_run: bool,
    ) -> Result<(), String> {
        if !should_run {
            return Ok(());
        }
        Self::begin_model_access_trace(worker);
        let result = Self::model_invariants_hold(worker, invariants, point);
        let invariant_wrote = Self::finish_model_access_trace(worker, |trace| {
            dependencies.extend(&trace.reads);
            !trace.writes.is_empty()
        });
        // Invariants are observational by language contract. Keep the runtime
        // fail-closed if malformed checker-less bytecode violates that rule.
        if invariant_wrote {
            return Err("model invariant attempted to mutate state".to_string());
        }
        result
    }

    fn model_sample(
        worker: &VM,
        plan: &ModelObservationPlan,
        prior_event_count: &mut usize,
        signals: &mut [Vec<bool>],
        occurrences: &mut [usize],
        previous_membership: &mut [bool],
        event_counts: &mut [usize],
    ) {
        event_counts.fill(0);
        for event in worker.event_log.iter().skip(*prior_event_count) {
            if let Some(slot) = plan.slot(&event.event_name) {
                event_counts[slot] += 1;
            }
        }
        for (slot, name) in plan.names.iter().enumerate() {
            let event_count = event_counts[slot];
            let membership = worker.world.any_with_component(name);
            let previous = std::mem::replace(&mut previous_membership[slot], membership);
            if event_count > 0 {
                occurrences[slot] += event_count;
            } else if membership && !previous {
                occurrences[slot] += 1;
            }
            signals[slot].push(event_count > 0 || membership);
        }
        *prior_event_count = worker.event_log.len();
    }

    fn model_repeat_unchanged_sample(
        signals: &mut [Vec<bool>],
        previous_membership: &[bool],
    ) {
        for (signal, membership) in signals.iter_mut().zip(previous_membership) {
            signal.push(*membership);
        }
    }

    fn model_temporal_holds(
        temporal: &[ModelTemporal],
        plan: &ModelObservationPlan,
        signals: &[Vec<bool>],
        occurrences: &[usize],
    ) -> Result<(), String> {
        let values = |name: &str| {
            plan.slot(name)
                .map(|slot| signals[slot].as_slice())
                .unwrap_or(&[])
        };
        for clause in temporal {
            match clause {
                ModelTemporal::Always(name) => {
                    if values(name).iter().any(|value| !value) {
                        return Err(format!("temporal clause `always {name}` failed"));
                    }
                }
                ModelTemporal::Eventually(name) => {
                    if !values(name).iter().any(|value| *value) {
                        return Err(format!("temporal clause `eventually {name}` failed"));
                    }
                }
                ModelTemporal::Until(condition, terminal) => {
                    let left = values(condition);
                    let right = values(terminal);
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
                    let left = values(trigger);
                    let right = values(consequence);
                    for (index, active) in left.iter().enumerate() {
                        if *active && !right.iter().skip(index).any(|value| *value) {
                            return Err(format!(
                                "temporal clause `{trigger} eventually {consequence}` failed after observation {index}"
                            ));
                        }
                    }
                }
                ModelTemporal::ExactlyOnce(name) => {
                    let count = plan.slot(name).map_or(0, |slot| occurrences[slot]);
                    if count != 1 {
                        return Err(format!(
                            "temporal clause `exactly_once {name}` observed {count} occurrence(s)"
                        ));
                    }
                }
                ModelTemporal::NeverAfter(prohibited, terminal) => {
                    let prohibited_values = values(prohibited);
                    let terminal_values = values(terminal);
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
                    let triggers = values(trigger);
                    let consequences = values(consequence);
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
}
