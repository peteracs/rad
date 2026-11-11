// Rendering of a `BlameReport`: the text a person reads and the JSON a tool
// consumes. Both come from the same findings; neither replays anything.

/// Report format written by `rad blame --json`.
pub const BLAME_FORMAT: &str = "rad-blame";
/// Version of the JSON report; bumped when a field changes meaning or goes.
pub const BLAME_FORMAT_VERSION: u64 = 1;

impl BlameReport {
    /// The report as one JSON object (`format`, `version`, symptom, inputs,
    /// minimal cause and script, lineage, search statistics).
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        let (symptom_kind, symptom_text) = match &self.symptom {
            Symptom::Error(text) => ("error", text),
            Symptom::Output(text) => ("output", text),
        };
        let inputs: Vec<serde_json::Value> = self
            .inputs
            .iter()
            .map(|finding| {
                json!({
                    "input": finding.unit.ordinal,
                    "io_record": finding.unit.record,
                    "frame": finding.unit.frame,
                    "builtin": finding.unit.builtin,
                    "value": finding.unit.preview,
                    "in_minimal_cause": finding.in_minimal_cause,
                    "but_for": finding.but_for.map(ButFor::as_str),
                    "withheld": finding.withheld_verdict.map(Verdict::as_str),
                    "withheld_error": finding.withheld_detail,
                    "in_lineage": finding.in_lineage,
                })
            })
            .collect();
        json!({
            "format": BLAME_FORMAT,
            "version": BLAME_FORMAT_VERSION,
            "program": self.program,
            "symptom": { "kind": symptom_kind, "text": symptom_text },
            "io_records": self.io_records,
            "inputs": inputs,
            "minimal_cause": self.minimal_cause,
            "minimal_script": self.minimal_script(),
            "occurs_without_input": self.occurs_without_input,
            "lineage": self.lineage.as_ref().map(|lineage| json!({
                "target": lineage.target,
                "explanation": lineage.explanation,
                "frames": lineage.frames,
                "inputs": lineage.inputs,
            })),
            "search": {
                "strategy": match self.strategy {
                    Strategy::Guided => "guided",
                    Strategy::Ddmin => "ddmin",
                },
                "replays": self.replays,
                "cache_hits": self.cache_hits,
                "jobs": self.jobs,
                "elapsed_ms": self.elapsed_ms as u64,
                "steps": self.steps,
            },
        })
    }

    /// The report as a person reads it; `trace_name` labels the header.
    pub fn render_text(&self, trace_name: &str) -> String {
        let mut out = String::new();
        let line = |out: &mut String, text: &str| {
            out.push_str(text);
            out.push('\n');
        };
        let total = self.inputs.len();
        let mut builtins: Vec<&str> = self
            .inputs
            .iter()
            .map(|f| f.unit.builtin.as_str())
            .collect();
        builtins.sort_unstable();
        builtins.dedup();

        line(&mut out, &format!("rad blame  {trace_name}"));
        line(
            &mut out,
            &format!("  symptom    {}", self.symptom.describe()),
        );
        line(&mut out, &format!("  program    {}", self.program));
        line(
            &mut out,
            &format!(
                "  session    {} recorded io, {} inputs that can be withheld ({})",
                self.io_records,
                total,
                builtins.join(", ")
            ),
        );
        line(
            &mut out,
            &format!(
                "  searched   {} histories replayed in {:.2} s on {} worker{}",
                self.replays,
                self.elapsed_ms as f64 / 1000.0,
                self.jobs,
                if self.jobs == 1 { "" } else { "s" }
            ),
        );
        line(&mut out, "");

        if self.occurs_without_input {
            line(
                &mut out,
                "No input causes it: the symptom occurs with every input withheld.",
            );
            line(
                &mut out,
                "Look at the program and its start state, not at the session.",
            );
        } else {
            line(
                &mut out,
                &format!(
                    "Minimal cause: {} of {} inputs. These alone still produce the symptom;",
                    self.minimal_cause.len(),
                    total
                ),
            );
            line(
                &mut out,
                "withhold any one of them as well and it does not occur.",
            );
            line(&mut out, "");
            for finding in self.inputs.iter().filter(|f| f.in_minimal_cause) {
                line(
                    &mut out,
                    &format!(
                        "  #{:<4} frame {:<5} {}  {}",
                        finding.unit.ordinal,
                        finding.unit.frame,
                        finding.unit.builtin,
                        finding.unit.preview
                    ),
                );
                if let Some(note) = withheld_note(finding) {
                    line(&mut out, &format!("         {note}"));
                }
            }
        }

        if self.inputs.iter().any(|f| f.but_for.is_some()) {
            line(&mut out, "");
            line(&mut out, "Each input withheld alone from the full session:");
            for (class, meaning) in [
                (ButFor::Necessary, "the symptom disappears"),
                (ButFor::Structural, "the session cannot finish"),
                (ButFor::Incidental, "the symptom still occurs"),
            ] {
                let members: Vec<usize> = self
                    .inputs
                    .iter()
                    .filter(|f| f.but_for == Some(class))
                    .map(|f| f.unit.ordinal)
                    .collect();
                line(
                    &mut out,
                    &format!(
                        "  {:<11} {:>4}   {}{}",
                        class.as_str(),
                        members.len(),
                        meaning,
                        if members.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", ordinal_ranges(&members))
                        }
                    ),
                );
            }
            let redundant: Vec<usize> = self
                .inputs
                .iter()
                .filter(|f| f.in_minimal_cause && f.but_for == Some(ButFor::Incidental))
                .map(|f| f.unit.ordinal)
                .collect();
            if !redundant.is_empty() {
                line(&mut out, "");
                line(
                    &mut out,
                    &format!(
                        "Overdetermined: {} is in the minimal cause yet incidental in the full",
                        ordinal_ranges(&redundant)
                    ),
                );
                line(
                    &mut out,
                    "session. Another input can stand in for it, so the session holds more than",
                );
                line(
                    &mut out,
                    "one sufficient cause; this report shows one of them.",
                );
            }
        }

        if let Some(lineage) = &self.lineage {
            line(&mut out, "");
            line(
                &mut out,
                &format!("Lineage of {} (what why() reports):", lineage.target),
            );
            for text in lineage.explanation.lines() {
                line(&mut out, &format!("  {text}"));
            }
            let cause: Vec<usize> = self.minimal_cause.clone();
            let named: Vec<usize> = lineage.inputs.clone();
            let missed: Vec<usize> = cause
                .iter()
                .copied()
                .filter(|o| !named.contains(o))
                .collect();
            let extra: Vec<usize> = named
                .iter()
                .copied()
                .filter(|o| !cause.contains(o))
                .collect();
            line(
                &mut out,
                &format!(
                    "  lineage names {}; the minimal cause is {}",
                    if named.is_empty() {
                        "no input".to_string()
                    } else {
                        format!("input {}", ordinal_ranges(&named))
                    },
                    if cause.is_empty() {
                        "empty".to_string()
                    } else {
                        ordinal_ranges(&cause)
                    }
                ),
            );
            if missed.is_empty() && extra.is_empty() {
                line(&mut out, "  They agree.");
            }
            if !missed.is_empty() {
                line(
                    &mut out,
                    &format!(
                        "  Not in the lineage, yet required: {}. The chain follows the write, not the values it read.",
                        ordinal_ranges(&missed)
                    ),
                );
            }
            if !extra.is_empty() {
                line(
                    &mut out,
                    &format!(
                        "  In the lineage, yet not required for this symptom: {}.",
                        ordinal_ranges(&extra)
                    ),
                );
            }
        }

        if !self.steps.is_empty() {
            line(&mut out, "");
            line(&mut out, "Search:");
            for step in &self.steps {
                line(&mut out, &format!("  {step}"));
            }
        }
        out
    }
}

fn withheld_note(finding: &InputFinding) -> Option<String> {
    let verdict = finding.withheld_verdict?;
    let effect = match verdict {
        Verdict::Present => "the symptom still occurs (another input stands in)".to_string(),
        Verdict::Absent => "the symptom is gone".to_string(),
        Verdict::Diverted => format!(
            "the run ends differently: {}",
            finding
                .withheld_detail
                .as_deref()
                .unwrap_or("another error")
        ),
        Verdict::Broken => "the session cannot finish".to_string(),
    };
    Some(format!("without it in the full session: {effect}"))
}

/// `[1, 2, 3, 7, 9, 10]` as `#1-#3 #7 #9-#10`.
fn ordinal_ranges(ordinals: &[usize]) -> String {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < ordinals.len() {
        let start = ordinals[index];
        let mut end = start;
        while index + 1 < ordinals.len() && ordinals[index + 1] == end + 1 {
            index += 1;
            end = ordinals[index];
        }
        parts.push(if end == start {
            format!("#{start}")
        } else if end == start + 1 {
            format!("#{start} #{end}")
        } else {
            format!("#{start}-#{end}")
        });
        index += 1;
    }
    parts.join(" ")
}
