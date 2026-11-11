#[cfg(test)]
mod tests {
    use super::*;

    /// A door alarm: it trips only while armed, and either sensor trips it.
    const ALARM: &str = r#"
let mut armed = false
let mut tripped = false
let mut running = true
while running {
    let line = trim(readline())
    if line == "done" or line == "" {
        running = false
    } else if line == "arm" {
        armed = true
    } else if line == "disarm" {
        armed = false
    } else if line == "door" or line == "window" {
        if armed { tripped = true }
    }
}
if tripped { print("ALARM") }
assert(not tripped, "alarm tripped")
"#;

    /// A tape as `--record` writes it for a program fed `lines` on stdin.
    /// Built by hand because a unit test has no stdin to record from; the
    /// CLI tests record real sessions.
    fn tape(source: &str, lines: &[&str], end_error: Option<&str>) -> String {
        use serde_json::json;
        let io: Vec<(&str, serde_json::Value)> = lines
            .iter()
            .map(|line| ("readline", json!({"t": "str", "v": line})))
            .collect();
        tape_of(source, &io, end_error)
    }

    /// The same for any argument-less io: `(builtin, recorded result)`.
    fn tape_of(
        source: &str,
        io: &[(&str, serde_json::Value)],
        end_error: Option<&str>,
    ) -> String {
        use serde_json::json;
        let no_args = crate::replay::args_digest(&[]).expect("empty argument digest");
        let mut out = vec![json!({
            "t": "header",
            "version": crate::replay::TRACE_VERSION,
            "source": source,
            "source_hash": crate::replay::source_hash(source),
            "seed": 7,
        })
        .to_string()];
        for (index, (builtin, result)) in io.iter().enumerate() {
            out.push(
                json!({
                    "t": "io", "f": index, "s": index, "b": builtin, "a": no_args,
                    "r": result,
                })
                .to_string(),
            );
        }
        let outcome = match end_error {
            Some(error) => json!({"error": error}),
            None => json!({"ok": true}),
        };
        out.push(json!({"t": "end", "world": "unused", "outcome": outcome}).to_string());
        out.join("\n") + "\n"
    }

    fn output(text: &str) -> BlameOptions {
        BlameOptions {
            symptom: Some(Symptom::Output(text.into())),
            ..BlameOptions::default()
        }
    }

    fn classes(report: &BlameReport, class: ButFor) -> Vec<usize> {
        report
            .inputs
            .iter()
            .filter(|finding| finding.but_for == Some(class))
            .map(|finding| finding.unit.ordinal)
            .collect()
    }

    #[test]
    fn finds_the_two_inputs_that_interact_among_noise() {
        let lines = ["noise", "arm", "noise", "noise", "door", "noise", "done"];
        let report = blame(&tape(ALARM, &lines, None), None, &output("ALARM")).expect("blame");
        assert_eq!(report.minimal_cause, vec![2, 5], "arm and door");
        assert_eq!(classes(&report, ButFor::Necessary), vec![2, 5]);
        assert_eq!(classes(&report, ButFor::Incidental), vec![1, 3, 4, 6, 7]);
        assert_eq!(report.minimal_script().as_deref(), Some("arm\ndoor\n"));
        assert!(!report.occurs_without_input);
    }

    #[test]
    fn the_minimal_script_reproduces_and_each_member_is_required() {
        let lines = ["window", "arm", "disarm", "arm", "noise", "window", "done"];
        let options = output("ALARM");
        let report = blame(&tape(ALARM, &lines, None), None, &options).expect("blame");
        let script = report.minimal_script().expect("text inputs");
        let kept: Vec<&str> = script.lines().collect();
        // Independent of the search: replay the claimed cause, then every
        // one-smaller history, from fresh tapes.
        let alone = blame(&tape(ALARM, &kept, None), None, &options).expect("cause reproduces");
        assert_eq!(
            alone.minimal_cause.len(),
            kept.len(),
            "nothing left to drop"
        );
        for skip in 0..kept.len() {
            let mut fewer = kept.clone();
            fewer.remove(skip);
            let error = blame(&tape(ALARM, &fewer, None), None, &options)
                .expect_err("one input fewer must lose the symptom");
            assert!(
                error.contains("does not occur in the recorded session"),
                "{error}"
            );
        }
    }

    #[test]
    fn overdetermined_symptom_has_no_necessary_sensor_but_a_minimal_cause() {
        // Door and window each trip the armed alarm: withholding either one
        // alone changes nothing, so but-for calls both incidental.
        let lines = ["arm", "door", "window", "done"];
        let report = blame(&tape(ALARM, &lines, None), None, &output("ALARM")).expect("blame");
        assert_eq!(classes(&report, ButFor::Necessary), vec![1], "only arm");
        assert_eq!(classes(&report, ButFor::Incidental), vec![2, 3, 4]);
        assert_eq!(report.minimal_cause.len(), 2, "arm plus one sensor");
        assert!(report.minimal_cause.contains(&1));
        let text = report.render_text("alarm.radr");
        assert!(text.contains("Overdetermined"), "{text}");
    }

    #[test]
    fn an_input_that_prevents_the_symptom_is_never_in_its_cause() {
        // The second arm is what matters: the first one was disarmed.
        let lines = ["arm", "disarm", "door", "arm", "window", "done"];
        let report = blame(&tape(ALARM, &lines, None), None, &output("ALARM")).expect("blame");
        assert_eq!(report.minimal_cause, vec![4, 5]);
    }

    #[test]
    fn explains_the_recorded_error_when_no_symptom_is_given() {
        let lines = ["arm", "noise", "door"];
        let trace = tape(ALARM, &lines, Some("alarm tripped"));
        let report = blame(&trace, None, &BlameOptions::default()).expect("blame");
        assert_eq!(report.symptom, Symptom::Error("alarm tripped".into()));
        assert_eq!(report.minimal_cause, vec![1, 3]);
    }

    #[test]
    fn refuses_a_symptom_the_recorded_session_does_not_show() {
        let trace = tape(ALARM, &["arm", "done"], None);
        let error = blame(&trace, None, &output("ALARM")).expect_err("no alarm");
        assert!(
            error.contains("does not occur in the recorded session"),
            "{error}"
        );
        let error = blame(&trace, None, &BlameOptions::default()).expect_err("no symptom");
        assert!(error.contains("say which outcome to explain"), "{error}");
    }

    #[test]
    fn a_symptom_that_needs_no_input_blames_no_input() {
        let source = "print(\"BOOT FAILED\")\nlet line = readline()\n";
        let report = blame(
            &tape(source, &["anything"], None),
            None,
            &output("BOOT FAILED"),
        )
        .expect("blame");
        assert!(report.occurs_without_input);
        assert!(report.minimal_cause.is_empty());
        assert!(report
            .render_text("boot.radr")
            .contains("No input causes it"));
    }

    #[test]
    fn a_program_that_never_sees_end_of_input_is_structural_not_a_hang() {
        // No "" check: with "done" withheld the loop would read forever.
        let source = r#"
let mut seen = 0
let mut running = true
while running {
    let line = readline()
    if line == "done" { running = false } else { seen = seen + 1 }
}
if seen >= 2 { print("TWO") }
"#;
        let lines = ["a", "b", "done"];
        let report = blame(&tape(source, &lines, None), None, &output("TWO")).expect("blame");
        assert_eq!(classes(&report, ButFor::Structural), vec![3]);
        assert_eq!(report.minimal_cause, vec![1, 2, 3]);
    }

    #[test]
    fn result_does_not_depend_on_worker_count_or_strategy() {
        let lines = [
            "noise", "door", "arm", "noise", "disarm", "noise", "arm", "noise", "noise", "window",
            "noise", "door", "done",
        ];
        let trace = tape(ALARM, &lines, None);
        let serial = blame(&trace, None, &output("ALARM")).expect("serial");
        let parallel = blame(
            &trace,
            None,
            &BlameOptions {
                jobs: 4,
                ..output("ALARM")
            },
        )
        .expect("parallel");
        assert_eq!(serial.minimal_cause, parallel.minimal_cause);
        assert_eq!(
            serial.replays, parallel.replays,
            "same histories, any thread count"
        );
        assert_eq!(serial.to_json()["inputs"], parallel.to_json()["inputs"]);

        let plain = blame(
            &trace,
            None,
            &BlameOptions {
                strategy: Strategy::Ddmin,
                but_for: false,
                ..output("ALARM")
            },
        )
        .expect("ddmin");
        // Both are 1-minimal; this session has two minimal causes that share
        // the second arm, so compare sizes and the shared member.
        assert_eq!(plain.minimal_cause.len(), serial.minimal_cause.len());
        assert!(plain.minimal_cause.contains(&7) && serial.minimal_cause.contains(&7));
    }

    /// A checkout that times out when `pay` comes long after the previous
    /// command. Each command reads the clock once.
    const CHECKOUT: &str = r#"
let mut last = 0
let mut running = true
while running {
    let line = trim(readline())
    if line == "done" or line == "" {
        running = false
    } else {
        let at = now_unix_ms()
        if line == "pay" and last != 0 and at - last > 150 { print("TIMEOUT") }
        last = at
    }
}
"#;

    #[test]
    fn a_withheld_command_does_not_shift_the_clock_of_later_ones() {
        use serde_json::json;
        let line = |text: &str| ("readline", json!({"t": "str", "v": text}));
        let clock = |ms: i64| ("now_unix_ms", json!({"t": "int", "v": ms}));
        let io = [
            line("login"),
            clock(1000),
            line("browse"),
            clock(1041),
            line("pay"),
            clock(1369),
            line("done"),
        ];
        let report = blame(&tape_of(CHECKOUT, &io, None), None, &output("TIMEOUT")).unwrap();
        // `pay` arrived 328 ms after `browse` and 369 ms after `login`. With
        // either earlier command withheld, `pay` still reads 1369 and still
        // times out against the other. Served in call order it would read
        // 1041 instead, and both would be reported as necessary.
        assert_eq!(classes(&report, ButFor::Necessary), vec![3], "{report:?}");
        assert_eq!(classes(&report, ButFor::Incidental), vec![1, 2, 4]);
        assert_eq!(report.minimal_cause, vec![1, 3]);
    }

    #[test]
    fn a_reading_taken_under_a_withheld_command_still_answers_later_ones() {
        use serde_json::json;
        // The first command of a session stamps its start; later commands
        // only read the stamp.
        let source = r#"
let mut started = 0
let mut running = true
while running {
    let line = trim(readline())
    if line == "done" or line == "" {
        running = false
    } else {
        if started == 0 { started = now_unix_s() }
        if line == "check" and started > 100 { print("LATE") }
    }
}
"#;
        let io = [
            ("readline", json!({"t": "str", "v": "hello"})),
            ("now_unix_s", json!({"t": "int", "v": 500})),
            ("readline", json!({"t": "str", "v": "check"})),
            ("readline", json!({"t": "str", "v": "done"})),
        ];
        let report = blame(&tape_of(source, &io, None), None, &output("LATE")).unwrap();
        // Without `hello`, `check` is the command that stamps the start. The
        // tape holds no reading under `check`, so the nearest earlier one
        // answers; the history is a session, not a hole in the recording.
        assert_eq!(classes(&report, ButFor::Structural), Vec::<usize>::new());
        assert_eq!(report.minimal_cause, vec![2], "{report:?}");
    }

    #[test]
    fn the_replay_budget_is_enforced() {
        let lines = ["noise", "arm", "noise", "door", "done"];
        let error = blame(
            &tape(ALARM, &lines, None),
            None,
            &BlameOptions {
                max_replays: 3,
                ..output("ALARM")
            },
        )
        .expect_err("budget");
        assert!(error.contains("--max-replays"), "{error}");
    }

    #[test]
    fn lineage_names_the_writer_and_intervention_adds_the_value_it_read() {
        // The total is written when "total" arrives; its handler reads the
        // amount an earlier line stored. Lineage follows the write.
        let source = r#"
component Line { amount: int = 0 }
resource Books { total: int = 0 }
event Totalled { by: str }
on Totalled(_event) {
    let line = get(get_entity("line"), Line) |> unwrap
    set_resource(Books, Books { total: line.amount })
}
let _ = spawn("line", Line { amount: 0 })
let mut running = true
while running {
    let text = trim(readline())
    if text == "done" or text == "" {
        running = false
    } else if text == "total" {
        emit Totalled { by: "clerk" }
    } else if text == "big" {
        set(get_entity("line"), Line { amount: 5000 })
    }
    flush_events()
}
let books = get_resource(Books) |> unwrap
if books.total > 1000 { print("OVER LIMIT") }
"#;
        let lines = ["noise", "big", "noise", "total", "done"];
        // `tape` numbers frames as the recorder would here: the program
        // flushes once per line, so line `i` is read in frame `i`.
        let trace = tape(source, &lines, None);
        let report = blame(
            &trace,
            None,
            &BlameOptions {
                lineage: Some(LineageTarget::Resource("Books".into())),
                ..output("OVER LIMIT")
            },
        )
        .expect("blame");
        assert_eq!(report.minimal_cause, vec![2, 4], "big and total");
        let lineage = report.lineage.as_ref().expect("lineage");
        assert!(
            lineage.explanation.contains("Totalled"),
            "{}",
            lineage.explanation
        );
        assert_eq!(
            lineage.inputs,
            vec![4],
            "why() names only the line that wrote"
        );
        let text = report.render_text("books.radr");
        assert!(
            text.contains("Not in the lineage, yet required: #2"),
            "{text}"
        );
    }
}
