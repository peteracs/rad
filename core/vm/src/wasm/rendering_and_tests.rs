

impl Default for RadRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Bytecode chunk plus an internal scratch `GcHeap` holding heap constants (`from_int` / `from_string`,
/// etc.). On [`RadRuntime::load_and_run`], that heap is **merged** into the VM heap before the
/// chunk is installed, so constant pointers stay valid.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct WasmChunk {
    inner: Chunk,
    gc: GcHeap,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl WasmChunk {
    pub fn add_int(&mut self, value: i64) -> u16 {
        self.inner
            .add_constant(Value::from_int(&mut self.gc, value))
    }

    pub fn add_float(&mut self, value: f64) -> u16 {
        self.inner.add_constant(Value::from_float(value))
    }

    pub fn add_string(&mut self, value: &str) -> u16 {
        self.inner
            .add_constant(Value::from_string(&mut self.gc, value.to_string()))
    }

    pub fn add_bool(&mut self, value: bool) -> u16 {
        self.inner.add_constant(Value::from_bool(value))
    }

    pub fn add_nil(&mut self) -> u16 {
        self.inner.add_constant(Value::NIL)
    }

    pub fn write_const_int(&mut self, value: i64, line: u32) {
        self.inner
            .write_const(Value::from_int(&mut self.gc, value), line);
    }

    pub fn write_const_float(&mut self, value: f64, line: u32) {
        self.inner.write_const(Value::from_float(value), line);
    }

    pub fn write_const_str(&mut self, value: &str, line: u32) {
        self.inner
            .write_const(Value::from_string(&mut self.gc, value.to_string()), line);
    }

    pub fn write_const_bool(&mut self, value: bool, line: u32) {
        self.inner.write_const(Value::from_bool(value), line);
    }

    pub fn emit_op(&mut self, op_name: &str, line: u32) -> Result<(), String> {
        let op = parse_op_name(op_name).ok_or_else(|| format!("Unknown opcode: {}", op_name))?;
        self.inner.write_op(op, line);
        Ok(())
    }

    pub fn emit_u16(&mut self, val: u16, line: u32) {
        self.inner.write_u16(val, line);
    }

    pub fn emit_byte(&mut self, val: u8, line: u32) {
        self.inner.write(val, line);
    }
}

fn parse_op_name(name: &str) -> Option<Op> {
    match name {
        "Const" => Some(Op::Const),
        "Pop" => Some(Op::Pop),
        "Dup" => Some(Op::Dup),
        "Add" => Some(Op::Add),
        "Sub" => Some(Op::Sub),
        "Mul" => Some(Op::Mul),
        "Div" => Some(Op::Div),
        "Mod" => Some(Op::Mod),
        "BitAnd" => Some(Op::BitAnd),
        "BitOr" => Some(Op::BitOr),
        "BitXor" => Some(Op::BitXor),
        "Shl" => Some(Op::Shl),
        "Shr" => Some(Op::Shr),
        "BitNot" => Some(Op::BitNot),
        "Neg" => Some(Op::Neg),
        "Not" => Some(Op::Not),
        "Eq" => Some(Op::Eq),
        "Neq" => Some(Op::Neq),
        "Lt" => Some(Op::Lt),
        "Lte" => Some(Op::Lte),
        "Gt" => Some(Op::Gt),
        "Gte" => Some(Op::Gte),
        // Logical and/or are compiled as short-circuit jumps, not direct opcodes.
        "DefGlobal" => Some(Op::DefGlobal),
        "GetGlobal" => Some(Op::GetGlobal),
        "SetGlobal" => Some(Op::SetGlobal),
        "GetLocal" => Some(Op::GetLocal),
        "SetLocal" => Some(Op::SetLocal),
        "MoveLocal" => Some(Op::MoveLocal),
        "Jump" => Some(Op::Jump),
        "JumpIfFalse" => Some(Op::JumpIfFalse),
        "JumpBack" => Some(Op::JumpBack),
        "Call" => Some(Op::Call),
        "AsyncCall" => Some(Op::AsyncCall),
        "Await" => Some(Op::Await),
        "Yield" => Some(Op::Yield),
        "Return" => Some(Op::Return),
        "Try" => Some(Op::Try),
        "MakeList" => Some(Op::MakeList),
        "MakeComp" => Some(Op::MakeComp),
        "MakeState" => Some(Op::MakeState),
        "GetField" => Some(Op::GetField),
        "SetField" => Some(Op::SetField),
        "GetIndex" => Some(Op::GetIndex),
        "SetIndex" => Some(Op::SetIndex),
        "EcsSpawn" => Some(Op::EcsSpawn),
        "EcsGet" => Some(Op::EcsGet),
        "EcsSet" => Some(Op::EcsSet),
        "EcsHas" => Some(Op::EcsHas),
        "EcsQuery" => Some(Op::EcsQuery),
        "Transition" => Some(Op::Transition),
        "MakeVariant" => Some(Op::MakeVariant),
        "Emit" => Some(Op::Emit),
        "RunSystem" => Some(Op::RunSystem),
        "RunSchedule" => Some(Op::RunSchedule),
        "RunScheduleSerial" => Some(Op::RunScheduleSerial),
        "MatchState" => Some(Op::MatchState),
        "IsVariant" => Some(Op::IsVariant),
        "Print" => Some(Op::Print),
        "Len" => Some(Op::Len),
        "TypeOf" => Some(Op::TypeOf),
        "Closure" => Some(Op::Closure),
        "GetUpvalue" => Some(Op::GetUpvalue),
        "SetUpvalue" => Some(Op::SetUpvalue),
        "MakeMap" => Some(Op::MakeMap),
        "GetFieldSlot" => Some(Op::GetFieldSlot),
        "SetFieldSlot" => Some(Op::SetFieldSlot),
        "MakeCompSlot" => Some(Op::MakeCompSlot),
        "LogicalLoad" => Some(Op::LogicalLoad),
        "LogicalStore" => Some(Op::LogicalStore),
        "MaterializeAoS" => Some(Op::MaterializeAoS),
        "QueryFilter" => Some(Op::QueryFilter),
        "Snapshot" => Some(Op::Snapshot),
        "Rollback" => Some(Op::Rollback),
        "VecBroadcast" => Some(Op::VecBroadcast),
        "ConcatN" => Some(Op::ConcatN),
        "Halt" => Some(Op::Halt),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_basic_execution() {
        let mut rt = RadRuntime::new();
        let mut chunk = rt.create_chunk("test");
        chunk.write_const_str("Hello WASM", 1);
        chunk.emit_op("Print", 1).unwrap();
        chunk.emit_byte(1, 1);
        chunk.emit_op("Halt", 2).unwrap();

        let result = rt.load_and_run(chunk).unwrap();
        assert_eq!(result, "Hello WASM");
    }

    #[test]
    fn add_state_transition_with_guard_sets_guard_chunk() {
        let mut rt = RadRuntime::new();
        rt.add_state_transition_with_guard("Door", "Closed", "open", "Open", Some(7));
        let transitions = rt
            .vm
            .state_machines
            .get("Door")
            .and_then(|m| m.get("Closed"))
            .expect("transition list should exist");
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].guard_chunk_id, Some(7));
    }

    #[test]
    fn runtime_arithmetic() {
        let mut rt = RadRuntime::new();
        let mut chunk = rt.create_chunk("math");
        chunk.write_const_int(10, 1);
        chunk.write_const_int(20, 1);
        chunk.emit_op("Add", 1).unwrap();
        chunk.emit_op("Print", 1).unwrap();
        chunk.emit_byte(1, 1);
        chunk.emit_op("Halt", 2).unwrap();

        let result = rt.load_and_run(chunk).unwrap();
        assert_eq!(result, "30");
    }

    #[test]
    fn compile_and_run_source() {
        let mut rt = RadRuntime::new();
        let result = rt.compile_and_run("print(2 + 3)").unwrap();
        assert_eq!(result, "5");
    }

    #[test]
    fn compile_and_run_causal_settlement_when_capability_is_advertised() {
        let mut rt = RadRuntime::new();
        let source = r#"
component Health { hp: int = 100 }
intent Damage { key target: entity, amount: int }
law Hit(target: entity) { propose Damage { target: target, amount: 20 } }
resolver ResolveDamage for Damage(target, proposals) {
    next(target, Health { hp: require(target, Health).hp - 20 })
}
entity hero { Health {} }
settle { Hit(hero) }
print(require(hero, Health).hp)
"#;
        assert_eq!(rt.compile_and_run(source).unwrap(), "80");
        let features: serde_json::Value =
            serde_json::from_str(&rt.runtime_features()).expect("feature JSON");
        assert_eq!(features["causal_laws"], 1);
        assert_eq!(features["relations_frontend"], 1);
        assert_eq!(features["causal_constraints"], 1);
        assert_eq!(features["host_values"], 1);
        assert_eq!(features["presentation"]["avatar_instances"]["version"], 2);
        assert_eq!(
            features["presentation"]["avatar_instances"]["fields"]["entity_generation"],
            1
        );
        assert_eq!(features["causal_value_limits"]["max_depth"], 128);
        assert_eq!(
            features["causal_value_limits"]["max_encoded_bytes"],
            8 * 1024 * 1024
        );
        assert_eq!(
            features["constraint_limits"]["max_aggregate_fuel"],
            crate::constraint_types::ConstraintLimitProfile::HARD_MAX_AGGREGATE_FUEL
        );
    }

    #[test]
    fn webgpu_dogfood_compiles_ticks_and_exports_exact_packet() {
        let mut runtime = RadRuntime::new();
        runtime
            .session_start(include_str!(
                "../../../../projects/rad-webgpu/demo/world.rad"
            ))
            .expect("WebGPU dogfood source should compile");
        runtime
            .session_emit("Tick", r#"{"dt":0.016}"#)
            .expect("tick should enqueue");
        runtime.session_pump().expect("tick should execute");
        runtime
            .session_render_buffer_refresh_bounded(4, 4)
            .expect("four avatars fit the packet profile");
        assert_eq!(runtime.render_buffer[0], presentation::MAGIC);
        assert_eq!(runtime.render_buffer[1], presentation::VERSION);
        assert_eq!(runtime.render_buffer[3], 4);
        assert_eq!(
            runtime.render_buffer.len(),
            presentation::HEADER_WORDS + 4 * presentation::RECORD_WORDS
        );
    }

    #[test]
    fn browser_boundary_returns_tagged_candidate_rejection_json() {
        let mut runtime = RadRuntime::new();
        let result: serde_json::Value = serde_json::from_str(&runtime.compile_and_run_result_json(
            r#"
component Position { x: int = 0 }
intent Move { key target: entity, amount: int }
law Push(target: entity) { propose Move { target: target, amount: 20 } }
resolver ResolveMove for Move(target, proposals) {
    next(target, Position { x: proposals[0].amount })
}
constraint WorldBounds for Position(subject, proposed) {
    require proposed.x <= 10 else "position.above_max"
}
entity hero { Position {} }
settle { Push(hero) }
"#,
        ))
        .expect("tagged result JSON");
        assert_eq!(result["kind"], "settlement_rejected");
        assert_eq!(result["violations"][0]["code"], "position.above_max");
        assert_eq!(result["evaluation_failures"], serde_json::json!([]));
    }

    #[test]
    fn compile_and_run_autocalls_main() {
        let mut rt = RadRuntime::new();
        let source = r#"
fn main() -> nil {
    print("hello wasm")
}
"#;
        let result = rt.compile_and_run(source).unwrap();
        assert_eq!(result, "hello wasm");
    }

    #[test]
    fn compile_and_run_playground_types_snippet() {
        let mut rt = RadRuntime::new();
        let source = r#"
fn add(a: int, b: int) -> int {
    return a + b
}

fn main() -> nil {
    print(add(20, 22))
}
"#;
        let result = rt.compile_and_run(source).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn compile_and_run_playground_match_snippet() {
        let mut rt = RadRuntime::new();
        let source = r#"
type Packet {
    Ping { id: 0 }
    Data { id: 0 }
}

let p = Packet::Data { id: 7 }
match p {
    Ping { id } => { print("ping") print(id) }
    Data { id } => { print("data") print(id) }
}
"#;
        let result = rt.compile_and_run(source).unwrap();
        assert_eq!(result, "data\n7");
    }

    #[test]
    fn compile_and_run_with_state_machine() {
        let mut rt = RadRuntime::new();
        let source = r#"
state Door {
    Closed { on open -> Open }
    Open { on close -> Closed }
}

let d = Door::Closed
let r = transition(d, "open") |> unwrap
print(r)
"#;
        let result = rt.compile_and_run(source).unwrap();
        assert_eq!(result, "Door::Open");
    }

    #[test]
    fn compile_and_run_syntax_error() {
        let mut rt = RadRuntime::new();
        let result = rt.compile_and_run("let = bad");
        assert!(result.is_err());
    }

    #[test]
    fn compile_and_run_reports_use_not_supported() {
        let mut rt = RadRuntime::new();
        let result = rt.compile_and_run(r#"use "shared.rad""#);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("not supported in browser playground"));
    }

    #[test]
    fn check_source_ok() {
        let rt = RadRuntime::new();
        let result = rt.check_source("let x = 42\nprint(x)");
        assert!(result.is_ok());
    }

    #[test]
    fn check_source_error() {
        let rt = RadRuntime::new();
        let result = rt.check_source("let x = 42\nx = 10");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("immutable"));
    }

    #[test]
    fn check_source_reports_use_not_supported() {
        let rt = RadRuntime::new();
        let result = rt.check_source(r#"use "shared.rad""#);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("not supported in browser playground"));
    }

    #[test]
    fn compile_only_counts_chunks_systems_handlers() {
        let rt = RadRuntime::new();
        let src = r#"
component Pos { x: int }

fn main() -> nil {
    print(1)
}

system step(p: mut Pos) {
}

event Tap { }

on Tap(_t) {
    print("tap")
}
"#;
        let msg = rt.compile_only(src).unwrap();
        assert!(
            msg.contains("chunks") && msg.contains("systems") && msg.contains("handlers"),
            "got: {msg}"
        );
    }

    #[test]
    fn get_world_snapshot_json_like() {
        let rt = RadRuntime::new();
        // resources ride along since RADGUI (renderers read UiConfig etc.)
        assert_eq!(
            rt.get_world_snapshot(),
            "{\"entities\":[],\"resources\":{}}"
        );
    }

    #[test]
    fn emit_op_accepts_query_snapshot_rollback() {
        let rt = RadRuntime::new();
        let mut chunk = rt.create_chunk("snap_ops");
        chunk.emit_op("QueryFilter", 1).unwrap();
        chunk.emit_op("Snapshot", 1).unwrap();
        chunk.emit_op("Rollback", 1).unwrap();
    }

    // -- D4: streaming sessions ---------------------------------------------

    const COLLAB_SRC: &str = r#"
component Note { text: "", by: "" }
resource Board { count: 0 }
event AddNote { text: str, by: str }
event ClearNotes { by: str }

on AddNote(e) {
    let b = get_resource(Board) |> unwrap
    let n = b.count + 1
    let _ = spawn(f"note-{n}", Note { text: e.text, by: e.by })
    set_resource(Board, Board { count: n })
    print(f"{e.by} added note {n}")
}

on ClearNotes(e) {
    for ent in entities(Note) { despawn(ent) }
    print(f"{e.by} cleared the board")
}
"#;

    /// One session, many frames, zero recompiles: emit -> pump mutates the
    /// world through declared handlers, and output is per-frame.
    #[test]
    fn session_emits_pump_frames_without_recompiling() {
        let mut rt = RadRuntime::new();
        rt.session_start(COLLAB_SRC).unwrap();

        rt.session_emit("AddNote", r#"{"text": "ship D4", "by": "alice"}"#)
            .unwrap();
        let out1 = rt.session_pump().unwrap();
        assert_eq!(out1, "alice added note 1");

        rt.session_emit("AddNote", r#"{"text": "λ unicode", "by": "bob"}"#)
            .unwrap();
        rt.session_emit("AddNote", r#"{"text": "third", "by": "bob"}"#)
            .unwrap();
        let out2 = rt.session_pump().unwrap();
        assert_eq!(out2, "bob added note 2\nbob added note 3");

        let snap = rt.get_world_snapshot();
        assert!(
            snap.contains("ship D4") && snap.contains("λ unicode"),
            "got: {snap}"
        );
    }

    /// Bad pushes are session-fatal for the *input*, not the session:
    /// unknown events and missing fields Err, then the session keeps going.
    #[test]
    fn session_emit_validates_and_survives() {
        let mut rt = RadRuntime::new();
        rt.session_start(COLLAB_SRC).unwrap();
        assert!(rt.session_emit("NoSuchEvent", "{}").is_err());
        assert!(rt.session_emit("AddNote", r#"{"text": "no by"}"#).is_err());
        assert!(rt.session_emit("AddNote", "not json").is_err());
        rt.session_emit("AddNote", r#"{"text": "still alive", "by": "carol"}"#)
            .unwrap();
        assert_eq!(rt.session_pump().unwrap(), "carol added note 1");
    }

    /// The D4 PASS shape, natively provable: three tabs (one authority, two
    /// replicas) start the same source; the authority's edits stream as one
    /// fork_delta per flush; replicas apply in order; world_digest agrees on
    /// ALL THREE after every flush. Replicas never recompile and never run
    /// the handlers — they converge on state alone.
    #[test]
    fn session_three_tabs_converge_via_deltas() {
        let mut host = RadRuntime::new();
        let mut tab2 = RadRuntime::new();
        let mut tab3 = RadRuntime::new();
        host.session_start(COLLAB_SRC).unwrap();
        tab2.session_start(COLLAB_SRC).unwrap();
        tab3.session_start(COLLAB_SRC).unwrap();
        assert_eq!(
            host.session_digest().unwrap(),
            tab2.session_digest().unwrap(),
            "deterministic start"
        );

        // Frame 1: one edit.
        host.session_emit("AddNote", r#"{"text": "from the host", "by": "alice"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d1 = host.session_delta().unwrap();
        tab2.session_apply(&d1).unwrap();
        tab3.session_apply(&d1).unwrap();
        let h = host.session_digest().unwrap();
        assert_eq!(h, tab2.session_digest().unwrap(), "tab2 after frame 1");
        assert_eq!(h, tab3.session_digest().unwrap(), "tab3 after frame 1");
        assert!(tab3.get_world_snapshot().contains("from the host"));

        // Frame 2: two edits batched in one flush -> still ONE delta.
        host.session_emit("AddNote", r#"{"text": "two", "by": "bob"}"#)
            .unwrap();
        host.session_emit("AddNote", r#"{"text": "three", "by": "bob"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d2 = host.session_delta().unwrap();
        tab2.session_apply(&d2).unwrap();
        tab3.session_apply(&d2).unwrap();
        let h = host.session_digest().unwrap();
        assert_eq!(h, tab2.session_digest().unwrap(), "tab2 after frame 2");
        assert_eq!(h, tab3.session_digest().unwrap(), "tab3 after frame 2");

        // Frame 3: a despawning edit (clear) converges too.
        host.session_emit("ClearNotes", r#"{"by": "alice"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d3 = host.session_delta().unwrap();
        tab2.session_apply(&d3).unwrap();
        tab3.session_apply(&d3).unwrap();
        let h = host.session_digest().unwrap();
        assert_eq!(h, tab2.session_digest().unwrap(), "tab2 after clear");
        assert_eq!(h, tab3.session_digest().unwrap(), "tab3 after clear");
        assert!(!tab2.get_world_snapshot().contains("from the host"));
    }

    /// Late join: a fourth tab arrives mid-session, adopts the full state,
    /// then rides the same delta stream as everyone else.
    #[test]
    fn session_late_joiner_loads_state_then_streams() {
        let mut host = RadRuntime::new();
        host.session_start(COLLAB_SRC).unwrap();
        host.session_emit("AddNote", r#"{"text": "early", "by": "a"}"#)
            .unwrap();
        host.session_pump().unwrap();
        host.session_delta().unwrap(); // broadcast nobody hears; state moves on

        // Late tab: no source run needed beyond decls, adopt state directly.
        let mut late = RadRuntime::new();
        late.session_start(COLLAB_SRC).unwrap();
        let state = host.session_state().unwrap();
        late.session_load(&state).unwrap();
        assert_eq!(
            host.session_digest().unwrap(),
            late.session_digest().unwrap(),
            "late joiner adopts the full state"
        );

        // And the stream continues for both.
        host.session_emit("AddNote", r#"{"text": "after join", "by": "b"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d = host.session_delta().unwrap();
        late.session_apply(&d).unwrap();
        assert_eq!(
            host.session_digest().unwrap(),
            late.session_digest().unwrap()
        );
        assert!(late.get_world_snapshot().contains("after join"));
    }

    /// Deltas chain and order matters: applying frame 2's delta before
    /// frame 1's is a refusal (fingerprint mismatch), not a corrupt world.
    #[test]
    fn session_apply_refuses_out_of_order_deltas() {
        let mut host = RadRuntime::new();
        let mut replica = RadRuntime::new();
        host.session_start(COLLAB_SRC).unwrap();
        replica.session_start(COLLAB_SRC).unwrap();

        host.session_emit("AddNote", r#"{"text": "one", "by": "a"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d1 = host.session_delta().unwrap();
        host.session_emit("AddNote", r#"{"text": "two", "by": "a"}"#)
            .unwrap();
        host.session_pump().unwrap();
        let d2 = host.session_delta().unwrap();

        let err = replica.session_apply(&d2).unwrap_err();
        assert!(err.contains("different base"), "got: {err}");
        // In order, both apply and the worlds agree.
        replica.session_apply(&d1).unwrap();
        replica.session_apply(&d2).unwrap();
        assert_eq!(
            host.session_digest().unwrap(),
            replica.session_digest().unwrap()
        );
    }

    /// Causality survives the host boundary: a host-pushed event answers
    /// why() with the handler chain, like any rad-emitted event.
    #[test]
    fn session_pushed_events_have_causality() {
        let mut rt = RadRuntime::new();
        rt.session_start(COLLAB_SRC).unwrap();
        rt.session_emit("AddNote", r#"{"text": "traced", "by": "dana"}"#)
            .unwrap();
        rt.session_pump().unwrap();
        let snap = rt.get_world_snapshot();
        assert!(snap.contains("traced"), "got: {snap}");
        let recorded = rt.vm.ledger.emits.iter().any(|e| e.event == "AddNote");
        assert!(recorded, "host emit must land in the causality ledger");
    }

    #[test]
    fn emit_op_rejects_legacy_pipe_and_break() {
        let rt = RadRuntime::new();
        let mut chunk = rt.create_chunk("legacy");
        let pipe_err = chunk.emit_op("Pipe", 1).unwrap_err();
        assert!(pipe_err.contains("Unknown opcode"), "got: {pipe_err}");
        let break_err = chunk.emit_op("Break", 1).unwrap_err();
        assert!(break_err.contains("Unknown opcode"), "got: {break_err}");
    }
}
