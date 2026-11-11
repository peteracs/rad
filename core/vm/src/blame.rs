//! Counterfactual root cause for a recorded session (`rad blame`).
//!
//! `why()` answers from lineage: which handler wrote a value, which event it
//! handled, who emitted that event. Lineage reports what happened. It cannot
//! say whether the outcome *needed* any of it, and it never mentions an input
//! whose only contribution was a value some later handler read.
//!
//! This module answers by intervention instead. A trace records every input
//! that crossed the determinism boundary, so the session that would have
//! happened without one of them can simply be run: replay the tape with that
//! message withheld and look at the outcome. Doing that systematically gives
//! three results for one symptom:
//!
//! - a **minimal cause**: a set of inputs that still produces the symptom
//!   when everything else is withheld, and stops producing it when any single
//!   member is withheld as well (1-minimal, checked by replay);
//! - a **but-for class** for every input of the full session: withholding it
//!   alone removes the symptom (necessary), breaks the session (structural),
//!   or changes nothing about the symptom (incidental);
//! - optionally, the inputs that `why()` lineage names for a value, so the
//!   two notions of cause can be compared on the same session.
//!
//! Every claim in a report is the result of an actual replay. The search is
//! deterministic: candidate histories run on worker threads, and the
//! lowest-numbered success of each round is taken whatever the thread count.

use crate::replay::{is_stream_input, IoRecord, TraceReplayer, COUNTERFACTUAL_EXHAUSTED_PREFIX};
use crate::source_bundle::SourceLayout;
use std::collections::HashMap;

/// The outcome to explain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Symptom {
    /// The run ends in a runtime error whose message contains this text
    /// (empty: any runtime error).
    Error(String),
    /// Something the program printed contains this text.
    Output(String),
}

impl Symptom {
    /// The symptom as a clause for reports: "the program prints …".
    pub fn describe(&self) -> String {
        match self {
            Symptom::Error(text) if text.is_empty() => "the run ends in a runtime error".into(),
            Symptom::Error(text) => format!("the run ends in an error containing {text:?}"),
            Symptom::Output(text) => format!("the program prints {text:?}"),
        }
    }
}

/// A world location whose `why()` lineage is compared with the result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineageTarget {
    /// A component of a named entity (`--why <entity> <component>`).
    Component { entity: String, component: String },
    /// A world resource (`--why-resource <resource>`).
    Resource(String),
}

impl LineageTarget {
    fn describe(&self) -> String {
        match self {
            LineageTarget::Component { entity, component } => format!("{component} of {entity}"),
            LineageTarget::Resource(resource) => format!("resource {resource}"),
        }
    }
}

/// How the minimal cause is searched. Both give a 1-minimal result; they
/// differ in how many histories they replay to get there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Classify every input first, then try the necessary and structural
    /// inputs (and the lineage inputs, when given) as the cause before
    /// falling back to delta debugging.
    Guided,
    /// Plain delta debugging (ddmin) over all inputs.
    Ddmin,
}

/// Source that replaces the trace's embedded program for every history, as
/// `rad replay --with` does: the recorded inputs against edited code.
#[derive(Clone, Debug)]
pub struct EditedProgram {
    /// The program text, already bundled with its modules.
    pub source: String,
    /// Module layout of `source`, for diagnostics and imports.
    pub layout: SourceLayout,
    /// How the report names the program, usually its path.
    pub description: String,
}

/// What to explain and how hard to search. `Default` is the CLI's default
/// except for `jobs`, which the CLI sets from the machine's parallelism.
#[derive(Clone, Debug)]
pub struct BlameOptions {
    /// `None` explains the error the recorded run ended in.
    pub symptom: Option<Symptom>,
    /// A value whose `why()` lineage is reported next to the cause.
    pub lineage: Option<LineageTarget>,
    /// How the minimal cause is searched.
    pub strategy: Strategy,
    /// Classify every input of the full session (one replay per input).
    pub but_for: bool,
    /// Worker threads for candidate histories (at least 1).
    pub jobs: usize,
    /// Upper bound on replayed histories; the search fails past it.
    pub max_replays: usize,
    /// Accept a trace whose integrity check fails, as `rad replay --force`.
    pub force: bool,
}

impl Default for BlameOptions {
    fn default() -> Self {
        Self {
            symptom: None,
            lineage: None,
            strategy: Strategy::Guided,
            but_for: true,
            jobs: 1,
            max_replays: 100_000,
            force: false,
        }
    }
}

/// What one alternate history did to the symptom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The symptom occurs.
    Present,
    /// The session finishes and the symptom does not occur.
    Absent,
    /// The session ends in a different runtime error.
    Diverted,
    /// The history is not a session the program can complete: it asks for
    /// input the history lacks, or exceeds the instruction budget.
    Broken,
}

impl Verdict {
    /// The lower-case name used in text and JSON reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Present => "present",
            Verdict::Absent => "absent",
            Verdict::Diverted => "diverted",
            Verdict::Broken => "broken",
        }
    }
}

#[derive(Clone, Debug)]
struct Outcome {
    verdict: Verdict,
    /// The runtime error of the history, when it had one.
    detail: Option<String>,
}

/// One recorded input that a history may withhold.
#[derive(Clone, Debug)]
pub struct InputUnit {
    /// 1-based position among the inputs of the session.
    pub ordinal: usize,
    /// Index among all io records of the trace.
    pub record: usize,
    /// Frame in which the input was delivered.
    pub frame: u64,
    /// The builtin that read it: `readline`, `input`, `tcp_read`, `udp_recv_*`.
    pub builtin: String,
    /// The delivered value, shortened for display.
    pub preview: String,
    /// The delivered text, when the input is a line of text.
    pub text: Option<String>,
}

/// But-for class of an input in the full recorded session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButFor {
    /// Withholding it alone removes the symptom.
    Necessary,
    /// Withholding it alone leaves a session the program cannot complete.
    Structural,
    /// Withholding it alone leaves the symptom in place.
    Incidental,
}

impl ButFor {
    /// The lower-case name used in text and JSON reports.
    pub fn as_str(self) -> &'static str {
        match self {
            ButFor::Necessary => "necessary",
            ButFor::Structural => "structural",
            ButFor::Incidental => "incidental",
        }
    }
}

/// One input of the recorded session and what the search found about it.
#[derive(Clone, Debug)]
pub struct InputFinding {
    /// The input itself.
    pub unit: InputUnit,
    /// Member of the reported minimal cause.
    pub in_minimal_cause: bool,
    /// `None` when the but-for pass was skipped.
    pub but_for: Option<ButFor>,
    /// The verdict of the history that withheld this input alone.
    pub withheld_verdict: Option<Verdict>,
    /// The runtime error of that history, if it had one.
    pub withheld_detail: Option<String>,
    /// `None` when no lineage target was given.
    pub in_lineage: Option<bool>,
}

/// The `why()` lineage of the requested value, mapped to inputs.
#[derive(Clone, Debug)]
pub struct LineageFinding {
    /// The target as the report names it: "Cell of B5", "resource Books".
    pub target: String,
    /// The `why()` text for the target in the recorded session.
    pub explanation: String,
    /// Frames on the causal chain, newest first. The last one is where
    /// main-timeline code started the chain.
    pub frames: Vec<u64>,
    /// Ordinals of the inputs delivered in that starting frame.
    pub inputs: Vec<usize>,
}

/// The result of [`blame`]: render it with `render_text` or `to_json`.
#[derive(Clone, Debug)]
pub struct BlameReport {
    /// The symptom explained (the recorded error when none was given).
    pub symptom: Symptom,
    /// The program the histories ran: the embedded source or `--with` file.
    pub program: String,
    /// All io records of the trace, inputs and served-as-recorded reads.
    pub io_records: usize,
    /// Every input of the session, in delivery order.
    pub inputs: Vec<InputFinding>,
    /// Ordinals of the minimal cause, ascending. Empty when the symptom
    /// occurs with every input withheld.
    pub minimal_cause: Vec<usize>,
    /// The symptom occurs even when every input is withheld.
    pub occurs_without_input: bool,
    /// Present when a lineage target was given.
    pub lineage: Option<LineageFinding>,
    /// The strategy that ran.
    pub strategy: Strategy,
    /// Histories actually replayed, the recorded one included.
    pub replays: usize,
    /// Candidate histories answered from an earlier replay.
    pub cache_hits: usize,
    /// Worker threads used.
    pub jobs: usize,
    /// Wall-clock time of the whole search.
    pub elapsed_ms: u128,
    /// How the search went, one line per step.
    pub steps: Vec<String>,
}

impl BlameReport {
    /// The minimal cause as input lines, one per line, when every member is
    /// a line of text. Feeding it to the program reproduces the symptom.
    pub fn minimal_script(&self) -> Option<String> {
        let mut out = String::new();
        for finding in self.inputs.iter().filter(|f| f.in_minimal_cause) {
            match &finding.unit.text {
                Some(text) => {
                    out.push_str(text);
                    out.push('\n');
                }
                // End of input needs no line: the stream ending supplies it.
                None if finding.unit.preview == END_OF_INPUT => {}
                None => return None,
            }
        }
        Some(out)
    }
}

const END_OF_INPUT: &str = "(end of input)";
const PREVIEW_CHARS: usize = 72;

/// Immutable tape data shared by every history.
struct Tape {
    trace_source: String,
    trace_layout: SourceLayout,
    features: Vec<String>,
    seed: u64,
    records: Vec<IoRecord>,
    program_source: String,
    program_layout: SourceLayout,
    program_description: String,
}

/// A finished history on the thread that ran it. The VM stays available for
/// questions only that thread can ask (world snapshot, ledger).
struct LocalRun {
    vm: crate::vm::VM,
    outcome: Outcome,
    fuel_spent: u64,
}

/// Instruction budget of an unmetered-looking run that still counts.
const MEASURING_FUEL: u64 = 1 << 62;

fn run_history(
    tape: &Tape,
    symptom: &Symptom,
    removed: &[bool],
    fuel: u64,
) -> Result<LocalRun, String> {
    let replayer = TraceReplayer::counterfactual(
        &tape.trace_source,
        &tape.trace_layout,
        &tape.features,
        tape.seed,
        &tape.records,
        removed,
    );
    let mut vm = crate::replay_compile::compile_trace_vm(
        &tape.program_source,
        &tape.program_description,
        &tape.features,
        &tape.program_layout,
    )?;
    vm.suppress_output();
    vm.fuel = fuel;
    vm.enable_replay(replayer);
    let error = vm.run(0).err();
    let fuel_spent = fuel.saturating_sub(vm.fuel);
    let outcome = classify(symptom, error, &vm.print_buffer);
    Ok(LocalRun {
        vm,
        outcome,
        fuel_spent,
    })
}

fn classify(symptom: &Symptom, error: Option<String>, printed: &[String]) -> Outcome {
    if let Some(message) = &error {
        let broken = message.contains(COUNTERFACTUAL_EXHAUSTED_PREFIX)
            || message.contains("retroactive replay hole")
            || message.contains("Budget exhausted");
        if broken {
            return Outcome {
                verdict: Verdict::Broken,
                detail: error,
            };
        }
    }
    let present = match symptom {
        Symptom::Error(text) => error
            .as_deref()
            .is_some_and(|message| message.contains(text)),
        Symptom::Output(text) => printed.iter().any(|line| line.contains(text)),
    };
    let verdict = if present {
        Verdict::Present
    } else if error.is_some() {
        Verdict::Diverted
    } else {
        Verdict::Absent
    };
    Outcome {
        verdict,
        detail: error,
    }
}

fn preview_of(record: &IoRecord) -> (String, Option<String>) {
    match &record.result {
        Err(error) => (shorten(&format!("error: {error}")), None),
        Ok(value) => match value.get("t").and_then(|tag| tag.as_str()) {
            Some("str") => {
                let text = value["v"].as_str().unwrap_or_default().to_string();
                (shorten(&format!("{text:?}")), Some(text))
            }
            Some("nil") => (END_OF_INPUT.to_string(), None),
            _ => (shorten(&value.to_string()), None),
        },
    }
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= PREVIEW_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(PREVIEW_CHARS - 1).collect();
    out.push('…');
    out
}

/// The search state: which histories were replayed and what they did.
struct Search<'a> {
    tape: &'a Tape,
    symptom: &'a Symptom,
    /// Record index of each input, by position (ordinal - 1).
    units: Vec<usize>,
    fuel: u64,
    jobs: usize,
    max_replays: usize,
    /// Kept input positions (ascending) -> outcome.
    cache: HashMap<Vec<u32>, Outcome>,
    replays: usize,
    cache_hits: usize,
}

impl Search<'_> {
    fn removed_mask(&self, kept: &[u32]) -> Vec<bool> {
        let mut removed = vec![false; self.tape.records.len()];
        for &record in &self.units {
            removed[record] = true;
        }
        for &position in kept {
            removed[self.units[position as usize]] = false;
        }
        removed
    }

    fn all(&self) -> Vec<u32> {
        (0..self.units.len() as u32).collect()
    }

    fn without(&self, kept: &[u32], position: u32) -> Vec<u32> {
        kept.iter().copied().filter(|&p| p != position).collect()
    }

    /// Replay every candidate not yet known and return all outcomes in
    /// candidate order.
    fn test_batch(&mut self, candidates: &[Vec<u32>]) -> Result<Vec<Outcome>, String> {
        let mut pending: Vec<&Vec<u32>> = Vec::new();
        for candidate in candidates {
            if self.cache.contains_key(candidate) {
                self.cache_hits += 1;
            } else if !pending.contains(&candidate) {
                pending.push(candidate);
            }
        }
        if self.replays + pending.len() > self.max_replays {
            return Err(format!(
                "the search needs more than {} alternate histories; raise --max-replays to continue",
                self.max_replays
            ));
        }
        let masks: Vec<Vec<bool>> = pending.iter().map(|kept| self.removed_mask(kept)).collect();
        let outcomes = self.run_masks(&masks)?;
        self.replays += pending.len();
        let fresh: Vec<(Vec<u32>, Outcome)> = pending.into_iter().cloned().zip(outcomes).collect();
        self.cache.extend(fresh);
        Ok(candidates
            .iter()
            .map(|candidate| self.cache[candidate].clone())
            .collect())
    }

    fn test(&mut self, kept: &[u32]) -> Result<Outcome, String> {
        Ok(self.test_batch(&[kept.to_vec()])?.remove(0))
    }

    fn run_masks(&self, masks: &[Vec<bool>]) -> Result<Vec<Outcome>, String> {
        let workers = self.jobs.min(masks.len());
        if workers <= 1 {
            return masks
                .iter()
                .map(|mask| {
                    run_history(self.tape, self.symptom, mask, self.fuel).map(|run| run.outcome)
                })
                .collect();
        }
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results: std::sync::Mutex<Vec<Option<Result<Outcome, String>>>> =
            std::sync::Mutex::new(vec![None; masks.len()]);
        let (tape, symptom, fuel) = (self.tape, self.symptom, self.fuel);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                // The interpreter recurses with the program; match the room
                // a main thread gives it.
                let spawned = std::thread::Builder::new()
                    .stack_size(64 << 20)
                    .spawn_scoped(scope, || loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(mask) = masks.get(index) else { break };
                        let outcome = run_history(tape, symptom, mask, fuel).map(|run| run.outcome);
                        results
                            .lock()
                            .expect("no worker panics while holding the lock")[index] =
                            Some(outcome);
                    });
                if let Err(error) = spawned {
                    results.lock().expect("lock")[0]
                        .get_or_insert(Err(format!("cannot start a replay worker: {error}")));
                }
            }
        });
        results
            .into_inner()
            .expect("workers finished")
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| {
                // A slot is empty only when no worker could be started.
                outcome.unwrap_or_else(|| {
                    run_history(tape, symptom, &masks[index], fuel).map(|run| run.outcome)
                })
            })
            .collect()
    }

    /// Delta debugging: shrink `set` until no chunk of it and no complement
    /// of a chunk still produces the symptom. Every candidate also keeps the
    /// `pinned` inputs, which are not up for removal; `pinned` with all of
    /// `set` must produce the symptom.
    fn ddmin(
        &mut self,
        mut set: Vec<u32>,
        pinned: &[u32],
        steps: &mut Vec<String>,
    ) -> Result<Vec<u32>, String> {
        let with_pinned = |part: &[u32]| -> Vec<u32> {
            let mut kept = pinned.to_vec();
            kept.extend(part);
            kept.sort_unstable();
            kept
        };
        let (from, mut reductions) = (set.len(), 0usize);
        let mut granularity = 2usize;
        while set.len() >= 2 {
            let chunks = split(&set, granularity);
            let candidates: Vec<Vec<u32>> = chunks.iter().map(|chunk| with_pinned(chunk)).collect();
            let outcomes = self.test_batch(&candidates)?;
            if let Some(hit) = outcomes.iter().position(|o| o.verdict == Verdict::Present) {
                set = chunks[hit].clone();
                granularity = 2;
                reductions += 1;
                continue;
            }
            if granularity > 2 {
                let complements: Vec<Vec<u32>> = chunks
                    .iter()
                    .map(|chunk| set.iter().copied().filter(|p| !chunk.contains(p)).collect())
                    .collect();
                let candidates: Vec<Vec<u32>> =
                    complements.iter().map(|rest| with_pinned(rest)).collect();
                let outcomes = self.test_batch(&candidates)?;
                if let Some(hit) = outcomes.iter().position(|o| o.verdict == Verdict::Present) {
                    set = complements[hit].clone();
                    granularity = (granularity - 1).max(2);
                    reductions += 1;
                    continue;
                }
            }
            if granularity >= set.len() {
                break;
            }
            granularity = (granularity * 2).min(set.len());
        }
        if from > set.len() {
            steps.push(format!(
                "delta debugging: {from} inputs down to {} in {reductions} reductions",
                set.len()
            ));
        }
        Ok(set)
    }

    /// Withhold each member in turn until none can go: the result is
    /// 1-minimal by direct replay, whatever produced `set`.
    fn make_one_minimal(
        &mut self,
        mut set: Vec<u32>,
        steps: &mut Vec<String>,
    ) -> Result<Vec<u32>, String> {
        loop {
            let candidates: Vec<Vec<u32>> = set
                .iter()
                .map(|&position| self.without(&set, position))
                .collect();
            let outcomes = self.test_batch(&candidates)?;
            match outcomes.iter().position(|o| o.verdict == Verdict::Present) {
                Some(hit) => {
                    steps.push(format!(
                        "input #{} is not needed once the others are fixed; dropped",
                        set[hit] + 1
                    ));
                    set = candidates[hit].clone();
                }
                None => return Ok(set),
            }
        }
    }
}

/// Split `set` into `parts` consecutive chunks of near-equal size.
fn split(set: &[u32], parts: usize) -> Vec<Vec<u32>> {
    let parts = parts.clamp(1, set.len().max(1));
    let (base, extra) = (set.len() / parts, set.len() % parts);
    let mut chunks = Vec::with_capacity(parts);
    let mut start = 0;
    for part in 0..parts {
        let len = base + usize::from(part < extra);
        chunks.push(set[start..start + len].to_vec());
        start += len;
    }
    chunks
}

/// Find the inputs of the recorded session `trace_text` that cause a symptom.
pub fn blame(
    trace_text: &str,
    edited: Option<EditedProgram>,
    options: &BlameOptions,
) -> Result<BlameReport, String> {
    let started = std::time::Instant::now();
    let base = TraceReplayer::parse(trace_text, options.force)?;
    let recorded_outcome = base
        .recorded_outcome()
        .map(|error| error.map(str::to_string));
    let (program_source, program_layout, program_description) = match edited {
        Some(program) => (program.source, program.layout, program.description),
        None => (
            base.source().to_string(),
            base.source_layout().clone(),
            "embedded source".to_string(),
        ),
    };
    let tape = Tape {
        trace_source: base.source().to_string(),
        trace_layout: base.source_layout().clone(),
        features: base.features().to_vec(),
        seed: base.seed(),
        records: base.io_records().to_vec(),
        program_source,
        program_layout,
        program_description: program_description.clone(),
    };
    drop(base);

    let units: Vec<usize> = tape
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| is_stream_input(&record.builtin))
        .map(|(index, _)| index)
        .collect();
    if units.is_empty() {
        return Err(
            "the trace holds no stream input to withhold (readline, input, tcp_read, udp_recv_*): \
             nothing the outside world sent can be the cause"
                .into(),
        );
    }

    let symptom = match &options.symptom {
        Some(symptom) => symptom.clone(),
        None => match recorded_outcome {
            Some(Some(error)) => Symptom::Error(error),
            _ => {
                return Err(
                    "the recorded run ended without an error, so say which outcome to explain: \
                     --output <text> for something it printed, or --error <text> with --with <file.rad>"
                        .into(),
                )
            }
        },
    };

    let mut steps = Vec::new();

    // The recorded session itself, through the same machinery as every
    // alternate history: the symptom must be there, and its instruction
    // count bounds the others.
    let keep_all = vec![false; tape.records.len()];
    let recorded = run_history(&tape, &symptom, &keep_all, MEASURING_FUEL)?;
    if recorded.outcome.verdict != Verdict::Present {
        return Err(match &recorded.outcome.detail {
            Some(error) => format!(
                "the symptom does not occur in the recorded session ({}); the session ended in: {error}",
                symptom.describe()
            ),
            None => format!(
                "the symptom does not occur in the recorded session ({})",
                symptom.describe()
            ),
        });
    }
    // An alternate history may do more work than the recorded one, but one
    // that loops is not a session. Four times the recorded work, plus slack
    // for sessions too short to measure.
    let fuel = recorded
        .fuel_spent
        .saturating_mul(4)
        .saturating_add(1_000_000);

    let mut search = Search {
        tape: &tape,
        symptom: &symptom,
        units,
        fuel,
        jobs: options.jobs.max(1),
        max_replays: options.max_replays,
        cache: HashMap::new(),
        replays: 1,
        cache_hits: 0,
    };
    let all = search.all();
    search.cache.insert(all.clone(), recorded.outcome.clone());

    let lineage = options
        .lineage
        .as_ref()
        .map(|target| lineage_of(&recorded.vm, target, &tape, &search.units));

    let nothing = search.test(&[])?;
    let occurs_without_input = nothing.verdict == Verdict::Present;

    // But-for class of every input in the full session.
    let mut but_for: Vec<Option<(ButFor, Outcome)>> = vec![None; all.len()];
    if options.but_for && !occurs_without_input {
        let candidates: Vec<Vec<u32>> = all.iter().map(|&p| search.without(&all, p)).collect();
        let outcomes = search.test_batch(&candidates)?;
        for (position, outcome) in outcomes.into_iter().enumerate() {
            let class = match outcome.verdict {
                Verdict::Present => ButFor::Incidental,
                Verdict::Broken => ButFor::Structural,
                Verdict::Absent | Verdict::Diverted => ButFor::Necessary,
            };
            but_for[position] = Some((class, outcome));
        }
        let count = |class: ButFor| {
            but_for
                .iter()
                .filter(|entry| entry.as_ref().is_some_and(|(c, _)| *c == class))
                .count()
        };
        steps.push(format!(
            "but-for: withheld each of {} inputs alone: {} necessary, {} structural, {} incidental",
            all.len(),
            count(ButFor::Necessary),
            count(ButFor::Structural),
            count(ButFor::Incidental)
        ));
    }

    let minimal: Vec<u32> = if occurs_without_input {
        steps.push("the symptom occurs with every input withheld: no input causes it".into());
        Vec::new()
    } else {
        let mut start = all.clone();
        let mut pinned: Vec<u32> = Vec::new();
        if options.strategy == Strategy::Guided {
            let required: Vec<u32> = (0..all.len() as u32)
                .filter(|&p| {
                    matches!(
                        but_for[p as usize],
                        Some((ButFor::Necessary | ButFor::Structural, _))
                    )
                })
                .collect();
            let lineage_inputs: Vec<u32> = lineage
                .as_ref()
                .map(|l| {
                    l.inputs
                        .iter()
                        .map(|ordinal| (*ordinal - 1) as u32)
                        .collect()
                })
                .unwrap_or_default();
            let mut with_lineage = required.clone();
            with_lineage.extend(&lineage_inputs);
            with_lineage.sort_unstable();
            with_lineage.dedup();
            let seeds = [
                ("the inputs lineage names", lineage_inputs),
                ("the necessary and structural inputs", required.clone()),
                ("the necessary, structural and lineage inputs", with_lineage),
            ];
            let mut seeded = false;
            for (label, seed) in seeds {
                if seed.is_empty() || seed.len() >= all.len() {
                    continue;
                }
                if search.test(&seed)?.verdict == Verdict::Present {
                    steps.push(format!(
                        "{label} ({}) reproduce the symptom on their own",
                        seed.len()
                    ));
                    start = seed;
                    seeded = true;
                    break;
                }
                steps.push(format!(
                    "{label} ({}) do not reproduce it alone",
                    seed.len()
                ));
            }
            // Every sufficient set found by removal holds the inputs that
            // cannot be removed. Keep them and search only the rest for
            // what they still need.
            if !seeded && !required.is_empty() {
                start = all
                    .iter()
                    .copied()
                    .filter(|p| !required.contains(p))
                    .collect();
                pinned = required;
            }
        }
        let mut reduced = search.ddmin(start, &pinned, &mut steps)?;
        reduced.extend(&pinned);
        reduced.sort_unstable();
        search.make_one_minimal(reduced, &mut steps)?
    };
    if !occurs_without_input {
        steps.push(format!(
            "minimal cause: {} of {} inputs, 1-minimal by replay",
            minimal.len(),
            all.len()
        ));
    }

    let lineage_set: Option<Vec<usize>> = lineage.as_ref().map(|l| l.inputs.clone());
    let inputs: Vec<InputFinding> = search
        .units
        .iter()
        .enumerate()
        .map(|(position, &record)| {
            let io = &tape.records[record];
            let (preview, text) = preview_of(io);
            let ordinal = position + 1;
            let withheld = match &but_for[position] {
                Some((_, outcome)) => Some(outcome.clone()),
                None => search
                    .cache
                    .get(&search.without(&all, position as u32))
                    .cloned(),
            };
            InputFinding {
                unit: InputUnit {
                    ordinal,
                    record,
                    frame: io.frame,
                    builtin: io.builtin.clone(),
                    preview,
                    text,
                },
                in_minimal_cause: minimal.contains(&(position as u32)),
                but_for: but_for[position].as_ref().map(|(class, _)| *class),
                withheld_verdict: withheld.as_ref().map(|outcome| outcome.verdict),
                withheld_detail: withheld.and_then(|outcome| outcome.detail),
                in_lineage: lineage_set.as_ref().map(|set| set.contains(&ordinal)),
            }
        })
        .collect();

    Ok(BlameReport {
        symptom: symptom.clone(),
        program: program_description,
        io_records: tape.records.len(),
        inputs,
        minimal_cause: minimal.iter().map(|&p| p as usize + 1).collect(),
        occurs_without_input,
        lineage,
        strategy: options.strategy,
        replays: search.replays,
        cache_hits: search.cache_hits,
        jobs: search.jobs,
        elapsed_ms: started.elapsed().as_millis(),
        steps,
    })
}

fn lineage_of(
    vm: &crate::vm::VM,
    target: &LineageTarget,
    tape: &Tape,
    units: &[usize],
) -> LineageFinding {
    let ledger = &vm.ledger;
    let (explanation, frames) = match target {
        LineageTarget::Component { entity, component } => (
            ledger.explain_named(entity, component, u64::MAX),
            ledger.lineage_frames(|write| {
                write.entity_name.as_deref() == Some(entity.as_str())
                    && (write.component == component.as_str() || write.component == "*")
            }),
        ),
        LineageTarget::Resource(resource) => (
            ledger.explain_resource(resource, u64::MAX),
            ledger.lineage_frames(|write| {
                write.entity.is_none() && write.component == resource.as_str()
            }),
        ),
    };
    let frames = frames.unwrap_or_default();
    // The chain ends in the frame where main-timeline code started it. The
    // inputs delivered in that frame are the ones lineage points at; the
    // frames before it on the chain are handlers running, not input arriving.
    let inputs = match frames.last() {
        Some(&root) => units
            .iter()
            .enumerate()
            .filter(|(_, &record)| tape.records[record].frame == root)
            .map(|(position, _)| position + 1)
            .collect(),
        None => Vec::new(),
    };
    LineageFinding {
        target: target.describe(),
        explanation,
        frames,
        inputs,
    }
}

include!("blame/render.rs");
include!("blame/tests.rs");
