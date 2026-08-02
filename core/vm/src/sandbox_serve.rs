//! `rad sandbox serve` — a JSON-RPC 2.0 host protocol over stdio for agent
//! frameworks (Feature #1, Phase 3).
//!
//! The client (an agent framework, orchestrator, or any process that can
//! speak line-delimited JSON) drives the speculate-inspect-commit loop:
//!
//! ```text
//! → {"jsonrpc":"2.0","id":1,"method":"propose","params":{"source":"...","input":{...},"caps":{...}}}
//! ← {"jsonrpc":"2.0","id":1,"result":{"ok":true,"fork_id":1,"out":...,"diff":{"Health":3},"fuel_spent":1234,"prints":[...]}}
//! → {"jsonrpc":"2.0","id":2,"method":"peek","params":{"fork_id":1,"entity":"hero","component":"Health"}}
//! ← {"jsonrpc":"2.0","id":2,"result":{"found":true,"fields":{"hp":40}}}
//! → {"jsonrpc":"2.0","id":3,"method":"commit","params":{"fork_id":1}}
//! ← {"jsonrpc":"2.0","id":3,"result":{"committed":true}}
//! ```
//!
//! Methods: `propose`, `peek`, `commit`, `drop`, `shutdown`.
//!
//! Trust model: the *client* is trusted (it owns the world and decides what
//! commits, including per-propose capability grants). The *source* inside
//! `propose` is untrusted and runs under the full three-layer sandbox
//! (builtin mask, component-write ACL, fuel/memory budgets). The live world
//! is only ever mutated by `commit`.
//!
//! The server writes exactly one JSON line per request to stdout; all
//! diagnostics go to stderr, keeping the protocol channel clean.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::Arc;

use serde_json::{json, Value as Json};

use crate::sandbox::SandboxCaps;
use crate::vm::VM;
use crate::world::WorldSnapshot;

pub struct SandboxServer {
    vm: VM,
    default_caps_json: String,
    forks: HashMap<u64, Arc<WorldSnapshot>>,
    next_fork_id: u64,
}

/// JSON-RPC error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const UNKNOWN_FORK: i64 = -32000;

impl SandboxServer {
    /// Wrap a (trusted, already-initialized) host VM. `default_caps_json`
    /// applies to proposals that don't carry their own `caps`; when `None`,
    /// the deny-everything default grant is used.
    pub fn new(vm: VM, default_caps_json: Option<String>) -> Self {
        SandboxServer {
            vm,
            default_caps_json: default_caps_json.unwrap_or_else(|| "{}".to_string()),
            forks: HashMap::new(),
            next_fork_id: 1,
        }
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
            "propose" => (self.propose(&params), true),
            "peek" => (self.peek(&params), true),
            "commit" => (self.commit(&params), true),
            "drop" => (self.drop_fork(&params), true),
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

    /// `propose {source, input?, caps?}` — run untrusted source against a
    /// fork of the live world. Returns a fork handle plus a cheap diff,
    /// structured output, prints, and fuel accounting. Guest failure is a
    /// successful RPC with `ok: false` — agents need the diagnostics.
    fn propose(&mut self, params: &Json) -> Result<Json, (i64, String)> {
        let source = params.get("source").and_then(|s| s.as_str()).ok_or((
            INVALID_PARAMS,
            "propose: 'source' (string) is required".to_string(),
        ))?;
        let caps_text = match params.get("caps") {
            Some(c) => c.to_string(),
            None => self.default_caps_json.clone(),
        };
        let (caps, seed) = SandboxCaps::from_json(&caps_text)
            .map_err(|e| (INVALID_PARAMS, format!("propose: {}", e)))?;
        let input_json = params.get("input").map(|v| v.to_string());

        let base = self.vm.get_world().snapshot();
        let outcome = VM::run_sandbox_guest(
            source,
            base.clone(),
            caps,
            seed,
            input_json,
            self.vm.component_field_types.clone(),
        );

        match outcome.result {
            Ok(snap) => {
                let diff = WorldSnapshot::diff_summary(&base, &snap);
                let fork_id = self.next_fork_id;
                self.next_fork_id += 1;
                self.forks.insert(fork_id, Arc::new(snap));
                let out = outcome
                    .output_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(Json::Null);
                Ok(json!({
                    "ok": true,
                    "fork_id": fork_id,
                    "out": out,
                    "diff": diff,
                    "fuel_spent": outcome.fuel_spent,
                    "prints": outcome.prints,
                }))
            }
            Err(e) => Ok(json!({
                "ok": false,
                "error": e,
                "fuel_spent": outcome.fuel_spent,
                "prints": outcome.prints,
            })),
        }
    }

    /// `peek {fork_id, entity, component}` — read one component from a fork
    /// without committing. `entity` is a name (string) or entity id (number).
    fn peek(&mut self, params: &Json) -> Result<Json, (i64, String)> {
        let snap = self.lookup_fork(params)?;
        let component = params.get("component").and_then(|c| c.as_str()).ok_or((
            INVALID_PARAMS,
            "peek: 'component' (string) is required".to_string(),
        ))?;
        let eid = match params.get("entity") {
            Some(Json::String(name)) => snap
                .get_entity_by_name(name)
                .ok_or((INVALID_PARAMS, format!("peek: no entity named '{}'", name)))?,
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
                Ok(json!({ "found": true, "fields": Json::Object(fields) }))
            }
            None => Ok(json!({ "found": false })),
        }
    }

    /// `commit {fork_id}` — replace the live world with the fork's state.
    /// Mirrors the `commit()` builtin: pending host events are cleared since
    /// they reference pre-commit state. The fork stays available.
    fn commit(&mut self, params: &Json) -> Result<Json, (i64, String)> {
        let snap = self.lookup_fork(params)?;
        self.vm.get_world_mut().restore((*snap).clone());
        self.vm.events_current.clear();
        self.vm.events_next.clear();
        Ok(json!({ "committed": true }))
    }

    /// `drop {fork_id}` — discard a fork.
    fn drop_fork(&mut self, params: &Json) -> Result<Json, (i64, String)> {
        let fork_id = require_fork_id(params)?;
        let existed = self.forks.remove(&fork_id).is_some();
        Ok(json!({ "dropped": existed }))
    }

    fn lookup_fork(&self, params: &Json) -> Result<Arc<WorldSnapshot>, (i64, String)> {
        let fork_id = require_fork_id(params)?;
        self.forks
            .get(&fork_id)
            .cloned()
            .ok_or((UNKNOWN_FORK, format!("unknown fork_id {}", fork_id)))
    }
}

fn require_fork_id(params: &Json) -> Result<u64, (i64, String)> {
    params
        .get("fork_id")
        .and_then(|f| f.as_u64())
        .ok_or((INVALID_PARAMS, "'fork_id' (number) is required".to_string()))
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
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    const HOST: &str = r#"
        component Health { hp: 100 }
        component Gold { amount: 1000 }
        let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 1000 })
    "#;

    fn server() -> SandboxServer {
        let mut lexer = Lexer::new(HOST);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(parser.errors().is_empty());
        let result = Compiler::new().compile(&program).expect("host compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.load_compile_result(result);
        vm.run(0).expect("host run");
        SandboxServer::new(vm, Some(r#"{ "write": ["Health"] }"#.to_string()))
    }

    fn call(server: &mut SandboxServer, req: Json) -> Json {
        let (resp, keep) = server.handle_line(&req.to_string());
        assert!(keep);
        serde_json::from_str(&resp).expect("response is JSON")
    }

    #[test]
    fn propose_peek_commit_roundtrip() {
        let mut s = server();

        // Propose: guest reads input, writes Health, reports output.
        let resp = call(
            &mut s,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "propose",
                "params": {
                    "source": r#"
                        component Health { hp: 100 }
                        let plan = sandbox_input()
                        let target = plan["target_hp"]
                        set(get_entity("hero"), Health { hp: target })
                        sandbox_output({ "applied": target })
                    "#,
                    "input": { "target_hp": 42 },
                },
            }),
        );
        let result = &resp["result"];
        assert_eq!(result["ok"], json!(true), "propose failed: {}", resp);
        assert_eq!(result["out"]["applied"], json!(42));
        assert_eq!(
            result["diff"]["Health"],
            json!(1),
            "diff: {}",
            result["diff"]
        );
        assert!(result["diff"].get("Gold").is_none(), "Gold untouched");
        assert!(result["fuel_spent"].as_u64().unwrap() > 0);
        let fork_id = result["fork_id"].as_u64().unwrap();

        // Peek into the fork.
        let resp = call(
            &mut s,
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "peek",
                "params": { "fork_id": fork_id, "entity": "hero", "component": "Health" },
            }),
        );
        assert_eq!(resp["result"]["found"], json!(true));
        assert_eq!(resp["result"]["fields"]["hp"], json!(42));

        // Live world still untouched.
        let live = s.vm.get_world().snapshot();
        let eid = live.get_entity_by_name("hero").unwrap();
        let hp = live.get_component(eid, "Health").unwrap().values[0]
            .as_int()
            .unwrap();
        assert_eq!(hp, 100);

        // Commit, then the live world reflects the proposal.
        let resp = call(
            &mut s,
            json!({
                "jsonrpc": "2.0", "id": 3, "method": "commit",
                "params": { "fork_id": fork_id },
            }),
        );
        assert_eq!(resp["result"]["committed"], json!(true));
        let live = s.vm.get_world().snapshot();
        let hp = live.get_component(eid, "Health").unwrap().values[0]
            .as_int()
            .unwrap();
        assert_eq!(hp, 42);
    }

    #[test]
    fn hostile_proposal_returns_ok_false_with_diagnostics() {
        let mut s = server();
        let resp = call(
            &mut s,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "propose",
                "params": {
                    "source": r#"
                        component Gold { amount: 1000 }
                        set(get_entity("hero"), Gold { amount: 0 })
                    "#,
                },
            }),
        );
        let result = &resp["result"];
        assert_eq!(result["ok"], json!(false));
        assert!(
            result["error"]
                .as_str()
                .unwrap()
                .contains("denied by capability grant"),
            "got: {}",
            result["error"]
        );
        assert!(result.get("fork_id").is_none(), "no fork on failure");
    }

    #[test]
    fn per_request_caps_override_default() {
        let mut s = server();
        // Default caps deny Gold; per-request caps grant it.
        let resp = call(
            &mut s,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "propose",
                "params": {
                    "source": r#"
                        component Gold { amount: 1000 }
                        set(get_entity("hero"), Gold { amount: 1 })
                    "#,
                    "caps": { "write": ["Gold"] },
                },
            }),
        );
        assert_eq!(resp["result"]["ok"], json!(true), "got: {}", resp);
        assert_eq!(resp["result"]["diff"]["Gold"], json!(1));
    }

    #[test]
    fn drop_discards_fork_and_peek_reports_unknown() {
        let mut s = server();
        let resp = call(
            &mut s,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "propose",
                "params": { "source": "1 + 1" },
            }),
        );
        let fork_id = resp["result"]["fork_id"].as_u64().unwrap();

        let resp = call(
            &mut s,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "drop", "params": { "fork_id": fork_id } }),
        );
        assert_eq!(resp["result"]["dropped"], json!(true));

        let resp = call(
            &mut s,
            json!({
                "jsonrpc": "2.0", "id": 3, "method": "peek",
                "params": { "fork_id": fork_id, "entity": "hero", "component": "Health" },
            }),
        );
        assert_eq!(resp["error"]["code"], json!(UNKNOWN_FORK));
    }

    #[test]
    fn protocol_errors_are_jsonrpc_errors() {
        let mut s = server();

        let (resp, keep) = s.handle_line("this is not json");
        assert!(keep);
        let resp: Json = serde_json::from_str(&resp).unwrap();
        assert_eq!(resp["error"]["code"], json!(PARSE_ERROR));

        let resp = call(
            &mut s,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "frobnicate" }),
        );
        assert_eq!(resp["error"]["code"], json!(METHOD_NOT_FOUND));

        let resp = call(
            &mut s,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "propose", "params": {} }),
        );
        assert_eq!(resp["error"]["code"], json!(INVALID_PARAMS));
    }

    #[test]
    fn shutdown_stops_serving() {
        let mut s = server();
        let (resp, keep) =
            s.handle_line(&json!({ "jsonrpc": "2.0", "id": 1, "method": "shutdown" }).to_string());
        assert!(!keep);
        let resp: Json = serde_json::from_str(&resp).unwrap();
        assert_eq!(resp["result"]["bye"], json!(true));
    }

    #[test]
    fn serve_loop_over_buffers() {
        let mut s = server();
        let input = format!(
            "{}\n{}\n",
            json!({ "jsonrpc": "2.0", "id": 1, "method": "propose", "params": { "source": "1 + 1" } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" }),
        );
        let mut output = Vec::new();
        s.serve(input.as_bytes(), &mut output).expect("serve");
        let lines: Vec<&str> = std::str::from_utf8(&output).unwrap().lines().collect();
        assert_eq!(lines.len(), 2);
        let first: Json = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["result"]["ok"], json!(true));
        let second: Json = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["result"]["bye"], json!(true));
    }
}
