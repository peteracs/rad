use crate::opcode::{Chunk, Op};
use crate::value::{Builtin, FnValue, Value};
use crate::vm::{EcsCommand, EventLogEntry, TaskRecord, TaskStatus, VM};

fn write_u16_at(chunk: &mut Chunk, offset: usize, value: usize) {
    chunk.code[offset] = ((value >> 8) & 0xff) as u8;
    chunk.code[offset + 1] = (value & 0xff) as u8;
}

fn valid_return(name: &str) -> Chunk {
    let mut chunk = Chunk::new(name);
    chunk.write_const(Value::NIL, 1);
    chunk.write_op(Op::Return, 1);
    chunk
}

fn jump_over_end_then(name: &str, op: Op, operands: &[u8]) -> Chunk {
    let mut chunk = Chunk::new(name);
    chunk.write_op(Op::BeginSettlement, 1);
    chunk.write_op(Op::Jump, 1);
    chunk.write_u16(0, 1);
    chunk.write_op(Op::EndSettlement, 1);
    let after_end = chunk.code.len();
    write_u16_at(&mut chunk, 2, after_end);
    chunk.write_op(op, 1);
    for &byte in operands {
        chunk.write(byte, 1);
    }
    chunk.write_const(Value::NIL, 1);
    chunk.write_op(Op::Return, 1);
    chunk
}

fn callable(vm: &mut VM, chunk_id: usize, name: &str) -> Value {
    Value::from_fn(
        vm.gc_mut(),
        FnValue {
            name: name.to_string(),
            arity: 0,
            chunk_id,
        },
    )
}

fn call_chunk(vm: &mut VM, chunk_id: usize, name: &str) -> Result<Value, String> {
    let callee = callable(vm, chunk_id, name);
    vm.call_value(&callee, Vec::new())
}

fn seed_observable_state(vm: &mut VM) {
    vm.events_current
        .push(("seed-current".to_string(), Value::NIL, 41));
    vm.events_next
        .push(("seed-next".to_string(), Value::NIL, 42));
    vm.events_processing
        .push(("seed-processing".to_string(), Value::NIL, 43));
    vm.delayed_events
        .push((3, "seed-delayed".to_string(), Value::NIL, 44));
    vm.emit_ids_current.push(41);
    vm.emit_ids_next.push(42);
    vm.print_buffer.push("seed stdout".to_string());
    vm.eprint_buffer.push("seed stderr".to_string());
    vm.rng_state = 0x5eed_cafe;
    vm.tasks.insert(
        73,
        TaskRecord {
            id: 73,
            status: TaskStatus::Ready,
        },
    );
    vm.next_task_id = 74;
    vm.timeline.push(vm.world.snapshot());
    vm.event_log.push(EventLogEntry {
        tick: 9,
        event_name: "seed-log".to_string(),
        payload: Value::NIL,
    });
    vm.command_buffer.push(EcsCommand::DespawnEntity(999));
    vm.sandbox_input_json = Some("{\"seed\":true}".to_string());
    vm.sandbox_output_json = Some("{\"out\":1}".to_string());
    vm.last_sandbox_output_json = Some("{\"last\":1}".to_string());
    vm.last_sandbox_fuel_spent = 17;
    vm.current_trace_id = Some(91);
    vm.next_trace_id = 92;
    vm.causality_frame = 12;
    vm.once_guard_passed = true;
}

fn assert_reusable(vm: &mut VM) {
    let valid = vm
        .load_verified_chunk(valid_return("reuse-after-bytecode-fault"))
        .expect("valid reuse chunk should verify");
    call_chunk(vm, valid, "reuse").expect("VM must remain reusable after fault");
}

#[test]
fn verifier_rejects_world_mutation_after_a_jump_over_end_before_loading() {
    let cases = [
        ("spawn", Op::EcsSpawn, vec![0, 0, 0, 0]),
        ("set", Op::EcsSet, vec![]),
        ("resource", Op::InitResource, vec![0, 0, 0, 0]),
    ];
    for (name, op, operands) in cases {
        let mut vm = VM::new();
        seed_observable_state(&mut vm);
        let before = vm.observable_state_signature();
        let error = vm
            .load_verified_chunk(jump_over_end_then(name, op, &operands))
            .expect_err("settlement-crossing jump must fail verification");
        assert!(error.message.contains("crosses settlement"), "{error}");
        assert_eq!(vm.observable_state_signature(), before);
    }
}

#[test]
fn unchecked_escape_hits_effect_firewall_before_world_mutation() {
    let cases = [
        ("spawn", Op::EcsSpawn, vec![0, 0, 0, 0]),
        ("set", Op::EcsSet, vec![]),
        ("resource", Op::InitResource, vec![0, 0, 0, 0]),
    ];
    for (name, op, operands) in cases {
        let mut vm = VM::new();
        let chunk = vm.load_unchecked_chunk(jump_over_end_then(name, op, &operands));
        let callee = callable(&mut vm, chunk, name);
        seed_observable_state(&mut vm);
        let before = vm.observable_state_signature();
        let error = vm
            .call_value(&callee, Vec::new())
            .expect_err("runtime firewall must reject unchecked mutation");
        assert!(error.contains("Settlement effect firewall"), "{error}");
        assert_eq!(vm.observable_state_signature(), before);
        assert_reusable(&mut vm);
    }
}

fn caller_chunk(name: &str, callee: Value, after_call: Option<(Op, &[u8])>) -> Chunk {
    let mut chunk = Chunk::new(name);
    chunk.write_op(Op::BeginSettlement, 1);
    chunk.write_const(callee, 1);
    chunk.write_op(Op::Call, 1);
    chunk.write(0, 1);
    if let Some((op, operands)) = after_call {
        chunk.write_op(op, 1);
        for &byte in operands {
            chunk.write(byte, 1);
        }
    }
    chunk.write_op(Op::EndSettlement, 1);
    chunk.write_const(Value::NIL, 1);
    chunk.write_op(Op::Return, 1);
    chunk
}

#[test]
fn callee_cannot_close_a_caller_owned_settlement() {
    let mut vm = VM::new();
    let mut malicious = Chunk::new("callee-end-caller-settlement");
    malicious.write_op(Op::EndSettlement, 1);
    malicious.write_const(Value::NIL, 1);
    malicious.write_op(Op::Return, 1);
    let callee_id = vm.load_unchecked_chunk(malicious);
    let callee = Value::from_fn(
        vm.gc_mut(),
        FnValue {
            name: "malicious_end".to_string(),
            arity: 0,
            chunk_id: callee_id,
        },
    );
    let caller = vm
        .load_verified_chunk(caller_chunk("caller-owner", callee, None))
        .expect("caller chunk is structurally valid");
    let caller = callable(&mut vm, caller, "caller-owner");
    seed_observable_state(&mut vm);
    let before = vm.observable_state_signature();
    let error = vm
        .call_value(&caller, Vec::new())
        .expect_err("callee EndSettlement must fail");
    assert!(error.contains("cannot close settlement"), "{error}");
    assert_eq!(vm.observable_state_signature(), before);
    assert_reusable(&mut vm);
}

#[test]
fn callee_return_keeps_caller_settlement_active_and_firewalled() {
    let mut vm = VM::new();
    let callee_id = vm
        .load_verified_chunk(valid_return("ordinary-callee"))
        .expect("callee should verify");
    let callee = Value::from_fn(
        vm.gc_mut(),
        FnValue {
            name: "ordinary".to_string(),
            arity: 0,
            chunk_id: callee_id,
        },
    );
    let caller = vm
        .load_verified_chunk(caller_chunk(
            "caller-mutates-after-callee",
            callee,
            Some((Op::EcsSpawn, &[0, 0, 0, 0])),
        ))
        .expect("caller should verify");
    let caller = callable(&mut vm, caller, "caller-mutation");
    seed_observable_state(&mut vm);
    let before = vm.observable_state_signature();
    let error = vm
        .call_value(&caller, Vec::new())
        .expect_err("caller mutation must remain firewalled");
    assert!(error.contains("direct world mutation"), "{error}");
    assert_eq!(vm.observable_state_signature(), before);
    assert_reusable(&mut vm);
}

fn balanced_effect_chunk(name: &str, op: Op, operands: &[u8]) -> Chunk {
    let mut chunk = Chunk::new(name);
    chunk.write_op(Op::BeginSettlement, 1);
    chunk.write_op(op, 1);
    for &byte in operands {
        chunk.write(byte, 1);
    }
    chunk.write_op(Op::EndSettlement, 1);
    chunk.write_const(Value::NIL, 1);
    chunk.write_op(Op::Return, 1);
    chunk
}

#[test]
fn event_output_global_upvalue_and_task_effects_are_firewalled() {
    let cases = [
        ("event", Op::Emit, vec![]),
        ("output", Op::Print, vec![0]),
        ("global-definition", Op::DefGlobal, vec![0, 0]),
        ("global-assignment", Op::SetGlobal, vec![0, 0]),
        ("upvalue", Op::SetUpvalue, vec![0, 0]),
        ("task", Op::AsyncCall, vec![0]),
    ];
    for (name, op, operands) in cases {
        let mut vm = VM::new();
        let chunk = vm
            .load_verified_chunk(balanced_effect_chunk(name, op, &operands))
            .expect("balanced firewall probe should verify");
        let callee = callable(&mut vm, chunk, name);
        seed_observable_state(&mut vm);
        let before = vm.observable_state_signature();
        let error = vm
            .call_value(&callee, Vec::new())
            .expect_err("effect must be rejected");
        assert!(error.contains("Settlement effect firewall"), "{error}");
        assert_eq!(vm.observable_state_signature(), before);
        assert_reusable(&mut vm);
    }
}

#[test]
fn rng_builtin_is_rejected_before_advancing_rng_state() {
    let mut vm = VM::new();
    let mut chunk = Chunk::new("rng-firewall");
    let rand = Value::from_builtin(vm.gc_mut(), Builtin::RandInt);
    let low = Value::from_int(vm.gc_mut(), 0);
    let high = Value::from_int(vm.gc_mut(), 10);
    chunk.write_op(Op::BeginSettlement, 1);
    chunk.write_const(low, 1);
    chunk.write_const(high, 1);
    chunk.write_const(rand, 1);
    chunk.write_op(Op::Call, 1);
    chunk.write(2, 1);
    chunk.write_op(Op::EndSettlement, 1);
    chunk.write_const(Value::NIL, 1);
    chunk.write_op(Op::Return, 1);
    let id = vm
        .load_verified_chunk(chunk)
        .expect("rng firewall probe should verify");
    let callee = callable(&mut vm, id, "rng-firewall");
    seed_observable_state(&mut vm);
    let before = vm.observable_state_signature();
    let error = vm
        .call_value(&callee, Vec::new())
        .expect_err("RNG builtin must be rejected");
    assert!(error.contains("rand_int"), "{error}");
    assert_eq!(vm.observable_state_signature(), before);
    assert_reusable(&mut vm);
}

#[test]
fn fuzz_bytecode_verifier_never_panics_or_changes_its_answer() {
    let iterations = std::env::var("RAD_FUZZ_ITERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2_000);
    let mut state = 0xc411_5e77_1e5a_u64;
    for case in 0..iterations {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let len = (state as usize) & 0xff;
        let mut chunk = Chunk::new(&format!("fuzz-{case}"));
        for _ in 0..len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            chunk.write(state as u8, 1);
        }
        let first = std::panic::catch_unwind(|| crate::bytecode_verifier::verify_chunk(&chunk));
        assert!(first.is_ok(), "verifier panicked for case {case}");
        let first = first.unwrap();
        let second = crate::bytecode_verifier::verify_chunk(&chunk);
        assert_eq!(
            first.as_ref().map(|proof| proof.instruction_count),
            second.as_ref().map(|proof| proof.instruction_count),
            "verifier result changed for case {case}: {first:?} vs {second:?}"
        );
        assert_eq!(first.err(), second.err());
    }
}
