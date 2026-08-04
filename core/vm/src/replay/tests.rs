

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::VM;

    fn record_run_raw(src: &str) -> (Result<(), String>, Vec<serde_json::Value>) {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        let result = Compiler::new().compile(&program).expect("compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.set_random_seed(7);
        vm.enable_recording(src);
        vm.load_compile_result(result);
        let run_result = vm.run(0);
        let trace = vm.take_trace().expect("trace");
        let lines = trace
            .lines()
            .map(|l| serde_json::from_str(l).expect("valid json line"))
            .collect();
        (run_result, lines)
    }

    fn record_run(src: &str) -> Vec<serde_json::Value> {
        let (result, lines) = record_run_raw(src);
        result.expect("run");
        lines
    }

    #[test]
    fn codec_roundtrips_every_data_kind() {
        let mut vm = VM::new();
        let gc = &mut vm.gc;

        let s = Value::from_string(gc, "hello \"quoted\"\nline".to_string());
        let li = {
            let one = Value::from_int(gc, 1);
            let pi = Value::from_float(3.5);
            Value::list(gc, vec![one, pi, Value::NIL, Value::from_bool(true)])
        };
        let m = {
            let mut storage = MapStorage::new();
            let v1 = Value::from_int(gc, 10);
            let v2 = Value::from_int(gc, 20);
            storage.insert(MapKey::Str("a".into()), v1);
            storage.insert(MapKey::Int(5), v2);
            Value::map(gc, storage)
        };
        let st = {
            let mut fields = HashMap::new();
            let inner = Value::from_int(gc, 99);
            fields.insert("value".to_string(), inner);
            Value::sum_type(gc, "Result".into(), "Ok".into(), fields)
        };
        let comp = {
            let x = Value::from_int(gc, 4);
            Value::component(
                gc,
                "Pos".into(),
                std::sync::Arc::new(vec!["x".to_string()]),
                vec![x],
            )
        };
        let ent = Value::from_entity_id(gc, 3);

        for v in [s, li, m, st, comp, ent] {
            let encoded = encode_value(&v).expect("encode");
            let decoded = decode_value(&mut vm.gc, &encoded).expect("decode");
            assert_eq!(
                format!("{}", v),
                format!("{}", decoded),
                "roundtrip changed value: {}",
                encoded
            );
        }
    }

    #[test]
    fn trace_entity_values_and_map_keys_reject_u32_overflow() {
        let mut vm = VM::new();
        for overflow in [u32::MAX as u64 + 1, u64::MAX] {
            let entity = serde_json::json!({"t": "entity", "v": overflow});
            let error = decode_value(&mut vm.gc, &entity).expect_err("entity overflow");
            assert!(error.contains("entity id exceeds u32"), "{error}");

            let map = serde_json::json!({
                "t": "map",
                "v": [[["e", overflow], {"t": "nil"}]]
            });
            let error = decode_value(&mut vm.gc, &map).expect_err("map key overflow");
            assert!(error.contains("entity map key exceeds u32"), "{error}");
        }
    }

    #[test]
    fn recorder_captures_io_and_frames_but_not_pure_builtins() {
        let dir = std::env::temp_dir().join("rad_replay_p1_test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("data.txt").to_string_lossy().replace('\\', "/");
        std::fs::write(dir.join("data.txt"), "payload").unwrap();

        let src = format!(
            r#"
            event Tick {{ n }}
            on Tick(e) {{
                print(e.n)
            }}
            print(rand_int(1, 100))
            let c = clock()
            let body = read_file("{file}")
            emit Tick {{ n: 1 }}
            flush_events()
            let c2 = now_unix_ms()
            print(len(str(c) + str(c2)))
            "#
        );
        let lines = record_run(&src);

        assert_eq!(lines[0]["t"], "header");
        assert_eq!(lines[0]["version"], 1);
        assert_eq!(lines[0]["seed"], 7);
        assert_eq!(lines[0]["source_hash"], source_hash(&src));
        assert_eq!(lines[0]["features"], serde_json::json!([]));

        let ios: Vec<&serde_json::Value> = lines.iter().filter(|l| l["t"] == "io").collect();
        let names: Vec<&str> = ios.iter().map(|l| l["b"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec!["clock", "read_file", "now_unix_ms"],
            "exactly the boundary-crossing builtins, in program order"
        );
        // rand_int and print must not appear: pure xorshift / deterministic output.

        // read_file result is the tagged file payload.
        let rf = ios[1];
        assert_eq!(rf["r"]["t"], "str");
        assert_eq!(rf["r"]["v"], "payload");
        assert_eq!(rf["a"].as_str().unwrap().len(), 16);

        // Frame accounting: clock + read_file land in frame 0, the
        // now_unix_ms after flush_events lands in frame 1 with seq reset.
        assert_eq!(ios[0]["f"], 0);
        assert_eq!(ios[0]["s"], 0);
        assert_eq!(ios[1]["f"], 0);
        assert_eq!(ios[1]["s"], 1);
        assert_eq!(ios[2]["f"], 1);
        assert_eq!(ios[2]["s"], 0);

        let frames: Vec<&serde_json::Value> = lines.iter().filter(|l| l["t"] == "frame").collect();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["n"], 0);
    }

    #[test]
    fn trace_preserves_and_authenticates_language_features() {
        let features = vec!["causal_laws".to_string()];
        let recorder = TraceRecorder::new_with_features("settle {}", 7, &features);
        let trace = recorder.to_jsonl();
        let parsed = TraceReplayer::parse(&trace, false).expect("feature-bearing trace");
        assert_eq!(parsed.features(), features);

        let mut lines = trace.lines();
        let mut header: serde_json::Value =
            serde_json::from_str(lines.next().unwrap()).expect("header json");
        header["features"] = serde_json::json!([]);
        let tampered = std::iter::once(header.to_string())
            .chain(lines.map(str::to_string))
            .collect::<Vec<_>>()
            .join("\n");
        let error = TraceReplayer::parse(&tampered, false)
            .expect_err("feature tampering must invalidate the trace");
        assert!(error.contains("feature_hash"), "{error}");
    }

    #[test]
    fn terminal_outcome_is_verified_independently_of_world_digest() {
        let mut recorder = TraceRecorder::new("nil", 7);
        recorder.record_end_with_outcome("unchanged", None);
        let trace = recorder.to_jsonl();
        let replayer = TraceReplayer::parse(&trace, false).expect("trace");
        let report = replayer.report_with_outcome("unchanged", Some("unexpected crash"));
        assert_eq!(report.end_digest_match, Some(true));
        assert_eq!(report.end_outcome_match, Some(false));

        let replayer = TraceReplayer::parse(&trace, false).expect("trace");
        assert_eq!(
            replayer
                .report_with_outcome("unchanged", None)
                .end_outcome_match,
            Some(true)
        );
    }

    #[test]
    fn recorder_captures_io_failures_as_errors() {
        // read_file on a missing path is a hard VM error. The trace must
        // still capture it so replay can reproduce the same failure without
        // touching the file system.
        let src = r#"
            let missing = read_file("h:/definitely/not/a/real/path/x.txt")
            print(missing)
        "#;
        let (result, lines) = record_run_raw(src);
        assert!(result.is_err(), "missing file must error");
        let ios: Vec<&serde_json::Value> = lines.iter().filter(|l| l["t"] == "io").collect();
        assert_eq!(ios.len(), 1);
        assert_eq!(ios[0]["b"], "read_file");
        assert!(
            ios[0].get("r").is_none(),
            "failed io must not record a result"
        );
        assert!(
            ios[0]["e"].as_str().unwrap().contains("read_file() failed"),
            "error text must be captured: {}",
            ios[0]
        );
    }

    fn compile_and_make_vm(src: &str) -> VM {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        let result = Compiler::new().compile(&program).expect("compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.load_compile_result(result);
        vm
    }

    /// Record a run, then replay its trace in a fresh VM. Returns
    /// (recorded prints, recorded digest, replayed prints, replayed digest, report).
    fn roundtrip(src: &str) -> (Vec<String>, String, Vec<String>, String, ReplayReport) {
        let mut rec_vm = compile_and_make_vm(src);
        rec_vm.enable_recording(src);
        rec_vm.run(0).expect("recorded run");
        let rec_prints = rec_vm.print_buffer.clone();
        let rec_digest = rec_vm.world.content_digest();
        let trace = rec_vm.take_trace().expect("trace");

        let replayer = TraceReplayer::parse(&trace, false).expect("parse trace");
        let mut rep_vm = compile_and_make_vm(replayer.source());
        rep_vm.enable_replay(replayer);
        rep_vm.run(0).expect("replayed run");
        let rep_prints = rep_vm.print_buffer.clone();
        let rep_digest = rep_vm.world.content_digest();
        let report = rep_vm.finish_replay().expect("report");
        (rec_prints, rec_digest, rep_prints, rep_digest, report)
    }

    const ROUNDTRIP_SRC_TEMPLATE: &str = r#"
        component Pos { x: 0 }
        resource Score { total: 0 }
        event Bump { who }
        on Bump(e) {
            let p = get(e.who, Pos) |> unwrap
            set(e.who, Pos { x: p.x + 100 })
        }
        let hero = spawn("hero", Pos { x: rand_int(1, 40) })
        let started = clock()
        let cfg = read_file("__FILE__")
        print(cfg)
        emit Bump { who: hero }
        flush_events()
        set_resource(Score, Score { total: rand_int(1, 1000000) })
        flush_events()
        print((get(hero, Pos) |> unwrap).x)
        print(clock() >= started)
    "#;

    /// Each caller passes a unique subdir: tests run in parallel and one of
    /// them deletes its file mid-test, so sharing a path is a race.
    fn roundtrip_src(subdir: &str) -> String {
        let dir = std::env::temp_dir().join(subdir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("cfg.txt"), "cfg-payload").unwrap();
        let file = dir.join("cfg.txt").to_string_lossy().replace('\\', "/");
        ROUNDTRIP_SRC_TEMPLATE.replace("__FILE__", &file)
    }

    #[test]
    fn replay_roundtrip_is_byte_identical() {
        let src = roundtrip_src("rad_replay_p2_roundtrip");
        let (rec_prints, rec_digest, rep_prints, rep_digest, report) = roundtrip(&src);
        assert_eq!(rec_prints, rep_prints, "print buffers must match exactly");
        assert_eq!(rec_digest, rep_digest, "world digests must match exactly");
        assert_eq!(report.end_digest_match, Some(true));
        assert_eq!(report.leftover_io, 0);
        assert_eq!(report.frames_replayed, 2);
        assert_eq!(report.io_replayed, 3, "clock, read_file, clock");
    }

    #[test]
    fn replay_never_refires_io() {
        // Record with the file present, then DELETE it. Replay must still
        // produce the recorded payload: io is served from the trace, never
        // re-executed.
        let src = roundtrip_src("rad_replay_p2_refire");
        let mut rec_vm = compile_and_make_vm(&src);
        rec_vm.enable_recording(&src);
        rec_vm.run(0).expect("recorded run");
        let trace = rec_vm.take_trace().expect("trace");

        std::fs::remove_file(
            std::env::temp_dir()
                .join("rad_replay_p2_refire")
                .join("cfg.txt"),
        )
        .expect("delete the file out from under the replay");

        let replayer = TraceReplayer::parse(&trace, false).expect("parse");
        let mut rep_vm = compile_and_make_vm(replayer.source());
        rep_vm.enable_replay(replayer);
        rep_vm
            .run(0)
            .expect("replay must not touch the file system");
        assert_eq!(rep_vm.print_buffer[0], "cfg-payload");
    }

    #[test]
    fn replay_halts_on_digest_divergence() {
        let src = roundtrip_src("rad_replay_p2_divergence");
        let mut rec_vm = compile_and_make_vm(&src);
        rec_vm.enable_recording(&src);
        rec_vm.run(0).expect("recorded run");
        let trace = rec_vm.take_trace().expect("trace");

        // Tamper with one recorded args digest: simulates the replayed run
        // computing different io arguments than the recorded one.
        let mut lines: Vec<serde_json::Value> = trace
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let idx = lines
            .iter()
            .position(|l| l["t"] == "io" && l["b"] == "read_file")
            .expect("read_file record");
        lines[idx]["a"] = serde_json::Value::String("deadbeefdeadbeef".into());
        let tampered = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(trace.trim(), tampered, "tamper must change the trace");

        let replayer = TraceReplayer::parse(&tampered, false).expect("parse");
        let mut rep_vm = compile_and_make_vm(replayer.source());
        rep_vm.enable_replay(replayer);
        let err = rep_vm.run(0).expect_err("divergence must halt the replay");
        assert!(err.contains("replay divergence"), "got: {}", err);
        assert!(err.contains("read_file"), "got: {}", err);
    }

    #[test]
    fn tampered_source_is_refused_without_force() {
        let src = roundtrip_src("rad_replay_p2_tamper");
        let mut rec_vm = compile_and_make_vm(&src);
        rec_vm.enable_recording(&src);
        rec_vm.run(0).expect("recorded run");
        let trace = rec_vm.take_trace().expect("trace");

        let tampered = trace.replace("rand_int(1, 40)", "rand_int(1, 41)");
        assert_ne!(trace, tampered);

        let err = TraceReplayer::parse(&tampered, false).expect_err("must refuse");
        assert!(err.contains("integrity"), "got: {}", err);
        assert!(err.contains("--force"), "got: {}", err);
        // --force overrides the refusal (and the divergence machinery is
        // still armed downstream).
        assert!(TraceReplayer::parse(&tampered, true).is_ok());
    }

    #[test]
    fn replay_reproduces_recorded_io_failures() {
        let src = r#"
            let t = clock()
            let missing = read_file("h:/definitely/not/a/real/path/x.txt")
            print(missing)
        "#;
        let mut rec_vm = compile_and_make_vm(src);
        rec_vm.enable_recording(src);
        let rec_err = rec_vm.run(0).expect_err("recorded run crashes");
        let trace = rec_vm.take_trace().expect("trace");

        let replayer = TraceReplayer::parse(&trace, false).expect("parse");
        let mut rep_vm = compile_and_make_vm(replayer.source());
        rep_vm.enable_replay(replayer);
        let rep_err = rep_vm.run(0).expect_err("replay reproduces the crash");
        assert_eq!(rec_err, rep_err, "the crash must be reproduced verbatim");
        let report = rep_vm.finish_replay().expect("report");
        assert_eq!(
            report.end_digest_match,
            Some(true),
            "world at crash point must match"
        );
    }

    #[test]
    fn stop_at_frame_halts_at_frame_start() {
        let src = r#"
            resource Counter { n: 0 }
            event Tick {}
            on Tick(_e) {
                let c = get_resource(Counter) |> unwrap
                set_resource(Counter, Counter { n: c.n + 1 })
            }
            for _i in range(0, 5) {
                emit Tick {}
                flush_events()
            }
        "#;
        let mut rec_vm = compile_and_make_vm(src);
        rec_vm.enable_recording(src);
        rec_vm.run(0).expect("recorded run");
        let trace = rec_vm.take_trace().expect("trace");

        let mut replayer = TraceReplayer::parse(&trace, false).expect("parse");
        replayer.stop_at(2);
        let mut rep_vm = compile_and_make_vm(src);
        rep_vm.enable_replay(replayer);
        let err = rep_vm.run(0).expect_err("stop sentinel");
        // The VM decorates propagated errors with call-site context, so the
        // sentinel is matched with contains(), not starts_with().
        assert!(err.contains(REPLAY_STOP_PREFIX), "got: {}", err);
        // Handlers dispatched by flush #k belong to frame k: frame 0 only
        // emitted, frame 1 dispatched the first Tick. Stopping at the start
        // of frame 2 therefore shows exactly ONE tick applied.
        let mut twin = compile_and_make_vm(
            "resource Counter { n: 0 }\nset_resource(Counter, Counter { n: 1 })",
        );
        twin.run(0).expect("twin");
        assert_eq!(
            rep_vm.world.content_digest(),
            twin.world.content_digest(),
            "state at start of frame 2 must show exactly 1 dispatched tick"
        );
        let report = rep_vm.finish_replay().expect("report");
        assert_eq!(report.frames_replayed, 2);
    }

    #[test]
    fn seek_frame_repositions_the_io_cursor() {
        // Build a trace with io in frames 0, 1, and 3 (none in 2).
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_io(
            "clock",
            "d0".into(),
            &Ok(serde_json::json!({"t":"int","v":0})),
        );
        rec.record_frame(u64::MAX); // -> frame 1
        rec.record_io(
            "clock",
            "d1".into(),
            &Ok(serde_json::json!({"t":"int","v":1})),
        );
        rec.record_io(
            "clock",
            "d2".into(),
            &Ok(serde_json::json!({"t":"int","v":2})),
        );
        rec.record_frame(u64::MAX); // -> frame 2
        rec.record_frame(u64::MAX); // -> frame 3
        rec.record_io(
            "clock",
            "d3".into(),
            &Ok(serde_json::json!({"t":"int","v":3})),
        );
        let jsonl = rec.to_jsonl();

        let mut rep = TraceReplayer::parse(&jsonl, false).expect("parse");
        // Seek into frame 1: next io must be d1.
        rep.seek_frame(1);
        let r = rep.next_io("clock", "d1").expect("d1");
        assert_eq!(r.seq, 0);
        // Seek to frame 2 (no io): cursor lands on frame 3's record.
        rep.seek_frame(2);
        rep.advance_frame(); // 2 -> 3
        let r = rep.next_io("clock", "d3").expect("d3");
        assert_eq!(r.frame, 3);
        // Seek past the end: any further io is a divergence.
        rep.seek_frame(99);
        assert!(rep.next_io("clock", "dX").is_err());
    }

    #[test]
    fn retro_oracle_fifo_repeat_last_and_holes() {
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_io(
            "clock",
            "dX".into(),
            &Ok(serde_json::json!({"t":"int","v":1})),
        );
        rec.record_io(
            "clock",
            "dX".into(),
            &Ok(serde_json::json!({"t":"int","v":2})),
        );
        let mut rep = TraceReplayer::parse(&rec.to_jsonl(), false)
            .expect("parse")
            .into_retro();

        // FIFO per key: same question, answers in recorded order.
        assert_eq!(rep.next_io("clock", "dX").unwrap().result.unwrap()["v"], 1);
        assert_eq!(rep.next_io("clock", "dX").unwrap().result.unwrap()["v"], 2);
        // Exhausted key: repeatable read of the last value.
        assert_eq!(rep.next_io("clock", "dX").unwrap().result.unwrap()["v"], 2);
        assert_eq!(rep.report("x").reused_reads, 1);
        // Never-recorded key: a hole, loud.
        let err = rep.next_io("read_file", "dY").expect_err("hole");
        assert!(err.contains("hole"), "got: {}", err);
        assert!(err.contains("read_file"), "got: {}", err);
    }

    /// A4 BUG 06: `--to-frame 0` and any N beyond the last recorded frame
    /// boundary used to be silently dropped — the whole trace ran and the
    /// tool printed "Replay verified" for a request it did not honour.
    #[test]
    fn to_frame_range_is_validated_against_the_trace() {
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_frame(u64::MAX);
        rec.record_frame(u64::MAX);
        rec.record_frame(u64::MAX);
        let rep = TraceReplayer::parse(&rec.to_jsonl(), false).expect("parse");
        assert_eq!(rep.total_frames(), 3);
        // 0 (run nothing) through 3 (the last boundary) are honest stops.
        for n in 0..=3 {
            assert!(rep.validate_stop_frame(n).is_ok(), "n={}", n);
        }
        for n in [4u64, 50, 100000] {
            let err = rep.validate_stop_frame(n).expect_err("out of range");
            assert!(err.contains(&format!("--to-frame {}", n)), "got: {}", err);
            assert!(err.contains("3 frame boundaries"), "got: {}", err);
            assert!(err.contains("0..=3"), "got: {}", err);
        }
        // A trace that never flushed has no boundary to stop at.
        let empty = TraceRecorder::new("src", 1);
        let rep = TraceReplayer::parse(&empty.to_jsonl(), false).expect("parse");
        assert_eq!(rep.total_frames(), 0);
        assert!(rep.validate_stop_frame(0).is_ok());
        assert!(rep.validate_stop_frame(1).is_err());
    }

    /// A4 BUG 05 (defect 1): an edited program whose write_file carries a
    /// different payload used to halt as an "oracle hole". Writes consume
    /// nothing from the recorded world — they replay as virtualized no-ops.
    #[test]
    fn retro_virtualizes_changed_writes_instead_of_halting() {
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_io(
            "write_file",
            "d-old".into(),
            &Ok(serde_json::json!({"t":"nil"})),
        );
        let mut rep = TraceReplayer::parse(&rec.to_jsonl(), false)
            .expect("parse")
            .into_retro();

        // Same args as recorded: served from the oracle, not virtualized.
        assert!(rep.next_io("write_file", "d-old").unwrap().result.is_ok());
        // Changed payload: virtualized success (nil), not a hole.
        let rec2 = rep
            .next_io("write_file", "d-new")
            .expect("virtualized write");
        assert_eq!(rec2.result.unwrap()["t"], "nil");
        // A write the recording never performed at all is also fine.
        assert!(rep.next_io("append_file", "d-x").is_ok());
        let report = rep.report("x");
        assert_eq!(report.virtual_writes, 2);
        assert_eq!(report.io_replayed, 1, "only the oracle hit consumes");
    }

    /// A4 BUG 05 (defect 2): the hole diagnostic claimed "the recorded
    /// session never performed that io" even when the same builtin WAS
    /// recorded with different arguments. The two cases now read differently.
    #[test]
    fn retro_hole_diagnostic_distinguishes_changed_args_from_never_called() {
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_io(
            "read_file",
            "d-a".into(),
            &Ok(serde_json::json!({"t":"str","v":"x"})),
        );
        let mut rep = TraceReplayer::parse(&rec.to_jsonl(), false)
            .expect("parse")
            .into_retro();

        let err = rep.next_io("read_file", "d-b").expect_err("changed args");
        assert!(
            err.contains("called read_file() 1 time(s) but never with these arguments"),
            "got: {}",
            err
        );
        let err = rep.next_io("http_get", "d-c").expect_err("never called");
        assert!(err.contains("never called it"), "got: {}", err);
        assert!(!err.contains("never performed that io"), "got: {}", err);
    }

    #[test]
    fn retro_replay_serves_recorded_io_to_edited_code() {
        // Record with the original source, DELETE the file, then replay an
        // EDITED source (different spawn count) against the recorded inputs.
        let dir = std::env::temp_dir().join("rad_replay_p6_test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("cfg.txt"), "retro-payload").unwrap();
        let file = dir.join("cfg.txt").to_string_lossy().replace('\\', "/");

        let original = format!(
            r#"
            component Pos {{ x: 0 }}
            let a = spawn("a", Pos {{ x: 1 }})
            print(read_file("{file}"))
            flush_events()
            "#
        );
        let edited = format!(
            r#"
            component Pos {{ x: 0 }}
            let a = spawn("a", Pos {{ x: 1 }})
            let b = spawn("b", Pos {{ x: 2 }})
            print(read_file("{file}"))
            print(read_file("{file}"))
            flush_events()
            "#
        );

        let mut rec_vm = compile_and_make_vm(&original);
        rec_vm.enable_recording(&original);
        rec_vm.run(0).expect("recorded run");
        let world_a = rec_vm.world.snapshot();
        let trace = rec_vm.take_trace().expect("trace");

        std::fs::remove_file(dir.join("cfg.txt")).expect("delete file");

        let retro = TraceReplayer::parse(&trace, false)
            .expect("parse")
            .into_retro();
        let mut vm_b = compile_and_make_vm(&edited);
        vm_b.enable_replay(retro);
        vm_b.run(0).expect("retro run");
        // Both reads served from the oracle (second via repeatable read),
        // even though the file is gone.
        assert_eq!(vm_b.print_buffer, vec!["retro-payload", "retro-payload"]);
        let report = vm_b.finish_replay().expect("report");
        assert_eq!(report.reused_reads, 1);
        // The edit's blast radius is visible as a world diff: row "a" is
        // value-identical across both runs, only the new spawn counts.
        let world_b = vm_b.world.snapshot();
        let diff = crate::world::WorldSnapshot::diff_summary(&world_a, &world_b);
        assert_eq!(diff.get("Pos"), Some(&1usize), "diff: {:?}", diff);
    }

    #[test]
    fn retro_replay_shows_the_fix_blast_radius() {
        // The gold-drain bug from the Phase 3 dogfood: record the buggy run,
        // then retroactively replay the FIXED handler. The diff between the
        // two final worlds is exactly the bug's footprint.
        let buggy = r#"
            component Health { hp: 100 }
            component Gold { amount: 50 }
            event Hit { amount }
            let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 50 })
            on Hit(e) {
                let h = get(hero, Health) |> unwrap
                set(hero, Health { hp: h.hp - e.amount })
                if h.hp - e.amount < 80 {
                    set(hero, Gold { amount: 0 })
                }
            }
            for _i in range(0, 4) {
                emit Hit { amount: 10 }
                flush_events()
            }
        "#;
        let fixed = r#"
            component Health { hp: 100 }
            component Gold { amount: 50 }
            event Hit { amount }
            let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 50 })
            on Hit(e) {
                let h = get(hero, Health) |> unwrap
                set(hero, Health { hp: h.hp - e.amount })
            }
            for _i in range(0, 4) {
                emit Hit { amount: 10 }
                flush_events()
            }
        "#;
        let mut rec_vm = compile_and_make_vm(buggy);
        rec_vm.enable_recording(buggy);
        rec_vm.run(0).expect("recorded run");
        let world_buggy = rec_vm.world.snapshot();
        let trace = rec_vm.take_trace().expect("trace");

        let retro = TraceReplayer::parse(&trace, false)
            .expect("parse")
            .into_retro();
        let mut vm_fixed = compile_and_make_vm(fixed);
        vm_fixed.enable_replay(retro);
        vm_fixed.run(0).expect("retro run");

        let world_fixed = vm_fixed.world.snapshot();
        let diff = crate::world::WorldSnapshot::diff_summary(&world_buggy, &world_fixed);
        // Health histories are identical; only the drained Gold differs.
        assert_eq!(diff.get("Gold"), Some(&1usize), "diff: {:?}", diff);
        assert!(!diff.contains_key("Health"), "diff: {:?}", diff);
    }

    #[test]
    fn trace_is_deterministic_across_twin_recorded_runs() {
        let src = r#"
            component Pos { x: 0 }
            let e = spawn("hero", Pos { x: rand_int(1, 50) })
            let t = clock()
            flush_events()
            print(get(e, Pos) |> unwrap)
        "#;
        let run = |_: u32| {
            let lines = record_run(src);
            // Strip the clock's actual reading: wall time legitimately
            // differs. Everything else must match exactly.
            lines
                .into_iter()
                .map(|mut l| {
                    if l["t"] == "io" && l["b"] == "clock" {
                        l["r"] = serde_json::Value::Null;
                    }
                    l.to_string()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(0), run(1));
    }
}
