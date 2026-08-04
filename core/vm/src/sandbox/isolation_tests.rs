

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