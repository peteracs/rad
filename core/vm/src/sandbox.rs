//! Capability model for speculative sandboxed execution (Feature #1).
//!
//! A `SandboxCaps` bounds what untrusted code (AI-generated plans, mods,
//! plugins) may do while running inside a forked world. It is the runtime
//! half of the three-layer enforcement model:
//!
//! 1. **static** — the checker already bans IO/event systems in `simulate`
//!    schedules; sandbox callers additionally run under a denied-builtin mask.
//! 2. **runtime ACL** — `bi_set` / `bi_spawn` / `bi_despawn` consult the
//!    component allowlist on every write when `sandbox_caps` is set.
//! 3. **physics** — `fuel` and `mem_limit` (see `VM::charge_fuel`) ensure that
//!    anything surviving layers 1 and 2 still dies of resource starvation.
//!
//! `commit` is deliberately not grantable: the host commits winners, untrusted
//! code never touches the live world directly.

use std::collections::HashSet;

use crate::types::Effect;
use crate::value::Builtin;

/// Default fuel grant: 10M charge points (loop iterations + calls).
pub const DEFAULT_FUEL: u64 = 10_000_000;
/// Default allocation ceiling: 64 MiB.
pub const DEFAULT_MEM_BYTES: usize = 64 * 1024 * 1024;
/// Default RNG seed for sandboxed runs (deterministic by default).
pub const DEFAULT_SEED: u64 = 1;

/// Deny-by-default builtin mask for sandboxed execution.
///
/// Everything with an IO/Async effect is denied (files, network, stdin,
/// clocks, dynamic extensions) except `print`/`eprint`, which are buffered
/// because sandbox VMs run with output suppressed, and `rand_*`, which is
/// deterministic under the per-run seed. The WHOLE speculation/persistence
/// family (`fork`/`simulate*`/`fork_*`/`merge_forks*`/`load_world`/
/// `try_load_world`/`sandbox_run`/`commit`) is denied by name to prevent
/// nesting and wholesale world replacement; `commit` in particular is never
/// grantable — only the host commits.
pub fn builtin_allowed_in_sandbox(builtin: Builtin) -> bool {
    match builtin {
        // Buffered, host-inspectable output.
        Builtin::Print | Builtin::Eprint => true,
        // Guest randomness is a supported, deterministic feature: every
        // sandboxed run is seeded (DEFAULT_SEED or the grant's seed), so
        // rand_* stays allowed even though its honest effect row is IO.
        Builtin::RandInt | Builtin::RandFloat | Builtin::RandBool | Builtin::RandSeed => true,
        // Speculation family: no nesting, no committing from inside. The
        // whole family must be listed by name, because its members carry the
        // ECS effect (not IO/Async) and would otherwise fall through to the
        // allow arm — which is exactly what happened to the newer members:
        // simulate_many/simulate_seeded/fork_with nested speculation, and
        // load_world/try_load_world/fork_from_bytes/fork_apply/merge_forks
        // replaced or rewrote the guest world WHOLESALE, bypassing the
        // per-component write ACL that gates set/spawn/set_resource
        // (dogfood seq 254 residual audit, item 4).
        Builtin::Fork
        | Builtin::Simulate
        | Builtin::SimulatePar
        | Builtin::SimulateMany
        | Builtin::SimulateSeeded
        | Builtin::ForkWith
        | Builtin::ForkFromBytes
        | Builtin::ForkApply
        | Builtin::MergeForks
        | Builtin::MergeForksWith
        | Builtin::LoadWorld
        | Builtin::TryLoadWorld
        | Builtin::SandboxRun
        | Builtin::Commit
        | Builtin::Peek => false,
        // Host environment probes.
        Builtin::SysArgs | Builtin::LoadExtension | Builtin::GcCollect => false,
        // Everything else: deny if it carries IO or Async effects
        // (files, network, stdin, clocks, metrics), allow otherwise.
        b => {
            let effects = crate::builtins::builtin_effect(b.name());
            !effects.allows(Effect::IO) && !effects.allows(Effect::Async)
        }
    }
}

/// Capability grant for a single sandboxed simulation.
#[derive(Clone, Debug)]
pub struct SandboxCaps {
    /// Component types the sandbox may write via `set` / `spawn`.
    /// Empty set = no writes permitted at all.
    pub writable_components: HashSet<String>,
    /// Component/resource types the sandbox may read via `get` / `res` /
    /// `query` / … A grant with no `"read"` key gets `{"*"}` (read
    /// everything), so the read dimension is opt-in and existing grants keep
    /// their prior behavior. `{"*"}` in the set is the wildcard; an explicit
    /// list is an allowlist and also gates the bulk readers (which require
    /// the wildcard, mirroring how `despawn` requires the `"*"` write grant).
    pub readable_components: HashSet<String>,
    /// Instruction budget charged on loop back-edges and calls.
    pub fuel: u64,
    /// GC allocation ceiling in bytes.
    pub mem_limit: usize,
}

impl SandboxCaps {
    /// Trusted-constructor default: reads everything (`{"*"}`), matching the
    /// pre-read-dimension behavior. `from_json` overrides `readable_components`
    /// when the grant carries an explicit `"read"` key.
    pub fn new(writable_components: HashSet<String>, fuel: u64, mem_limit: usize) -> Self {
        SandboxCaps {
            writable_components,
            readable_components: HashSet::from(["*".to_string()]),
            fuel,
            mem_limit,
        }
    }

    /// Whether a write to `component` is permitted by this grant.
    pub fn may_write(&self, component: &str) -> bool {
        self.writable_components.contains("*") || self.writable_components.contains(component)
    }

    /// Structural changes (`despawn`) require the wildcard grant, since they
    /// touch every component on the entity.
    pub fn may_despawn(&self) -> bool {
        self.writable_components.contains("*")
    }

    /// Whether a read of `component` is permitted by this grant.
    pub fn may_read(&self, component: &str) -> bool {
        self.readable_components.contains("*") || self.readable_components.contains(component)
    }

    /// Bulk readers that dump or enumerate the whole world (`save_world`,
    /// `world_digest`, `entities()` with no filter) cannot be keyed to a
    /// single component, so they require the wildcard read grant — the same
    /// precedent as `despawn` requiring the wildcard write grant.
    pub fn may_read_all(&self) -> bool {
        self.readable_components.contains("*")
    }

    /// Parse a capability grant from its JSON wire format:
    ///
    /// ```json
    /// { "write": ["Health", "PlanBuffer"], "fuel": 1000000,
    ///   "mem_bytes": 16777216, "seed": 42 }
    /// ```
    ///
    /// Missing keys fall back to defaults; `write` defaults to empty (deny all
    /// writes). Returns `(caps, seed)`.
    pub fn from_json(text: &str) -> Result<(SandboxCaps, u64), String> {
        let parsed: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("sandbox caps: invalid JSON: {}", e))?;
        let obj = parsed
            .as_object()
            .ok_or_else(|| "sandbox caps: expected a JSON object".to_string())?;

        let mut writable = HashSet::new();
        if let Some(w) = obj.get("write") {
            let arr = w.as_array().ok_or_else(|| {
                "sandbox caps: 'write' must be an array of component names".to_string()
            })?;
            for item in arr {
                let name = item
                    .as_str()
                    .ok_or_else(|| "sandbox caps: 'write' entries must be strings".to_string())?;
                writable.insert(name.to_string());
            }
        }
        // `read` is symmetric with `write`; a MISSING key means "read
        // everything" (`{"*"}`) so a pre-read-dimension grant is unchanged,
        // while a PRESENT key — even `[]` — is an explicit allowlist that
        // also denies the bulk readers. `[]` therefore means "read nothing",
        // exactly as `write: []` means "write nothing".
        let readable = match obj.get("read") {
            Some(r) => {
                let arr = r.as_array().ok_or_else(|| {
                    "sandbox caps: 'read' must be an array of component names".to_string()
                })?;
                let mut set = HashSet::new();
                for item in arr {
                    let name = item.as_str().ok_or_else(|| {
                        "sandbox caps: 'read' entries must be strings".to_string()
                    })?;
                    set.insert(name.to_string());
                }
                set
            }
            None => HashSet::from(["*".to_string()]),
        };
        let fuel = match obj.get("fuel") {
            Some(v) => v
                .as_u64()
                .ok_or_else(|| "sandbox caps: 'fuel' must be a non-negative integer".to_string())?,
            None => DEFAULT_FUEL,
        };
        let mem_limit = match obj.get("mem_bytes") {
            Some(v) => v.as_u64().ok_or_else(|| {
                "sandbox caps: 'mem_bytes' must be a non-negative integer".to_string()
            })? as usize,
            None => DEFAULT_MEM_BYTES,
        };
        let seed = match obj.get("seed") {
            Some(v) => v
                .as_u64()
                .ok_or_else(|| "sandbox caps: 'seed' must be a non-negative integer".to_string())?,
            None => DEFAULT_SEED,
        };
        for key in obj.keys() {
            if !matches!(
                key.as_str(),
                "write" | "read" | "fuel" | "mem_bytes" | "seed"
            ) {
                return Err(format!("sandbox caps: unknown key '{}'", key));
            }
        }
        let mut caps = SandboxCaps::new(writable, fuel, mem_limit);
        caps.readable_components = readable;
        Ok((caps, seed))
    }
}

/// Result of running untrusted source in a guest VM (see
/// `VM::run_sandbox_guest`). Everything here is plain data — no values from
/// the guest heap survive into this struct.
pub struct SandboxOutcome {
    /// The guest's final world on success, or its failure message (compile
    /// error, capability denial, budget exhaustion, runtime error).
    pub result: Result<crate::world::WorldSnapshot, String>,
    /// Buffered guest `print` output.
    pub prints: Vec<String>,
    /// Fuel consumed (charge points crossed: loop back-edges and calls).
    pub fuel_spent: u64,
    /// JSON set by the guest's last `sandbox_output(v)` call, if any.
    pub output_json: Option<String>,
}

/// Deterministic per-fork seed derivation (SplitMix64 finalizer).
///
/// Used so that `simulate_par(world, schedule, ticks, n, seed)` produces
/// bit-identical results regardless of how many threads execute the forks.
#[inline]
pub fn fork_seed(parent_seed: u64, fork_index: u64) -> u64 {
    let mut z = parent_seed.wrapping_add(fork_index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let out = z ^ (z >> 31);
    if out == 0 {
        0xD1B5_4A32_D192_ED03
    } else {
        out
    }
}

/// Escape-attempt suite (ratified spec: ships *with* the sandbox, not after).
///
/// Each test is an attack class: IO smuggling, ACL bypass via builtins and via
/// compiled opcodes, fuel/memory bombs, speculation-family nesting, and the
/// captured-events semantics that distinguish `sandbox_run` from `simulate`.
#[cfg(test)]
mod escape_tests {
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::value::{Builtin, Value};
    use crate::vm::VM;

    /// Compile and run trusted host source, returning the live VM.
    fn host_vm(src: &str) -> VM {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "host parse errors: {:?}",
            parser.errors()
        );
        let result = Compiler::new().compile(&program).expect("host compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.load_compile_result(result);
        vm.run(0).expect("host run");
        vm
    }

    /// Like `host_vm`, but compiled through the checker so the host's
    /// declared component field types reach the VM — required for the
    /// write-shape ACL, which binds a guest write to the host's schema.
    fn host_vm_checked(src: &str) -> VM {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "host parse errors: {:?}",
            parser.errors()
        );
        let mut checker = crate::checker::Checker::new();
        let errors = checker.check(&program);
        assert!(errors.is_empty(), "host check errors: {:?}", errors);
        let result = Compiler::new()
            .with_checker_output(checker.output())
            .compile(&program)
            .expect("host compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.load_compile_result(result);
        vm.run(0).expect("host run");
        vm
    }

    /// Fork the host world and run untrusted `guest_src` under `caps_json`.
    /// Returns (succeeded, payload): payload is the result fork on success or
    /// the error message value on failure.
    fn sandbox(vm: &mut VM, guest_src: &str, caps_json: &str) -> (bool, Value) {
        let fork = vm.call_builtin(Builtin::Fork, vec![]).expect("fork");
        let src_v = Value::from_string(vm.gc_mut(), guest_src.to_string());
        let caps_v = Value::from_string(vm.gc_mut(), caps_json.to_string());
        let res = vm
            .call_builtin(Builtin::SandboxRun, vec![src_v, fork, caps_v])
            .expect("sandbox_run itself must not abort the host");
        let st = res.as_sum_type().expect("sandbox_run returns Result");
        assert_eq!(st.type_name, "Result");
        // Ok carries `value`; Err carries `message` (language convention).
        let field = if st.variant == "Ok" {
            "value"
        } else {
            "message"
        };
        let inner = st.fields.get(field).copied().unwrap_or(Value::NIL);
        (st.variant == "Ok", inner)
    }

    fn err_text(vm: &mut VM, guest_src: &str, caps_json: &str) -> String {
        let (ok, payload) = sandbox(vm, guest_src, caps_json);
        assert!(!ok, "expected sandbox failure, got Ok");
        payload
            .as_str()
            .expect("Err payload should be a string")
            .to_string()
    }

    /// Peek `component` on named entity inside a fork; returns the first field
    /// as i64.
    fn peek_field(vm: &mut VM, fork: Value, entity_name: &str, component: &str) -> i64 {
        let name_v = Value::from_string(vm.gc_mut(), entity_name.to_string());
        let ent = vm
            .call_builtin(Builtin::GetEntity, vec![name_v])
            .expect("get_entity");
        let comp_v = Value::from_string(vm.gc_mut(), component.to_string());
        let peeked = vm
            .call_builtin(Builtin::Peek, vec![fork, ent, comp_v])
            .expect("peek");
        let st = peeked.as_sum_type().expect("peek returns Option");
        assert_eq!(
            st.variant, "Some",
            "expected component '{}' present",
            component
        );
        let comp = st.fields.get("value").copied().unwrap();
        let data = comp.as_component().expect("component data");
        data.values[0].as_int().expect("int field")
    }

    const HOST: &str = r#"
        component Health { hp: 100 }
        component Gold { amount: 1000 }
        let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 1000 })
    "#;

    // -- Attack class 1: IO builtins ------------------------------------

    #[test]
    fn escape_io_builtin_denied() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"write_file("pwned.txt", "data")"#,
            r#"{ "write": ["Health"] }"#,
        );
        assert!(msg.contains("not permitted"), "got: {}", msg);
        assert!(!std::path::Path::new("pwned.txt").exists());
    }

    #[test]
    fn escape_network_builtin_denied() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"http_get("http://example.com")"#,
            r#"{ "write": ["*"] }"#,
        );
        assert!(msg.contains("not permitted"), "got: {}", msg);
    }

    // -- Attack class 2: component-write ACL -----------------------------

    #[test]
    fn escape_write_outside_grant_denied() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                component Gold { amount: 1000 }
                set(get_entity("hero"), Gold { amount: 999999 })
            "#,
            r#"{ "write": ["Health"] }"#,
        );
        assert!(msg.contains("denied by capability grant"), "got: {}", msg);
    }

    #[test]
    fn escape_spawn_outside_grant_denied() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                component Gold { amount: 1000 }
                spawn("forged", Gold { amount: 999999 })
            "#,
            r#"{ "write": ["Health"] }"#,
        );
        assert!(msg.contains("denied by capability grant"), "got: {}", msg);
    }

    #[test]
    fn escape_despawn_requires_wildcard() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"despawn(require_entity("hero"))"#,
            r#"{ "write": ["Health", "Gold"] }"#,
        );
        assert!(msg.contains("despawn denied"), "got: {}", msg);

        // Wildcard grant permits it.
        let (ok, _) = sandbox(
            &mut vm,
            r#"despawn(require_entity("hero"))"#,
            r#"{ "write": ["*"] }"#,
        );
        assert!(ok, "wildcard grant should permit despawn");
    }

    #[test]
    fn escape_system_mut_outside_grant_denied() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                component Gold { amount: 1000 }
                system Drain(g: mut Gold) {
                    g = Gold { amount: 0 }
                }
                schedule [system::Drain]
            "#,
            r#"{ "write": ["Health"] }"#,
        );
        assert!(
            msg.contains("mutable access") && msg.contains("denied"),
            "got: {}",
            msg
        );
    }

    // -- Attack class 3: resource starvation ------------------------------

    #[test]
    fn escape_infinite_loop_starves() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                let mut i = 0
                while true {
                    i = i + 1
                }
            "#,
            r#"{ "write": [], "fuel": 50000 }"#,
        );
        assert!(msg.contains("fuel"), "got: {}", msg);
    }

    #[test]
    fn escape_memory_bomb_starves() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                let mut xs = []
                while true {
                    xs = push(xs, "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
                }
            "#,
            r#"{ "write": [], "fuel": 100000000, "mem_bytes": 1048576 }"#,
        );
        assert!(msg.contains("memory limit"), "got: {}", msg);
    }

    /// A self-re-emitting handler is an infinite loop that never touches the
    /// bytecode charge points: the drain loop is Rust, and a body of
    /// `emit P { .. }` has no call and no loop back-edge. Before the drain
    /// loop charged fuel, this ran forever under any budget — including
    /// `fuel: 1, mem_bytes: 1024`.
    #[test]
    fn escape_event_bomb_starves() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                event P { n }
                on P(e) {
                    emit P { n: e.n + 1 }
                }
                emit P { n: 0 }
            "#,
            r#"{ "write": [], "fuel": 500, "mem_bytes": 1048576 }"#,
        );
        assert!(msg.contains("Budget exhausted"), "got: {}", msg);
    }

    /// The sandbox mask names the speculation family explicitly because its
    /// members carry the ECS effect and would otherwise fall through to the
    /// allow arm. The newer members did exactly that: `load_world` REPLACED
    /// the guest world wholesale (bypassing the per-component write ACL that
    /// gates set/spawn/set_resource), and `simulate_many` nested speculation.
    #[test]
    fn escape_world_replacement_and_nested_speculation_denied() {
        let mut vm = host_vm(HOST);
        let msg = err_text(&mut vm, r#"let n = load_world("{}")"#, r#"{ "write": [] }"#);
        assert!(
            msg.contains("'load_world' is not permitted"),
            "got: {}",
            msg
        );
        let msg = err_text(
            &mut vm,
            r#"let outs = simulate_many([], [], 1, 7)"#,
            r#"{ "write": [] }"#,
        );
        assert!(
            msg.contains("'simulate_many' is not permitted"),
            "got: {}",
            msg
        );
    }

    /// Guest randomness is a supported deterministic feature (every run is
    /// seeded), so rand_* stays allowed even though its honest effect row in
    /// builtin_effect is now IO.
    #[test]
    fn guest_rand_still_allowed() {
        let mut vm = host_vm(HOST);
        let (ok, _payload) = sandbox(
            &mut vm,
            r#"
                let r = rand_int(1, 6)
                sandbox_output(r)
            "#,
            r#"{ "write": [] }"#,
        );
        assert!(ok, "seeded guest rand must still run");
    }

    /// The other side of the same fix: charging the drain must not starve a
    /// guest whose handlers legitimately terminate. This one fans out to
    /// three generations and must still return Ok.
    #[test]
    fn bounded_event_chain_still_completes() {
        let mut vm = host_vm(HOST);
        let (ok, _payload) = sandbox(
            &mut vm,
            r#"
                event P { n }
                on P(e) {
                    if e.n < 3 {
                        emit P { n: e.n + 1 }
                    }
                }
                emit P { n: 0 }
            "#,
            r#"{ "write": [], "fuel": 500, "mem_bytes": 1048576 }"#,
        );
        assert!(ok, "a terminating event chain must not be starved");
    }

    // -- Attack class 4: speculation-family nesting ----------------------

    #[test]
    fn escape_fork_commit_denied_inside_sandbox() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                let f = fork()
                commit(f)
            "#,
            r#"{ "write": ["*"] }"#,
        );
        assert!(msg.contains("not permitted"), "got: {}", msg);
    }

    #[test]
    fn escape_nested_sandbox_denied() {
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                let f = fork()
                sandbox_run("1 + 1", f, "{}")
            "#,
            r#"{ "write": ["*"] }"#,
        );
        assert!(msg.contains("not permitted"), "got: {}", msg);
    }

    // -- Attack class 5: compile-time smuggling --------------------------

    #[test]
    fn escape_module_import_rejected() {
        let mut vm = host_vm(HOST);
        let msg = err_text(&mut vm, "use \"evil\"\n", r#"{ "write": ["*"] }"#);
        assert!(msg.contains("module imports"), "got: {}", msg);
    }

    #[test]
    fn sandbox_input_round_trips_host_value() {
        // 4th sandbox_run argument crosses the boundary as JSON and comes
        // back through the guest's sandbox_input() — the typed channel that
        // replaces splicing host data into guest source text.
        let mut vm = host_vm(HOST);
        let fork = vm.call_builtin(Builtin::Fork, vec![]).expect("fork");
        let src = r#"
            component Health { hp: 100 }
            let input = sandbox_input()
            set(get_entity("hero"), Health { hp: input })
        "#;
        let src_v = Value::from_string(vm.gc_mut(), src.to_string());
        let caps_v = Value::from_string(vm.gc_mut(), r#"{ "write": ["Health"] }"#.to_string());
        let input_v = Value::from_int(vm.gc_mut(), 41);
        let res = vm
            .call_builtin(Builtin::SandboxRun, vec![src_v, fork, caps_v, input_v])
            .expect("sandbox_run with input");
        let st = res.as_sum_type().expect("Result");
        assert_eq!(st.variant, "Ok", "guest should succeed: {:?}", st.fields);
        let out_fork = st.fields.get("value").copied().unwrap();
        assert_eq!(peek_field(&mut vm, out_fork, "hero", "Health"), 41);
    }

    #[test]
    fn guest_compile_error_is_err_not_host_abort() {
        let mut vm = host_vm(HOST);
        let (ok, _) = sandbox(&mut vm, "let = (((", r#"{}"#);
        assert!(!ok);
        // Host still functional after guest compile failure.
        let f = vm.call_builtin(Builtin::Fork, vec![]).expect("host alive");
        assert!(f.as_world_fork().is_some());
    }

    // -- Granted-path behavior (the sandbox must also *work*) ------------

    #[test]
    fn granted_write_lands_in_fork_not_live_world() {
        let mut vm = host_vm(HOST);
        let (ok, fork) = sandbox(
            &mut vm,
            r#"
                component Health { hp: 100 }
                set(get_entity("hero"), Health { hp: 7 })
            "#,
            r#"{ "write": ["Health"] }"#,
        );
        assert!(ok);
        assert_eq!(peek_field(&mut vm, fork, "hero", "Health"), 7);

        // Live world untouched until the host commits.
        let live = vm.call_builtin(Builtin::Fork, vec![]).expect("fork");
        assert_eq!(peek_field(&mut vm, live, "hero", "Health"), 100);

        // Host commits the proposal; live world now reflects it.
        vm.call_builtin(Builtin::Commit, vec![fork])
            .expect("commit");
        let live = vm.call_builtin(Builtin::Fork, vec![]).expect("fork");
        assert_eq!(peek_field(&mut vm, live, "hero", "Health"), 7);
    }

    #[test]
    fn captured_events_run_inside_sandbox() {
        // Unlike simulate(), guest emits are not dropped: the guest VM owns
        // private event queues, and pending events are drained after main.
        let mut vm = host_vm(HOST);
        let (ok, fork) = sandbox(
            &mut vm,
            r#"
                component Health { hp: 100 }
                event Strike { dmg }
                on Strike(e) {
                    set(get_entity("hero"), Health { hp: 100 - e.dmg })
                }
                emit Strike { dmg: 60 }
            "#,
            r#"{ "write": ["Health"] }"#,
        );
        assert!(ok);
        assert_eq!(peek_field(&mut vm, fork, "hero", "Health"), 40);
    }

    #[test]
    fn guest_handler_write_outside_grant_denied() {
        // ACL also binds event handlers, not just top-level code.
        let mut vm = host_vm(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                component Gold { amount: 1000 }
                event Loot { amt }
                on Loot(e) {
                    set(get_entity("hero"), Gold { amount: e.amt })
                }
                emit Loot { amt: 999999 }
            "#,
            r#"{ "write": ["Health"] }"#,
        );
        assert!(msg.contains("denied by capability grant"), "got: {}", msg);
    }

    // -- Attack class 6: write-shape ACL (type/aliasing/drop, squatting) --

    #[test]
    fn escape_type_confused_write_denied() {
        // A guest that stays inside its grant but writes the granted
        // component with a wrong-typed field used to succeed, planting a str
        // in an int-declared field and poisoning trusted host code later.
        // The write is now rejected at the boundary against the host schema.
        let mut vm = host_vm_checked(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                component Gold { amount: "" }
                set(get_entity("hero"), Gold { amount: "not_a_number" })
            "#,
            r#"{ "write": ["Gold"] }"#,
        );
        assert!(
            msg.contains("host declares") && msg.contains("int"),
            "got: {}",
            msg
        );
    }

    #[test]
    fn escape_field_aliased_write_denied() {
        // Guest declares the granted component with a DIFFERENT field name;
        // fields are stored by position, so this used to alias into the
        // host's column. The exact-schema check rejects it.
        let mut vm = host_vm_checked(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                component Gold { balance: 0 }
                set(get_entity("hero"), Gold { balance: 999999 })
            "#,
            r#"{ "write": ["Gold"] }"#,
        );
        assert!(msg.contains("exact schema"), "got: {}", msg);
    }

    #[test]
    fn escape_field_widened_write_denied() {
        // Guest adds an extra field to the granted component; it used to be
        // silently dropped. The exact-schema check rejects the mismatch.
        let mut vm = host_vm_checked(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                component Gold { amount: 0, backdoor: 0 }
                set(get_entity("hero"), Gold { amount: 1, backdoor: 1 })
            "#,
            r#"{ "write": ["Gold"] }"#,
        );
        assert!(msg.contains("exact schema"), "got: {}", msg);
    }

    #[test]
    fn escape_entity_name_squat_denied() {
        // A guest holding the narrowest grant used to shadow any host entity
        // name via spawn(), orphaning the host's entity while diff/
        // assert_only_changed reported only the new component. Spawning an
        // existing name inside a sandbox is now denied.
        let mut vm = host_vm_checked(HOST);
        let msg = err_text(
            &mut vm,
            r#"
                component Health { hp: 100 }
                spawn("hero", Health { hp: 1 })
            "#,
            r#"{ "write": ["Health"] }"#,
        );
        assert!(msg.contains("already exists"), "got: {}", msg);
    }

    // -- Attack class 7: output provenance ------------------------------

    #[test]
    fn guest_debug_trace_is_buffered_and_tagged() {
        // debug_trace was a third guest output channel nobody enumerated:
        // it wrote raw to the host's stderr, untagged and out of order,
        // while the sandbox's output contract is "buffered, tagged,
        // host-inspectable". Attacker-controlled text was indistinguishable
        // from the trusted host's own DEBUG lines. Inside a guest it must
        // arrive like print does — through the [sandbox]-tagged path, in
        // execution order — and keep its pass-through return value.
        let mut vm = host_vm(HOST);
        vm.print_buffer.clear();
        let (ok, _) = sandbox(
            &mut vm,
            r#"
                print("first")
                debug_trace("attacker-controlled text")
                let x = debug_trace(21) * 2
                print(f"x={x}")
            "#,
            r#"{ "write": [] }"#,
        );
        assert!(ok, "guest with debug_trace must still run");
        assert_eq!(
            vm.print_buffer,
            vec![
                "[sandbox] first",
                "[sandbox] DEBUG: attacker-controlled text",
                "[sandbox] DEBUG: 21",
                "[sandbox] x=42",
            ],
        );
    }

    // -- In-language guest->host result + fuel telemetry (seq 62) --------

    #[test]
    fn sandbox_output_and_fuel_readable_from_host() {
        let mut vm = host_vm(HOST);

        // Before any sandbox_run: nil output, zero fuel.
        let out0 = vm
            .call_builtin(Builtin::SandboxLastOutput, vec![])
            .expect("last_output");
        assert!(out0.is_nil(), "no output before any run");
        let fuel0 = vm
            .call_builtin(Builtin::SandboxLastFuel, vec![])
            .expect("last_fuel");
        assert_eq!(fuel0.as_int(), Some(0), "no fuel before any run");

        // A guest that reports a structured result and burns fuel in a loop.
        let (ok, _) = sandbox(
            &mut vm,
            r#"
                let mut i = 0
                while i < 50 { i = i + 1 }
                sandbox_output({ "setpoint": 55, "ok": true })
            "#,
            r#"{ "write": [] }"#,
        );
        assert!(ok, "guest should run");

        // The structured value round-trips onto the host heap (JSON in reverse).
        let out = vm
            .call_builtin(Builtin::SandboxLastOutput, vec![])
            .expect("last_output");
        let m = out.as_map().expect("output parses to a map");
        assert_eq!(
            m.get(&crate::value::MapKey::Str("setpoint".to_string()))
                .and_then(|v| v.as_int()),
            Some(55)
        );
        assert_eq!(
            m.get(&crate::value::MapKey::Str("ok".to_string()))
                .and_then(|v| v.as_bool()),
            Some(true)
        );

        // Fuel was metered: the loop crossed charge points.
        let fuel = vm
            .call_builtin(Builtin::SandboxLastFuel, vec![])
            .expect("last_fuel");
        assert!(
            fuel.as_int().unwrap_or(0) > 0,
            "the guest's loop should have spent fuel, got {:?}",
            fuel.as_int()
        );
    }

    #[test]
    fn sandbox_last_output_is_nil_when_guest_reports_none() {
        // A guest that never calls sandbox_output leaves the host output nil,
        // but fuel still reflects the run.
        let mut vm = host_vm(HOST);
        let (ok, _) = sandbox(&mut vm, r#"let x = 1 + 1"#, r#"{ "write": [] }"#);
        assert!(ok);
        let out = vm
            .call_builtin(Builtin::SandboxLastOutput, vec![])
            .expect("last_output");
        assert!(out.is_nil(), "no sandbox_output => nil");
    }

    #[test]
    fn sandbox_output_reflects_only_the_most_recent_run() {
        // Each run overwrites the telemetry: a later run that reports nothing
        // clears the earlier structured output rather than leaking it.
        let mut vm = host_vm(HOST);
        let (ok1, _) = sandbox(&mut vm, r#"sandbox_output(99)"#, r#"{ "write": [] }"#);
        assert!(ok1);
        let first = vm
            .call_builtin(Builtin::SandboxLastOutput, vec![])
            .expect("last_output");
        assert_eq!(first.as_int(), Some(99));

        let (ok2, _) = sandbox(&mut vm, r#"let y = 2"#, r#"{ "write": [] }"#);
        assert!(ok2);
        let second = vm
            .call_builtin(Builtin::SandboxLastOutput, vec![])
            .expect("last_output");
        assert!(
            second.is_nil(),
            "second run reported nothing => nil, not stale 99"
        );
    }

    // -- Attack class 9: the read (confidentiality) dimension ------------

    const SECRETS: &str = r#"
        component Health { hp: 100 }
        component Vault { gold: 5000 }
        component ApiToken { secret: 42 }
        let hero = spawn("hero", Health { hp: 100 })
        let bank = spawn("bank", Vault { gold: 5000 }, ApiToken { secret: 42 })
    "#;

    #[test]
    fn read_grant_absent_reads_everything_backcompat() {
        // A grant with no "read" key must behave exactly as before the read
        // dimension existed: the guest can read anything. This is what keeps
        // every pre-existing grant working.
        let mut vm = host_vm_checked(SECRETS);
        let (ok, _) = sandbox(
            &mut vm,
            r#"
                component Vault { gold: 5000 }
                let v = get(get_entity("bank"), Vault) |> unwrap
                print(f"gold={v.gold}")
            "#,
            r#"{ "write": [] }"#,
        );
        assert!(ok, "absent read key must grant read-all");
    }

    #[test]
    fn targeted_read_outside_grant_denied() {
        // The core confidentiality property (A3 probe_readcap): a guest
        // granted read on one component cannot read another, even though the
        // write grant is unchanged. Previously write:["Reactor"] leaked every
        // secret in the world through get().
        let mut vm = host_vm_checked(SECRETS);
        let msg = err_text(
            &mut vm,
            r#"
                component Vault { gold: 5000 }
                let v = get(get_entity("bank"), Vault) |> unwrap
                print(f"exfil gold={v.gold}")
            "#,
            r#"{ "write": [], "read": ["Health"] }"#,
        );
        assert!(
            msg.contains("read of component 'Vault' denied"),
            "got: {}",
            msg
        );
    }

    #[test]
    fn granted_read_inside_grant_allowed() {
        // The dual: a component named in the read grant is readable.
        let mut vm = host_vm_checked(SECRETS);
        let (ok, _) = sandbox(
            &mut vm,
            r#"
                component Health { hp: 100 }
                let h = get(get_entity("hero"), Health) |> unwrap
                print(f"hp={h.hp}")
            "#,
            r#"{ "write": [], "read": ["Health"] }"#,
        );
        assert!(ok, "component in the read grant must be readable");
    }

    #[test]
    fn query_and_entities_honor_read_grant() {
        // Enumeration is a read: query on an ungranted component is denied,
        // and the unfiltered entities() dump requires the wildcard.
        let mut vm = host_vm_checked(SECRETS);
        let q = err_text(
            &mut vm,
            "component Vault { gold: 0 }\nlet n = query_count(Vault)\nprint(f\"{n}\")",
            r#"{ "write": [], "read": ["Health"] }"#,
        );
        assert!(q.contains("read of component 'Vault' denied"), "got: {}", q);

        let e = err_text(
            &mut vm,
            "let all = entities()\nprint(f\"{len(all)}\")",
            r#"{ "write": [], "read": ["Health"] }"#,
        );
        assert!(
            e.contains("entities()") && e.contains("\"*\" read grant"),
            "got: {}",
            e
        );
    }

    #[test]
    fn bulk_dump_requires_wildcard_read() {
        // save_world() is the bulk-exfil channel from A3's evidence 2: a full
        // world dump under a narrow grant. It now requires the wildcard read.
        let mut vm = host_vm_checked(SECRETS);
        let msg = err_text(
            &mut vm,
            "let dump = save_world()\nprint(f\"{len(dump)}\")",
            r#"{ "write": ["Health"], "read": ["Health"] }"#,
        );
        assert!(
            msg.contains("save_world()") && msg.contains("\"*\" read grant"),
            "got: {}",
            msg
        );

        // The wildcard read grant permits it.
        let (ok, _) = sandbox(
            &mut vm,
            "let dump = save_world()\nprint(f\"{len(dump)}\")",
            r#"{ "write": [], "read": ["*"] }"#,
        );
        assert!(ok, "wildcard read must permit save_world()");
    }

    #[test]
    fn read_system_param_outside_grant_denied() {
        // A system with a read (non-mut) param reads that component, so it is
        // gated symmetrically with the mut-param write check.
        let mut vm = host_vm_checked(SECRETS);
        let msg = err_text(
            &mut vm,
            r#"
                component Vault { gold: 5000 }
                system Peeker(v: Vault) {
                    print(f"peeked {v.gold}")
                }
                schedule [system::Peeker]
            "#,
            r#"{ "write": [], "read": ["Health"] }"#,
        );
        assert!(
            msg.contains("reads component 'Vault'") && msg.contains("denied"),
            "got: {}",
            msg
        );
    }

    // -- Attack class 8: malformed capability grants ---------------------

    #[test]
    fn malformed_caps_return_err_not_host_abort() {
        // Caps are computed from plugin manifests in a real host, so a bad
        // grant is attacker-influenced input. It used to be the one
        // sandbox_run input that killed the host process before the ACL,
        // builtin mask, or budgets ever ran; it must come back through the
        // same Err arm as every other failure, with the parser's specific
        // message intact.
        let mut vm = host_vm(HOST);
        let cases: &[(&str, &str)] = &[
            (r#"{ "write": "Reactor" }"#, "'write' must be an array"),
            (
                r#"{ "write": [], "fuel": -1 }"#,
                "'fuel' must be a non-negative integer",
            ),
            (
                r#"{ "write": [], "allow_everything": true }"#,
                "unknown key 'allow_everything'",
            ),
            (r#"definitely not json"#, "invalid JSON"),
        ];
        for (caps, expected) in cases {
            let msg = err_text(&mut vm, r#"print("never runs")"#, caps);
            assert!(
                msg.contains("sandbox caps:") && msg.contains(expected),
                "caps {caps} -> got: {msg}"
            );
        }
        // Host is still alive and functional after all four rejections.
        let f = vm.call_builtin(Builtin::Fork, vec![]).expect("host alive");
        assert!(f.as_world_fork().is_some());
    }

    #[test]
    fn guest_rng_is_seed_deterministic() {
        let mut vm = host_vm(HOST);
        let src = r#"
            component Health { hp: 100 }
            set(get_entity("hero"), Health { hp: rand_int(1, 1000000) })
        "#;
        let caps_a = r#"{ "write": ["Health"], "seed": 7 }"#;
        let (ok1, f1) = sandbox(&mut vm, src, caps_a);
        let (ok2, f2) = sandbox(&mut vm, src, caps_a);
        assert!(ok1 && ok2);
        let v1 = peek_field(&mut vm, f1, "hero", "Health");
        let v2 = peek_field(&mut vm, f2, "hero", "Health");
        assert_eq!(v1, v2, "same seed must give identical guest runs");

        let (ok3, f3) = sandbox(&mut vm, src, r#"{ "write": ["Health"], "seed": 8 }"#);
        assert!(ok3);
        let v3 = peek_field(&mut vm, f3, "hero", "Health");
        assert_ne!(v1, v3, "different seeds should diverge");
    }
}

/// `simulate_par` determinism and isolation tests.
#[cfg(test)]
mod simulate_par_tests {
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::VM;

    fn run_host(src: &str) -> Vec<String> {
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
        vm.run(0).expect("run");
        vm.print_buffer.clone()
    }

    #[test]
    fn simulate_par_is_deterministic_per_seed_and_isolated() {
        let out = run_host(
            r#"
            component P { v: 0 }
            system Jitter(p: mut P) {
                p = P { v: p.v + rand_int(1, 1000000) }
            }
            let e = spawn("e", P { v: 0 })
            let f = fork()
            let runs1 = simulate_par(f, [system::Jitter], 5, 4, 42)
            let runs2 = simulate_par(f, [system::Jitter], 5, 4, 42)
            let mut i = 0
            while i < 4 {
                let a = peek(runs1[i], e, P) |> unwrap
                let b = peek(runs2[i], e, P) |> unwrap
                print(f"{a.v}|{b.v}")
                i = i + 1
            }
            let m = get(e, P) |> unwrap
            print(f"main={m.v}")
            "#,
        );
        assert_eq!(out.len(), 5);
        let mut fork_values = Vec::new();
        for line in &out[..4] {
            let (a, b) = line.split_once('|').expect("a|b");
            assert_eq!(a, b, "same (inputs, seed) must be bit-identical: {}", line);
            fork_values.push(a.to_string());
        }
        // Distinct fork indices get distinct derived seeds.
        let distinct: std::collections::HashSet<_> = fork_values.iter().collect();
        assert!(
            distinct.len() > 1,
            "fork seeds should diverge, got {:?}",
            fork_values
        );
        // The live world is never touched by speculation.
        assert_eq!(out[4], "main=0");
    }

    #[test]
    fn simulate_par_multi_tick_writes_accumulate() {
        // Regression guard for the is_worker trap: tick N+1 must observe tick
        // N's writes inside each fork (writes apply directly to the worker's
        // private world instead of being deferred to a command buffer).
        let out = run_host(
            r#"
            component N { v: 0 }
            system Inc(n: mut N) {
                n = N { v: n.v + 1 }
            }
            let e = spawn("e", N { v: 0 })
            let f = fork()
            let runs = simulate_par(f, [system::Inc], 7, 2, 1)
            let a = peek(runs[0], e, N) |> unwrap
            let b = peek(runs[1], e, N) |> unwrap
            print(f"{a.v},{b.v}")
            "#,
        );
        assert_eq!(out, vec!["7,7"]);
    }

    #[test]
    fn fork_with_seeds_a_resource_without_touching_live_world() {
        // dogfood feature seq 150: fork_with overrides a resource in a copy of
        // the fork; the live world's resource is unchanged (no commit()).
        let out = run_host(
            r#"
            resource Policy { rate: 1 }
            component Coin { n: 0 }
            let e = spawn("e", Coin { n: 0 })
            let root = fork()
            let seeded = fork_with(root, Policy { rate: 9 })
            let a = peek_resource(seeded, Policy) |> unwrap
            let b = res(Policy)
            print(f"seeded={a.rate} live={b.rate}")
            "#,
        );
        assert_eq!(out, vec!["seeded=9 live=1"]);
    }

    #[test]
    fn simulate_many_runs_distinct_candidate_forks_in_parallel() {
        // The heterogeneous axis: three candidates seeded to different policy
        // rates, each advanced 4 ticks under the same schedule, evaluated at
        // once. Each future reflects its own seed, and the live world stays 0.
        let out = run_host(
            r#"
            resource Policy { rate: 1 }
            component Coin { n: 0 }
            system Mint(c: mut Coin) { c = Coin { n: c.n + res(Policy).rate } }
            let e = spawn("e", Coin { n: 0 })
            let root = fork()
            let cands = [
                fork_with(root, Policy { rate: 1 }),
                fork_with(root, Policy { rate: 5 }),
                fork_with(root, Policy { rate: 10 }),
            ]
            let futs = simulate_many(cands, [system::Mint], 4, 42)
            let a = peek(futs[0], e, Coin) |> unwrap
            let b = peek(futs[1], e, Coin) |> unwrap
            let c = peek(futs[2], e, Coin) |> unwrap
            let live = get(e, Coin) |> unwrap
            print(f"{a.n},{b.n},{c.n},live={live.n}")
            "#,
        );
        // 4 ticks: rate 1 -> 4, rate 5 -> 20, rate 10 -> 40; live untouched.
        assert_eq!(out, vec!["4,20,40,live=0"]);
    }

    #[test]
    fn simulate_many_is_deterministic_regardless_of_order() {
        // Same list of seeded forks, same seed => bit-identical results,
        // mirroring simulate_par's per-index seeding guarantee.
        let src = r#"
            resource Policy { rate: 2 }
            component Coin { n: 0 }
            system Mint(c: mut Coin) { c = Coin { n: c.n + res(Policy).rate } }
            let e = spawn("e", Coin { n: 0 })
            let root = fork()
            let cands = [fork_with(root, Policy { rate: 3 }), fork_with(root, Policy { rate: 7 })]
            let futs = simulate_many(cands, [system::Mint], 5, 99)
            let a = peek(futs[0], e, Coin) |> unwrap
            let b = peek(futs[1], e, Coin) |> unwrap
            print(f"{a.n},{b.n}")
        "#;
        assert_eq!(run_host(src), run_host(src));
        assert_eq!(run_host(src), vec!["15,35"]);
    }

    #[test]
    fn simulate_par_zero_forks_gives_empty_list() {
        let out = run_host(
            r#"
            component N { v: 0 }
            system Inc(n: mut N) {
                n = N { v: n.v + 1 }
            }
            let e = spawn("e", N { v: 0 })
            let f = fork()
            let runs = simulate_par(f, [system::Inc], 1, 0, 1)
            print(len(runs))
            "#,
        );
        assert_eq!(out, vec!["0"]);
    }

    #[test]
    fn fork_seed_identifies_rollouts_and_simulate_seeded_reproduces_one() {
        // dogfood feature seq 150 follow-on: every simulate_par result knows
        // which effective rng seed produced it (fork_seed), and feeding that
        // seed to simulate_seeded re-runs exactly that rollout in isolation.
        let out = run_host(
            r#"
            component P { v: 0 }
            system Jitter(p: mut P) {
                p = P { v: p.v + rand_int(1, 1000000) }
            }
            let e = spawn("e", P { v: 0 })
            let f = fork()
            let runs = simulate_par(f, [system::Jitter], 3, 3, 42)
            print(f"{fork_seed(runs[0])}|{fork_seed(runs[1])}|{fork_seed(runs[2])}|{fork_seed(f)}")
            let repro = simulate_seeded(f, [system::Jitter], 3, fork_seed(runs[1]))
            let a = peek(runs[1], e, P) |> unwrap
            let b = peek(repro, e, P) |> unwrap
            let same = a.v == b.v
            print(f"reproduced={same}")
            print(f"repro_knows_seed={fork_seed(repro) == fork_seed(runs[1])}")
            "#,
        );
        assert_eq!(out.len(), 3);
        let seeds: Vec<i64> = out[0]
            .split('|')
            .map(|s| s.parse().expect("seed int"))
            .collect();
        // The three rollout seeds are nonzero and pairwise distinct; a plain
        // fork() has no rollout seed (0 is unambiguous — the SplitMix64
        // finalizer never derives 0).
        assert_ne!(seeds[0], 0);
        assert_ne!(seeds[1], 0);
        assert_ne!(seeds[2], 0);
        assert_ne!(seeds[0], seeds[1]);
        assert_ne!(seeds[1], seeds[2]);
        assert_eq!(seeds[3], 0, "plain fork() carries no rollout seed");
        assert_eq!(out[1], "reproduced=true");
        assert_eq!(out[2], "repro_knows_seed=true");
    }

    #[test]
    fn simulate_par_override_list_seeds_candidates_without_commit() {
        // dogfood feature seq 150 #2: the optional 6th argument overrides
        // resources on the base fork at the call site, so a pure search never
        // commit()s a candidate into the live world. The override also marks
        // derived copies as new candidates: fork_with on a rollout result
        // clears its rollout seed.
        let out = run_host(
            r#"
            resource Policy { rate: 1 }
            component Coin { n: 0 }
            system Mint(c: mut Coin) { c = Coin { n: c.n + res(Policy).rate } }
            let e = spawn("e", Coin { n: 0 })
            let f = fork()
            let runs = simulate_par(f, [system::Mint], 4, 2, 7, [Policy { rate: 5 }])
            let a = peek(runs[0], e, Coin) |> unwrap
            let b = peek(runs[1], e, Coin) |> unwrap
            let live = res(Policy)
            print(f"{a.n},{b.n},live={live.rate}")
            let derived = fork_with(runs[0], Policy { rate: 2 })
            print(fork_seed(derived))
            "#,
        );
        // rate 5 for 4 ticks in both rollouts; the live Policy still rate 1.
        assert_eq!(out, vec!["20,20,live=1", "0"]);
    }
}

/// Blast-radius assertion tests (List item #3): `diff` and
/// `assert_only_changed` — testing the negative space.
#[cfg(test)]
mod blast_radius_tests {
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::VM;

    fn run_host(src: &str) -> Result<Vec<String>, String> {
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
        vm.run(0)?;
        Ok(vm.print_buffer.clone())
    }

    const WORLD: &str = r#"
        component Health { hp: 100 }
        component Gold { amount: 1000 }
        component Position { x: 0 }
        system Damage(h: mut Health) {
            h = Health { hp: h.hp - 10 }
        }
        let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 1000 }, Position { x: 0 })
    "#;

    #[test]
    fn diff_reports_only_touched_components() {
        let out = run_host(&format!(
            r#"{WORLD}
            let before = fork()
            let after = simulate(before, [system::Damage], 3)
            let d = diff(before, after)
            print(d["Health"] |> unwrap_or(0))
            print(contains(keys(d), "Gold"))
            print(contains(keys(d), "Position"))
            "#
        ))
        .expect("run");
        assert_eq!(out, vec!["1", "false", "false"]);
    }

    #[test]
    fn diff_of_identical_forks_is_empty() {
        let out = run_host(&format!(
            r#"{WORLD}
            let a = fork()
            let b = fork()
            print(len(diff(a, b)))
            "#
        ))
        .expect("run");
        assert_eq!(out, vec!["0"]);
    }

    #[test]
    fn assert_only_changed_passes_when_within_radius() {
        let out = run_host(&format!(
            r#"{WORLD}
            let before = fork()
            let after = simulate(before, [system::Damage], 1)
            assert_only_changed(before, after, [Health])
            print("ok")
            "#
        ))
        .expect("run");
        assert_eq!(out, vec!["ok"]);
    }

    #[test]
    fn assert_only_changed_fails_outside_radius() {
        // The Damage system writes Health, but the assertion only allows Gold.
        let err = run_host(&format!(
            r#"{WORLD}
            let before = fork()
            let after = simulate(before, [system::Damage], 1)
            assert_only_changed(before, after, [Gold])
            print("unreachable")
            "#
        ))
        .expect_err("assertion must fail");
        assert!(err.contains("unexpected changes"), "got: {}", err);
        assert!(err.contains("Health"), "got: {}", err);
        assert!(err.contains("allowed: [Gold]"), "got: {}", err);
    }

    #[test]
    fn assert_only_changed_event_flow_from_council_sketch() {
        // The ratified one-liner: emit an event, flush, prove the blast
        // radius. String names are accepted alongside component type refs.
        let out = run_host(&format!(
            r#"{WORLD}
            event Hit {{ amount }}
            on Hit(e) {{
                let h = get(hero, Health) |> unwrap
                set(hero, Health {{ hp: h.hp - e.amount }})
            }}
            let before = fork()
            emit Hit {{ amount: 25 }}
            flush_events()
            assert_only_changed(before, fork(), ["Health"])
            let h = get(hero, Health) |> unwrap
            print(h.hp)
            "#
        ))
        .expect("run");
        assert_eq!(out, vec!["75"]);
    }

    #[test]
    fn diff_counts_spawned_and_despawned_rows() {
        let out = run_host(&format!(
            r#"{WORLD}
            let before = fork()
            spawn("goblin", Health {{ hp: 30 }})
            let after = fork()
            let d = diff(before, after)
            print(d["Health"] |> unwrap_or(0))
            "#
        ))
        .expect("run");
        // The goblin lands in a new archetype: 1 new Health row.
        assert_eq!(out, vec!["1"]);
    }

    #[test]
    fn diff_sees_resource_changes() {
        let out = run_host(
            r#"
            resource Score { total: 0 }
            fn main() -> nil {
                let before = fork()
                set_resource(Score, Score { total: 99 })
                let after = fork()
                let d = diff(before, after)
                print(d["Score"] |> unwrap_or(0))
                assert_only_changed(before, after, [Score])
                print("ok")
            }
            "#,
        )
        .expect("run");
        assert_eq!(out, vec!["1", "ok"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_seeds_are_distinct_and_nonzero() {
        let base = 12345u64;
        let a = fork_seed(base, 0);
        let b = fork_seed(base, 1);
        let c = fork_seed(base, 2);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn fork_seed_is_deterministic() {
        assert_eq!(fork_seed(99, 3), fork_seed(99, 3));
    }

    #[test]
    fn caps_write_acl() {
        let mut set = HashSet::new();
        set.insert("Health".to_string());
        let caps = SandboxCaps::new(set, 1000, 1 << 20);
        assert!(caps.may_write("Health"));
        assert!(!caps.may_write("Gold"));
    }

    #[test]
    fn caps_read_defaults_to_wildcard_when_key_absent() {
        // No "read" key => read everything (backward compatible).
        let (caps, _) = SandboxCaps::from_json(r#"{ "write": ["Health"] }"#).unwrap();
        assert!(caps.may_read("Health"));
        assert!(caps.may_read("AnythingElse"));
        assert!(caps.may_read_all());
    }

    #[test]
    fn caps_read_allowlist_is_exact_and_denies_bulk() {
        // An explicit "read" list is an allowlist; it does not include the
        // wildcard, so bulk readers are denied.
        let (caps, _) = SandboxCaps::from_json(r#"{ "write": [], "read": ["Health"] }"#).unwrap();
        assert!(caps.may_read("Health"));
        assert!(!caps.may_read("Vault"));
        assert!(!caps.may_read_all());
    }

    #[test]
    fn caps_empty_read_list_reads_nothing() {
        // Present-but-empty means "read nothing", symmetric with write: [].
        let (caps, _) = SandboxCaps::from_json(r#"{ "read": [] }"#).unwrap();
        assert!(!caps.may_read("Health"));
        assert!(!caps.may_read_all());
    }

    #[test]
    fn caps_read_wildcard_grants_all() {
        let (caps, _) = SandboxCaps::from_json(r#"{ "read": ["*"] }"#).unwrap();
        assert!(caps.may_read("Health"));
        assert!(caps.may_read_all());
    }

    #[test]
    fn caps_read_must_be_array() {
        let err = SandboxCaps::from_json(r#"{ "read": "Health" }"#).unwrap_err();
        assert!(err.contains("'read' must be an array"), "got: {}", err);
    }
}
