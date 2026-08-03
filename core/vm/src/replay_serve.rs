//! `rad replay <trace> --serve` — time-travel debugging as a JSON-RPC 2.0
//! API over stdio (list item #2, Phase 3).
//!
//! On startup the server replays the trace once, keyframing the world at
//! every frame boundary (CoW snapshots: O(archetypes) `Arc` bumps each).
//! After that single pass, every query is served from the timeline with no
//! re-execution: `goto_frame` is index movement, `peek` reads a snapshot,
//! and `diff_frames` is the blast-radius diff pointed backwards in time.
//!
//! ```text
//! → {"jsonrpc":"2.0","id":1,"method":"info"}
//! ← {"jsonrpc":"2.0","id":1,"result":{"frames":12,"io_records":3,"verified":true,...}}
//! → {"jsonrpc":"2.0","id":2,"method":"diff_frames","params":{"a":4,"b":5}}
//! ← {"jsonrpc":"2.0","id":2,"result":{"diff":{"Health":1}}}
//! → {"jsonrpc":"2.0","id":3,"method":"goto_frame","params":{"frame":4}}
//! ← {"jsonrpc":"2.0","id":3,"result":{"frame":4,"digest":"ab12…"}}
//! → {"jsonrpc":"2.0","id":4,"method":"peek","params":{"entity":"hero","component":"Health"}}
//! ← {"jsonrpc":"2.0","id":4,"result":{"found":true,"fields":{"hp":40}}}
//! ```
//!
//! Methods: `info`, `goto_frame`, `peek` (at the current or an explicit
//! frame), `diff_frames`, `shutdown`.
//!
//! The bug-bisection loop for an agent: binary-search frames with
//! `diff_frames(k, k+1)` until the frame that mutated the component of
//! interest is found, then `peek` neighbouring frames to confirm the bad
//! transition. Frame addressing: index `k` = world at the start of frame
//! `k`; the highest index is the world at program end (or crash).

use std::io::{BufRead, Write};
use std::sync::Arc;

use serde_json::{json, Value as Json};

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::replay::TraceReplayer;
use crate::vm::VM;
use crate::world::{World, WorldSnapshot};

/// JSON-RPC error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const UNKNOWN_FRAME: i64 = -32001;

pub struct ReplayServer {
    /// `timeline[k]` = world at start of frame `k`; last entry = program end.
    timeline: Vec<Arc<WorldSnapshot>>,
    current: usize,
    io_records: usize,
    /// `Some(true/false)` from the end-digest check, `None` if absent.
    verified: Option<bool>,
    /// The runtime error of the recorded run, when it crashed.
    run_error: Option<String>,
    /// Causality ledger (#4) rebuilt by the replay pass — `why` answers
    /// from it at any frame.
    ledger: crate::causality::CausalityLedger,
}

impl ReplayServer {
    /// Replay the trace once with per-frame keyframing and build the
    /// timeline. A recorded crash is not a setup failure — debugging crashes
    /// is the point — so it lands in `run_error` and the timeline ends at
    /// the crash state.
    pub fn from_trace(trace_text: &str, force: bool) -> Result<Self, String> {
        let mut replayer = TraceReplayer::parse(trace_text, force)?;
        replayer.enable_timeline_capture();
        let io_records = replayer.io_record_count();

        let source = replayer.source().to_string();
        let features = replayer.features().to_vec();
        let source_layout = replayer.source_layout().clone();
        let mut lexer = Lexer::new_with_source_layout(&source, &source_layout)
            .map_err(|error| format!("trace source layout is invalid: {error}"))?;
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        if let Some(e) = parser.errors().first() {
            return Err(format!("embedded source failed to parse: {}", e.message));
        }
        let compile_result = Compiler::new()
            .with_features(features)
            .compile(&program)
            .map_err(|e| format!("embedded source failed to compile: {}", e.message))?;

        let mut vm = VM::new();
        vm.suppress_output();
        vm.enable_replay(replayer);
        vm.load_compile_result(compile_result);
        let run_error = vm.run(0).err();
        let ledger = vm.take_causality_ledger();
        let (timeline, report) = vm
            .finish_replay_session()
            .expect("replayer was installed above");

        Ok(ReplayServer {
            timeline,
            current: 0,
            io_records,
            verified: report.end_digest_match,
            run_error,
            ledger,
        })
    }

    /// Serve line-delimited JSON-RPC until EOF or `shutdown`.
    pub fn serve<R: BufRead, W: Write>(&mut self, reader: R, mut writer: W) -> std::io::Result<()> {
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let (response, keep_going) = self.handle_line(&line);
            writeln!(writer, "{}", response)?;
            writer.flush()?;
            if !keep_going {
                break;
            }
        }
        Ok(())
    }

    /// Handle one request line. Returns (response JSON line, keep_serving).
    pub fn handle_line(&mut self, line: &str) -> (String, bool) {
        let req: Json = match serde_json::from_str(line) {
            Ok(j) => j,
            Err(e) => {
                return (
                    error_response(Json::Null, PARSE_ERROR, &format!("invalid JSON: {}", e)),
                    true,
                )
            }
        };
        let id = req.get("id").cloned().unwrap_or(Json::Null);
        let Some(method) = req.get("method").and_then(|m| m.as_str()) else {
            return (
                error_response(id, INVALID_REQUEST, "missing 'method'"),
                true,
            );
        };
        let params = req.get("params").cloned().unwrap_or(json!({}));

        let (result, keep_going) = match method {
            "info" => (self.info(), true),
            "goto_frame" => (self.goto_frame(&params), true),
            "peek" => (self.peek(&params), true),
            "diff_frames" => (self.diff_frames(&params), true),
            "why" => (self.why(&params), true),
            "shutdown" => (Ok(json!({ "bye": true })), false),
            other => (
                Err((METHOD_NOT_FOUND, format!("unknown method '{}'", other))),
                true,
            ),
        };

        let response = match result {
            Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }).to_string(),
            Err((code, message)) => error_response(id, code, &message),
        };
        (response, keep_going)
    }

    fn info(&self) -> Result<Json, (i64, String)> {
        let mut obj = json!({
            "frames": self.timeline.len().saturating_sub(1),
            "io_records": self.io_records,
            "current": self.current,
            "verified": self.verified,
        });
        if let Some(e) = &self.run_error {
            obj["run_error"] = Json::String(e.clone());
        }
        Ok(obj)
    }

    /// `goto_frame {frame}` — move the cursor. Pure index movement: every
    /// frame is a keyframe.
    fn goto_frame(&mut self, params: &Json) -> Result<Json, (i64, String)> {
        let frame = require_frame(params, "frame")?;
        let snap = self.frame_snapshot(frame)?;
        let digest = snapshot_digest(&snap);
        self.current = frame;
        Ok(json!({ "frame": frame, "digest": digest }))
    }

    /// `peek {entity, component, frame?}` — read one component at the
    /// current (or an explicit) frame. `entity` is a name string or id.
    fn peek(&mut self, params: &Json) -> Result<Json, (i64, String)> {
        let frame = match params.get("frame") {
            Some(_) => require_frame(params, "frame")?,
            None => self.current,
        };
        let snap = self.frame_snapshot(frame)?;
        let component = params.get("component").and_then(|c| c.as_str()).ok_or((
            INVALID_PARAMS,
            "peek: 'component' (string) is required".to_string(),
        ))?;
        let eid = match params.get("entity") {
            Some(Json::String(name)) => match snap.get_entity_by_name(name) {
                Some(id) => id,
                None => return Ok(json!({ "found": false })),
            },
            Some(Json::Number(n)) => n
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or((INVALID_PARAMS, "peek: 'entity' id out of range".to_string()))?,
            _ => {
                return Err((
                    INVALID_PARAMS,
                    "peek: 'entity' (name string or id number) is required".to_string(),
                ))
            }
        };
        match snap.get_component(eid, component) {
            Some(data) => {
                let mut fields = serde_json::Map::with_capacity(data.layout.len());
                for (idx, field) in data.layout.iter().enumerate() {
                    let v = data
                        .values
                        .get(idx)
                        .copied()
                        .unwrap_or(crate::value::Value::NIL);
                    let j =
                        crate::vm::value_to_json(&v, 0).unwrap_or(Json::String(v.print_display()));
                    fields.insert(field.clone(), j);
                }
                Ok(json!({ "frame": frame, "found": true, "fields": Json::Object(fields) }))
            }
            None => Ok(json!({ "frame": frame, "found": false })),
        }
    }

    /// `diff_frames {a, b}` — per-component changed-row counts between two
    /// frames: the blast-radius diff pointed backwards in time.
    fn diff_frames(&mut self, params: &Json) -> Result<Json, (i64, String)> {
        let a = require_frame(params, "a")?;
        let b = require_frame(params, "b")?;
        let snap_a = self.frame_snapshot(a)?;
        let snap_b = self.frame_snapshot(b)?;
        let summary = WorldSnapshot::diff_summary(&snap_a, &snap_b);
        let mut diff = serde_json::Map::with_capacity(summary.len());
        for (name, rows) in summary {
            diff.insert(name, json!(rows));
        }
        Ok(json!({ "a": a, "b": b, "diff": Json::Object(diff) }))
    }

    /// `why {entity?, resource?, component, frame?}` — causality query (#4)
    /// at the current (or an explicit) frame: walks the provenance chain of
    /// the value *as it was at that frame*. The answer at the bug frame IS
    /// the bisection result, with the chain attached.
    fn why(&mut self, params: &Json) -> Result<Json, (i64, String)> {
        let frame = match params.get("frame") {
            Some(_) => require_frame(params, "frame")?,
            None => self.current,
        };
        if frame >= self.timeline.len() {
            return Err((
                UNKNOWN_FRAME,
                format!(
                    "unknown frame {} (timeline has frames 0..={})",
                    frame,
                    self.timeline.len().saturating_sub(1)
                ),
            ));
        }
        // timeline[k] is the world *at the start of* frame k: only writes
        // from frames < k are visible. The final entry is the program-end
        // world and sees everything.
        let up_to = if frame + 1 == self.timeline.len() {
            u64::MAX
        } else {
            frame as u64
        };
        let explanation = match (params.get("entity"), params.get("resource")) {
            (Some(Json::String(name)), _) => {
                let component = params.get("component").and_then(|c| c.as_str()).ok_or((
                    INVALID_PARAMS,
                    "why: 'component' (string) is required".to_string(),
                ))?;
                self.ledger.explain_named(name, component, up_to)
            }
            (_, Some(Json::String(resource))) => self.ledger.explain_resource(resource, up_to),
            _ => {
                return Err((
                    INVALID_PARAMS,
                    "why: 'entity' (name string) with 'component', or 'resource', is required"
                        .to_string(),
                ))
            }
        };
        Ok(json!({ "frame": frame, "why": explanation }))
    }

    fn frame_snapshot(&self, frame: usize) -> Result<Arc<WorldSnapshot>, (i64, String)> {
        self.timeline.get(frame).cloned().ok_or((
            UNKNOWN_FRAME,
            format!(
                "unknown frame {} (timeline has frames 0..={})",
                frame,
                self.timeline.len().saturating_sub(1)
            ),
        ))
    }
}

/// Content digest of a snapshot, via a scratch world restore (CoW: cheap).
fn snapshot_digest(snap: &Arc<WorldSnapshot>) -> String {
    let mut w = World::new();
    w.restore((**snap).clone());
    w.content_digest()
}

fn require_frame(params: &Json, key: &str) -> Result<usize, (i64, String)> {
    params
        .get(key)
        .and_then(|f| f.as_u64())
        .map(|f| f as usize)
        .ok_or((
            INVALID_PARAMS,
            format!("'{}' (frame number) is required", key),
        ))
}

fn error_response(id: Json, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program with a deliberate bug: the Hit handler also drains Gold —
    /// the regression an agent would bisect for.
    const BUGGY_SRC: &str = r#"
        component Health { hp: 100 }
        component Gold { amount: 50 }
        event Hit { amount }
        on Hit(e) {
            let h = get(hero, Health) |> unwrap
            set(hero, Health { hp: h.hp - e.amount })
            if h.hp - e.amount < 80 {
                set(hero, Gold { amount: 0 })   // <- the bug
            }
        }
        let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 50 })
        for _i in range(0, 4) {
            emit Hit { amount: 10 }
            flush_events()
        }
    "#;

    fn record(src: &str) -> String {
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
        vm.enable_recording(src);
        vm.load_compile_result(result);
        let _ = vm.run(0);
        vm.take_trace().expect("trace")
    }

    fn rpc(server: &mut ReplayServer, req: Json) -> Json {
        let (resp, _) = server.handle_line(&req.to_string());
        serde_json::from_str(&resp).expect("valid response JSON")
    }

    fn server() -> ReplayServer {
        ReplayServer::from_trace(&record(BUGGY_SRC), false).expect("session")
    }

    #[test]
    fn info_reports_frames_and_verification() {
        let mut s = server();
        let resp = rpc(&mut s, json!({"jsonrpc":"2.0","id":1,"method":"info"}));
        // 4 flushes -> frames 0..=4 are starts, index 5 is program end.
        assert_eq!(resp["result"]["frames"], json!(5));
        assert_eq!(resp["result"]["verified"], json!(true));
        assert_eq!(resp["result"]["io_records"], json!(0));
    }

    #[test]
    fn bisection_loop_localizes_the_bug() {
        let mut s = server();
        // Hits apply in frames 1..=4 (handlers dispatched by flush #k run in
        // frame k). hp: 100,90,80,70,60. Gold drains when hp drops below 80,
        // i.e. during frame 3 (80 -> 70).
        let d12 = rpc(
            &mut s,
            json!({"id":2,"method":"diff_frames","params":{"a":2,"b":3}}),
        );
        assert_eq!(d12["result"]["diff"]["Health"], json!(1));
        assert!(d12["result"]["diff"].get("Gold").is_none(), "{}", d12);

        let d34 = rpc(
            &mut s,
            json!({"id":3,"method":"diff_frames","params":{"a":3,"b":4}}),
        );
        assert_eq!(d34["result"]["diff"]["Health"], json!(1));
        assert_eq!(
            d34["result"]["diff"]["Gold"],
            json!(1),
            "the Gold drain must localize to frame 3"
        );

        // Confirm the bad transition with peeks on both sides.
        let before = rpc(
            &mut s,
            json!({"id":4,"method":"peek","params":{"frame":3,"entity":"hero","component":"Gold"}}),
        );
        assert_eq!(before["result"]["fields"]["amount"], json!(50));
        let after = rpc(
            &mut s,
            json!({"id":5,"method":"peek","params":{"frame":4,"entity":"hero","component":"Gold"}}),
        );
        assert_eq!(after["result"]["fields"]["amount"], json!(0));
    }

    #[test]
    fn goto_frame_moves_the_cursor_and_peek_uses_it() {
        let mut s = server();
        let g = rpc(
            &mut s,
            json!({"id":1,"method":"goto_frame","params":{"frame":2}}),
        );
        assert_eq!(g["result"]["frame"], json!(2));
        assert_eq!(g["result"]["digest"].as_str().unwrap().len(), 64);
        // peek without an explicit frame reads at the cursor: frame 2 means
        // one Hit applied (100 -> 90).
        let p = rpc(
            &mut s,
            json!({"id":2,"method":"peek","params":{"entity":"hero","component":"Health"}}),
        );
        assert_eq!(p["result"]["fields"]["hp"], json!(90));
    }

    #[test]
    fn timeline_matches_linear_stop_at_frame_state() {
        // Acceptance #4: seeking must equal linear replay. Compare
        // the timeline snapshot digest at frame k against a fresh linear
        // replay halted with stop_at(k), for every addressable frame start.
        let trace = record(BUGGY_SRC);
        let mut s = ReplayServer::from_trace(&trace, false).expect("session");
        for k in 1..=4u64 {
            let g = rpc(
                &mut s,
                json!({"id":1,"method":"goto_frame","params":{"frame":k}}),
            );
            let timeline_digest = g["result"]["digest"].as_str().unwrap().to_string();

            let mut replayer = TraceReplayer::parse(&trace, false).expect("parse");
            replayer.stop_at(k);
            let src = replayer.source().to_string();
            let mut lexer = Lexer::new(&src);
            let tokens = lexer.tokenize().0;
            let mut parser = Parser::new(tokens);
            let program = parser.parse();
            let result = Compiler::new().compile(&program).expect("compile");
            let mut vm = VM::new();
            vm.suppress_output();
            vm.enable_replay(replayer);
            vm.load_compile_result(result);
            let err = vm.run(0).expect_err("stop sentinel");
            assert!(err.contains(crate::replay::REPLAY_STOP_PREFIX));
            assert_eq!(
                vm.world.content_digest(),
                timeline_digest,
                "timeline[{}] must equal linear replay stopped at frame {}",
                k,
                k
            );
        }
    }

    #[test]
    fn why_answers_change_across_the_bug_frame() {
        let mut s = server();
        // At frame 3 (before the drain lands) Gold's provenance is its spawn.
        let before = rpc(
            &mut s,
            json!({"id":1,"method":"why","params":{"frame":3,"entity":"hero","component":"Gold"}}),
        );
        let why_before = before["result"]["why"].as_str().unwrap();
        assert!(
            why_before.contains("spawned in frame 0"),
            "got: {}",
            why_before
        );
        assert!(!why_before.contains("on Hit"), "got: {}", why_before);

        // At frame 4 the drain is visible — and `why` IS the bisection
        // result, with the causal chain attached.
        let after = rpc(
            &mut s,
            json!({"id":2,"method":"why","params":{"frame":4,"entity":"hero","component":"Gold"}}),
        );
        let why_after = after["result"]["why"].as_str().unwrap();
        assert!(
            why_after.contains("Gold of hero = { amount: 0 }"),
            "got: {}",
            why_after
        );
        assert!(why_after.contains("(set in frame 3)"), "got: {}", why_after);
        assert!(
            why_after.contains("<- by `on Hit` handler"),
            "got: {}",
            why_after
        );
        assert!(
            why_after.contains("emitted in frame 2"),
            "got: {}",
            why_after
        );
        assert!(
            why_after.contains("<- by top-level code"),
            "got: {}",
            why_after
        );

        // Without an explicit frame, `why` reads at the cursor.
        rpc(
            &mut s,
            json!({"id":3,"method":"goto_frame","params":{"frame":3}}),
        );
        let at_cursor = rpc(
            &mut s,
            json!({"id":4,"method":"why","params":{"entity":"hero","component":"Gold"}}),
        );
        assert!(
            at_cursor["result"]["why"]
                .as_str()
                .unwrap()
                .contains("spawned in frame 0"),
            "got: {}",
            at_cursor
        );
    }

    #[test]
    fn unknown_frame_and_method_are_clean_errors() {
        let mut s = server();
        let resp = rpc(
            &mut s,
            json!({"id":1,"method":"goto_frame","params":{"frame":99}}),
        );
        assert_eq!(resp["error"]["code"], json!(UNKNOWN_FRAME));
        let resp = rpc(&mut s, json!({"id":2,"method":"timewarp"}));
        assert_eq!(resp["error"]["code"], json!(METHOD_NOT_FOUND));
        let (_, keep) = s.handle_line(&json!({"id":3,"method":"shutdown"}).to_string());
        assert!(!keep);
    }

    #[test]
    fn crashed_traces_still_serve_their_timeline() {
        let src = r#"
            component Pos { x: 0 }
            let e = spawn("crashy", Pos { x: 1 })
            flush_events()
            let boom = read_file("h:/definitely/not/a/real/path/x.txt")
        "#;
        let mut s = ReplayServer::from_trace(&record(src), false).expect("session");
        let info = rpc(&mut s, json!({"id":1,"method":"info"}));
        assert!(info["result"]["run_error"]
            .as_str()
            .unwrap()
            .contains("read_file() failed"));
        // The crash state is addressable: the entity exists at the end.
        let frames = info["result"]["frames"].as_u64().unwrap();
        let p = rpc(
            &mut s,
            json!({"id":2,"method":"peek","params":{"frame":frames,"entity":"crashy","component":"Pos"}}),
        );
        assert_eq!(p["result"]["fields"]["x"], json!(1));
    }
}
