use super::replay_clone::fingerprint_world_snapshot;
use super::VM;

pub(crate) type NestedNativeResult<T> = (
    Result<T, String>,
    Result<Vec<crate::replay::NestedNativeCall>, String>,
);

#[derive(Clone)]
pub(crate) struct NestedNativePlan {
    boundary: String,
    digest: String,
    recording: bool,
    replay_lanes: Option<std::sync::Arc<Vec<Vec<crate::replay::NestedNativeCall>>>>,
    lane_count: usize,
}

impl NestedNativePlan {
    fn disabled(lane_count: usize) -> Self {
        Self {
            boundary: String::new(),
            digest: String::new(),
            recording: false,
            replay_lanes: None,
            lane_count,
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.recording || self.replay_lanes.is_some()
    }

    pub(crate) fn tape_for_lane(&self, lane: usize) -> Option<crate::replay::NestedNativeTape> {
        if self.recording {
            return Some(crate::replay::NestedNativeTape::recording());
        }
        self.replay_lanes
            .as_ref()
            .map(|lanes| crate::replay::NestedNativeTape::replaying(lanes[lane].clone()))
    }
}

impl VM {
    fn nested_native_digest(
        operation: &str,
        snapshots: &[crate::world::WorldSnapshot],
        systems: &[String],
        numeric_parameters: &[u64],
    ) -> Result<String, String> {
        fn field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"rad-nested-native-boundary/v1");
        field(&mut hasher, operation.as_bytes());
        for snapshot in snapshots {
            let fingerprint = fingerprint_world_snapshot(snapshot)
                .map_err(|error| format!("cannot fingerprint {operation} input: {error}"))?;
            field(&mut hasher, fingerprint.as_bytes());
        }
        for system in systems {
            field(&mut hasher, system.as_bytes());
        }
        for parameter in numeric_parameters {
            hasher.update(&parameter.to_le_bytes());
        }
        Ok(hasher.finalize().to_hex().to_string())
    }

    pub(crate) fn prepare_nested_native_simulation(
        &mut self,
        operation: &str,
        snapshots: &[crate::world::WorldSnapshot],
        systems: &[String],
        numeric_parameters: &[u64],
        lane_count: usize,
    ) -> Result<NestedNativePlan, String> {
        // The normal simulation hot path pays one pair of discriminant tests,
        // not a world fingerprint or a lane-table allocation.
        if self.recorder.is_none() && self.replayer.is_none() {
            return Ok(NestedNativePlan::disabled(lane_count));
        }
        if self.recorder.is_some() && self.replayer.is_some() {
            return Err(
                "internal replay state is invalid: recording and replay cannot be active together"
                    .into(),
            );
        }
        let digest = Self::nested_native_digest(operation, snapshots, systems, numeric_parameters)?;
        self.prepare_nested_native(operation, digest, lane_count)
    }

    pub(crate) fn prepare_nested_native_model_trial(
        &mut self,
        base: &crate::world::WorldSnapshot,
        trace: &[crate::vm::ModelTraceStep],
        seed: u64,
    ) -> Result<NestedNativePlan, String> {
        if self.recorder.is_none() && self.replayer.is_none() {
            return Ok(NestedNativePlan::disabled(1));
        }
        let mut parameters = Vec::with_capacity(2 + trace.len() * 2);
        parameters.extend([seed, trace.len() as u64]);
        for step in trace {
            parameters.push(step.command as u64);
            parameters.push(u64::from(step.flush_before) | (u64::from(step.flush_after) << 1));
        }
        self.prepare_nested_native_simulation(
            "model_trial",
            std::slice::from_ref(base),
            &[],
            &parameters,
            1,
        )
    }

    fn prepare_nested_native(
        &mut self,
        operation: &str,
        digest: String,
        lane_count: usize,
    ) -> Result<NestedNativePlan, String> {
        let boundary = format!("nested-native:v1:{operation}");
        let replay_lanes = if let Some(replayer) = self.replayer.as_mut() {
            let record = replayer.next_io(&boundary, &digest)?;
            let encoded = record.result?;
            Some(std::sync::Arc::new(
                crate::replay::decode_nested_native_lanes(&encoded, lane_count)?,
            ))
        } else {
            None
        };
        Ok(NestedNativePlan {
            boundary,
            digest,
            recording: self.recorder.is_some(),
            replay_lanes,
            lane_count,
        })
    }

    pub(crate) fn finish_nested_native(
        &mut self,
        plan: NestedNativePlan,
        lanes: &[Vec<crate::replay::NestedNativeCall>],
    ) -> Result<(), String> {
        if lanes.len() != plan.lane_count {
            return Err(format!(
                "internal nested-native lane mismatch: expected {}, got {}",
                plan.lane_count,
                lanes.len()
            ));
        }
        if plan.recording {
            let encoded = crate::replay::encode_nested_native_lanes(lanes);
            self.recorder
                .as_mut()
                .expect("recording plan retains its parent recorder")
                .record_io(&plan.boundary, plan.digest, &Ok(encoded));
        }
        Ok(())
    }

    pub(crate) fn finish_worker_native_tape(
        &mut self,
    ) -> Result<Vec<crate::replay::NestedNativeCall>, String> {
        self.nested_native_tape
            .take()
            .map(crate::replay::NestedNativeTape::finish)
            .unwrap_or_else(|| Ok(Vec::new()))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::ffi::{
        NativeAbiContract, NativeEffectSet, NativeExecution, NativeExtensionManifest,
    };
    use crate::value::{NativeFnInfo, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static LIVE_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    unsafe extern "C" fn echo_native(arguments: *const u64, count: usize) -> u64 {
        LIVE_CALLS.fetch_add(1, Ordering::SeqCst);
        if count == 1 {
            unsafe { *arguments }
        } else {
            Value::NIL.to_raw()
        }
    }

    fn compile_vm(source: &str) -> VM {
        let mut lexer = crate::lexer::Lexer::new(source);
        let tokens = lexer.tokenize().0;
        let program = crate::parser::Parser::new(tokens).parse();
        let result = crate::compiler::Compiler::new()
            .compile(&program)
            .expect("native simulation fixture compiles");
        let mut vm = VM::new_with_seed(7);
        vm.load_compile_result(result);
        vm.run(0).expect("native simulation fixture initializes");
        vm
    }

    fn native_info(execution: NativeExecution) -> NativeFnInfo {
        let contract = NativeAbiContract::parse(
            r#"{"contract_version":1,"calling_convention":"C","types":[]}"#,
        )
        .expect("empty ABI contract");
        let extension = std::sync::Arc::new(NativeExtensionManifest::from_binary(
            std::path::Path::new("nested-native-test.dll"),
            b"nested-native-test-generation-1",
            "nested.native.test".into(),
            "1".into(),
            &[],
            contract,
        ));
        NativeFnInfo {
            name: "echo".into(),
            execution,
            arity: 1,
            effects: NativeEffectSet::default(),
            signature: "(int)->int".into(),
            deterministic: true,
            replayable: true,
            extension,
        }
    }

    fn install_native(vm: &mut VM, execution: NativeExecution) {
        let slot = vm
            .global_names
            .iter()
            .position(|name| name == "EXTERNAL")
            .expect("EXTERNAL global");
        vm.globals[slot] = Value::from_native_fn(&mut vm.gc, native_info(execution));
    }

    fn simulate_lanes(vm: &mut VM, builtin: crate::value::Builtin) -> Result<Value, String> {
        let base = vm.call_builtin(crate::value::Builtin::Fork, Vec::new())?;
        let system = Value::system_ref(&mut vm.gc, "Apply".into());
        let schedule = Value::list(&mut vm.gc, vec![system]);
        let ticks = Value::from_int(&mut vm.gc, 1);
        let seed = Value::from_int(&mut vm.gc, 91);
        match builtin {
            crate::value::Builtin::SimulateMany => {
                let bases = Value::list(&mut vm.gc, vec![base, base]);
                vm.call_builtin(builtin, vec![bases, schedule, ticks, seed])
            }
            crate::value::Builtin::SimulatePar => {
                let lanes = Value::from_int(&mut vm.gc, 2);
                vm.call_builtin(builtin, vec![base, schedule, ticks, lanes, seed])
            }
            crate::value::Builtin::SimulateSeeded => {
                vm.call_builtin(builtin, vec![base, schedule, ticks, seed])
            }
            _ => unreachable!("test only exercises pooled simulation builtins"),
        }
    }

    fn assert_nested_native_replay(builtin: crate::value::Builtin, expected_calls: usize) {
        let _guard = TEST_LOCK.lock().expect("nested native replay test lock");
        const SOURCE: &str = r#"
            component Counter { value: int = 0 }
            let EXTERNAL: pure fn(int) -> int = fn(value: int) -> int { return value }
            system Apply(counter: mut Counter) {
                counter = Counter { value: EXTERNAL(counter.value) }
            }
            let SUBJECT = spawn("subject", Counter { value: 7 })
        "#;

        LIVE_CALLS.store(0, Ordering::SeqCst);
        let mut recording = compile_vm(SOURCE);
        install_native(&mut recording, NativeExecution::InProcess(echo_native));
        recording.enable_recording(SOURCE);
        simulate_lanes(&mut recording, builtin).expect("recorded rollouts");
        assert_eq!(LIVE_CALLS.load(Ordering::SeqCst), expected_calls);
        let trace = recording.take_trace().expect("trace");

        let mut replaying = compile_vm(SOURCE);
        install_native(&mut replaying, NativeExecution::Replay);
        let replayer = crate::replay::TraceReplayer::parse(&trace, false).expect("parse trace");
        replaying.enable_replay(replayer);
        simulate_lanes(&mut replaying, builtin).expect("replayed rollouts");
        assert_eq!(
            LIVE_CALLS.load(Ordering::SeqCst),
            expected_calls,
            "replay must consume lane tapes instead of invoking native code"
        );
        let report = replaying.finish_replay().expect("replay report");
        assert_eq!(report.leftover_io, 0);
        assert_eq!(report.end_digest_match, Some(true));
    }

    #[test]
    fn simulate_many_replays_lane_native_calls_without_live_execution() {
        assert_nested_native_replay(crate::value::Builtin::SimulateMany, 2);
    }

    #[test]
    fn simulate_par_replays_lane_native_calls_without_live_execution() {
        assert_nested_native_replay(crate::value::Builtin::SimulatePar, 2);
    }

    #[test]
    fn simulate_seeded_replays_lane_native_calls_without_live_execution() {
        assert_nested_native_replay(crate::value::Builtin::SimulateSeeded, 1);
    }

    #[test]
    fn nested_native_boundary_uses_the_full_blake3_digest() {
        let vm = VM::new_with_seed(7);
        let snapshot = vm.snapshot_with_events();
        let digest = VM::nested_native_digest("test", &[snapshot], &[], &[])
            .expect("empty world has a canonical operational fingerprint");
        assert_eq!(digest.len(), 64);
    }
}
