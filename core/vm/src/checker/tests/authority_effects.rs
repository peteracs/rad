    fn authority_src(src: &str) -> (Vec<TypeError>, crate::types::AuthorityReport) {
        use crate::lexer::Lexer;
        use crate::parser::Parser;

        let tokens = Lexer::new(src).tokenize().0;
        let program = Parser::new(tokens).parse();
        let mut checker = Checker::new();
        let errors = checker.check(&program);
        (errors, checker.output().authority)
    }

    #[test]
    fn system_authority_follows_helper_writes_and_reports_the_path() {
        let (errors, report) = authority_src(
            r#"
            component LiveMembership {}
            component WireIdentity {}

            fn rewrite_wire(target: entity) -> nil {
                set(target, WireIdentity {})
            }

            system RemoveEntity(live: mut LiveMembership) {
                rewrite_wire(self)
            }
            "#,
        );
        let violation = errors
            .iter()
            .find(|error| error.message.contains("exceeds its declared authority"))
            .unwrap_or_else(|| {
                panic!(
                    "hidden helper write must be rejected; errors={errors:?}; callables={:?}",
                    report.callables
                )
            });
        assert!(violation.message.contains("writes [WireIdentity]"));
        assert!(
            violation
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("RemoveEntity -> rewrite_wire")),
            "diagnostic must carry a machine-backed path: {violation:?}"
        );
        assert_eq!(
            report.resolve("RemoveEntity").unwrap().transitive.writes,
            vec!["WireIdentity"]
        );
    }

    #[test]
    fn matching_mut_system_authority_accepts_transitive_helper_write() {
        let (errors, _) = authority_src(
            r#"
            component LiveMembership {}
            component WireIdentity {}

            fn rewrite_wire(target: entity) -> nil {
                set(target, WireIdentity {})
            }

            system RemoveEntity(live: mut LiveMembership, wire: mut WireIdentity) {
                rewrite_wire(self)
            }
            "#,
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "declared mut authority should cover helper writes: {errors:?}"
        );
    }

    #[test]
    fn authority_only_signature_entries_do_not_change_the_system_query() {
        let (errors, report) = authority_src(
            r#"
            component Root {}
            component Other {}

            fn rewrite(target: entity) -> nil {
                let _ = require(target, Other)
                set(target, Other {})
            }

            system Repair(root: Root, reads Other, writes Other) {
                rewrite(self)
            }
            "#,
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "explicit cross-entity authority must satisfy the sandbox: {errors:?}"
        );
        let repair = report.resolve("Repair").unwrap();
        assert_eq!(repair.direct.reads, vec!["Root"]);
        assert_eq!(repair.transitive.reads, vec!["Other", "Root"]);
        assert_eq!(repair.transitive.writes, vec!["Other"]);
    }

    #[test]
    fn wildcard_authority_is_explicit_and_unknown_names_are_rejected() {
        let (accepted, _) = authority_src(
            r#"
            component Lifetime {}
            system Cleanup(lifetime: Lifetime, writes *) { despawn(self) }
            "#,
        );
        assert!(
            !accepted
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "writes * must explicitly cover structural world writes: {accepted:?}"
        );

        let (rejected, _) = authority_src(
            r#"
            component Lifetime {}
            system Typo(lifetime: Lifetime, reads MissingAuthority) {}
            "#,
        );
        assert!(rejected.iter().any(|error| {
            error
                .message
                .contains("authority over unknown component/resource 'MissingAuthority'")
        }));
    }

    #[test]
    fn handler_chain_is_part_of_transitive_effects_and_reverse_indexes() {
        let (errors, report) = authority_src(
            r#"
            component LiveMembership {}
            component GraveyardMembership {}
            event EntityRetired { target: entity }

            on EntityRetired(retired) {
                set(retired.target, GraveyardMembership {})
            }

            system Retire(live: mut LiveMembership, emits EntityRetired) {
                emit EntityRetired { target: self }
            }
            "#,
        );
        let retire = report.resolve("Retire").unwrap();
        assert_eq!(retire.direct.emits, vec!["EntityRetired"]);
        assert_eq!(retire.transitive.writes, vec!["GraveyardMembership"]);
        assert!(!retire.deferred_calls.is_empty());
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "a queued handler is its own authority boundary: {errors:?}"
        );
        assert!(report.writers_of("GraveyardMembership").contains(&"Retire".to_string()));
        let handler = report
            .callables
            .values()
            .find(|item| item.kind == crate::types::AuthorityCallableKind::Handler)
            .unwrap();
        let path = report.path("Retire", &handler.display_name).unwrap().unwrap();
        assert_eq!(path, vec!["Retire", handler.display_name.as_str()]);
    }

    #[test]
    fn async_declarations_are_marked_even_without_await_in_the_body() {
        let (_, report) = authority_src(
            r#"
            event Ping {}
            async fn background() -> nil {}
            async on Ping(received) {}
            "#,
        );
        assert!(report.resolve("background").unwrap().direct.async_effect);
        let handler = report
            .callables
            .values()
            .find(|item| item.kind == crate::types::AuthorityCallableKind::Handler)
            .unwrap();
        assert!(handler.direct.async_effect);
    }

    #[test]
    fn bounded_callback_dispatch_propagates_closure_effects() {
        let (_, report) = authority_src(
            r#"
            component WireIdentity {}

            fn invoke(callback: fn(entity) -> nil, target: entity) -> nil {
                callback(target)
            }

            fn owner(target: entity) -> nil {
                invoke(fn(target: entity) -> nil {
                    set(target, WireIdentity {})
                }, target)
            }
            "#,
        );
        let owner = report.resolve("owner").unwrap();
        assert_eq!(owner.transitive.writes, vec!["WireIdentity"]);
        assert!(
            report
                .callables
                .values()
                .any(|item| item.kind == crate::types::AuthorityCallableKind::Closure
                    && item.direct.writes == vec!["WireIdentity"]),
            "inline closure must have its own inferred effect set"
        );
    }

    #[test]
    fn builtin_callback_metadata_propagates_function_parameter_effects() {
        let (_, report) = authority_src(
            r#"
            component WireIdentity {}

            fn invoke_all(callback: fn(entity) -> entity, target: entity) -> list<entity> {
                return map([target], callback)
            }

            fn owner(target: entity) -> nil {
                let _ = invoke_all(fn(item: entity) -> entity {
                    set(item, WireIdentity {})
                    return item
                }, target)
            }
            "#,
        );
        assert_eq!(
            report.resolve("owner").unwrap().transitive.writes,
            vec!["WireIdentity"]
        );
    }

    #[test]
    fn reassigned_function_value_keeps_every_statically_bounded_target() {
        let (errors, report) = authority_src(
            r#"
            component Marker {}
            component FirstWrite {}
            component SecondWrite {}

            system Dispatch(marker: Marker, writes FirstWrite, writes SecondWrite) {
                let mut callback = fn(target: entity) -> nil {
                    set(target, FirstWrite {})
                }
                if true {
                    callback = fn(target: entity) -> nil {
                        set(target, SecondWrite {})
                    }
                }
                callback(self)
            }
            "#,
        );
        assert_eq!(
            report.resolve("Dispatch").unwrap().transitive.writes,
            vec!["FirstWrite", "SecondWrite"]
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "all statically bounded targets were declared: {errors:?}"
        );
    }

    #[test]
    fn forwarded_callback_parameter_resolves_to_the_concrete_closure() {
        let (errors, report) = authority_src(
            r#"
            component Marker {}
            component WireIdentity {}

            fn invoke(callback: fn(entity) -> nil, target: entity) -> nil {
                callback(target)
            }
            fn relay(callback: fn(entity) -> nil, target: entity) -> nil {
                invoke(callback, target)
            }

            system Owner(marker: Marker, writes WireIdentity) {
                relay(fn(target: entity) -> nil {
                    set(target, WireIdentity {})
                }, self)
            }
            "#,
        );
        let owner = report.resolve("Owner").unwrap();
        assert_eq!(owner.transitive.writes, vec!["WireIdentity"]);
        assert!(!owner.transitive.unknown);
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "the concrete closure must flow through both callback parameters: {errors:?}"
        );
    }

    #[test]
    fn system_effects_are_specialized_for_each_callback_call_site() {
        let (errors, report) = authority_src(
            r#"
            component Root {}
            component X {}
            component Y {}

            fn invoke(callback: fn(entity) -> nil, target: entity) {
                callback(target)
            }
            fn write_x(target: entity) { set(target, X {}) }
            fn write_y(target: entity) { set(target, Y {}) }

            system SystemX(root: Root, writes X) { invoke(write_x, self) }
            system SystemY(root: Root, writes Y) { invoke(write_y, self) }
            "#,
        );
        assert_eq!(
            report.resolve("SystemX").unwrap().synchronous.writes,
            vec!["X"]
        );
        assert_eq!(
            report.resolve("SystemY").unwrap().synchronous.writes,
            vec!["Y"]
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "callers must not inherit each other's callback effects: {errors:?}"
        );
    }

    #[test]
    fn undeclared_helper_emission_is_rejected_transitively() {
        let (errors, _) = authority_src(
            r#"
            component Root {}
            event Retired {}
            fn publish() { emit Retired {} }
            system Run(root: Root) { publish() }
            "#,
        );
        assert!(errors.iter().any(|error| {
            error.message.contains("System 'Run' exceeds its declared authority")
                && error.message.contains("emits [Retired]")
                && error.hint.as_deref().is_some_and(|hint| {
                    hint.contains("Run -> publish") && hint.contains("emits Retired")
                })
        }));
    }

    #[test]
    fn undeclared_helper_async_reachability_is_rejected_transitively() {
        let (errors, _) = authority_src(
            r#"
            component Root {}
            async fn background() -> nil {}
            fn helper() { let _task = background() }
            system Run(root: Root) { helper() }
            "#,
        );
        assert!(errors.iter().any(|error| {
            error.message.contains("System 'Run' exceeds its declared authority")
                && error.message.contains("reaches async execution")
                && error.hint.as_deref().is_some_and(|hint| {
                    hint.contains("Run -> helper -> background")
                        && hint.contains("async true")
                })
        }));
    }

    #[test]
    fn callback_emission_io_and_async_reachability_require_all_grants() {
        let source = |grants: &str| {
            format!(
                r#"
                component Root {{}}
                event CallbackEvent {{}}
                async fn background() -> nil {{}}
                fn invoke(callback: fn(entity) -> nil, target: entity) {{ callback(target) }}
                system Run(root: Root{grants}) {{
                    invoke(fn(target: entity) -> nil {{
                        emit CallbackEvent {{}}
                        print("callback")
                        let _task = background()
                    }}, self)
                }}
                "#
            )
        };
        let (errors, _) = authority_src(&source(""));
        let violation = errors
            .iter()
            .find(|error| error.message.contains("System 'Run' exceeds its declared authority"))
            .expect("callback effects must exceed an empty grant");
        assert!(violation.message.contains("emits [CallbackEvent]"));
        assert!(violation.message.contains("performs IO"));
        assert!(violation.message.contains("reaches async execution"));

        let (granted_errors, report) = authority_src(&source(
            ", emits CallbackEvent, io true, async true",
        ));
        assert!(
            !granted_errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "complete callback grant should compile: {granted_errors:?}"
        );
        let run = report.resolve("Run").unwrap();
        assert_eq!(run.synchronous.emits, vec!["CallbackEvent"]);
        assert!(run.synchronous.io);
        assert!(run.synchronous.async_effect);
    }

    #[test]
    fn closure_captures_local_callable_binding() {
        let (errors, report) = authority_src(
            r#"
            component Marker {}
            component WireIdentity {}

            fn rewrite(target: entity) -> nil {
                set(target, WireIdentity {})
            }

            system Owner(marker: Marker, writes WireIdentity) {
                let local_rewrite = rewrite
                let callback = fn(target: entity) -> nil {
                    local_rewrite(target)
                }
                callback(self)
            }
            "#,
        );
        assert_eq!(
            report.resolve("Owner").unwrap().transitive.writes,
            vec!["WireIdentity"]
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "captured callable aliases must remain statically bounded: {errors:?}"
        );
    }

    #[test]
    fn let_else_pattern_alias_shadows_a_system_parameter() {
        let (_, report) = authority_src(
            r#"
            component WireIdentity { value: int = 0 }

            system Observe(wire: WireIdentity) {
                let Some { value: wire } = nil else { return }
                wire.value = 1
            }
            "#,
        );

        let observe = report.resolve("Observe").expect("Observe authority");
        assert_eq!(observe.direct.reads, vec!["WireIdentity"]);
        assert!(
            observe.direct.writes.is_empty(),
            "the let-else alias is local and must not become a component write: {observe:?}"
        );
    }

    #[test]
    fn pipeline_implicit_argument_keeps_callback_slot_statically_bounded() {
        let (errors, report) = authority_src(
            r#"
            component Visible {}

            system Observe(visible: Visible) {
                let _ = [self] |> any(fn(candidate: entity) -> bool {
                    return has(candidate, Visible)
                })
            }
            "#,
        );
        let observe = report.resolve("Observe").unwrap();
        assert!(!observe.transitive.unknown, "report={report:?}");
        assert_eq!(observe.transitive.reads, vec!["Visible"]);
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "pipeline callback must be resolved at its logical argument index: {errors:?}"
        );
    }

    #[test]
    fn typed_pure_function_value_is_an_enforced_zero_effect_boundary() {
        let (errors, report) = authority_src(
            r#"
            component Marker {}
            let MODULE = load_extension("missing-test-module")
            let EXTERNAL: pure fn(int) -> int = MODULE["external"]

            system UseExternal(marker: Marker) {
                let _ = EXTERNAL(1)
            }
            "#,
        );
        let external = report.resolve("EXTERNAL").unwrap();
        assert!(!external.transitive.unknown, "report={report:?}");
        assert!(external.transitive.reads.is_empty());
        assert!(external.transitive.writes.is_empty());
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "the ordinary type checker enforces the pure fn annotation: {errors:?}"
        );
    }

    #[test]
    fn unbounded_function_value_fails_closed_inside_a_system() {
        let (errors, report) = authority_src(
            r#"
            component Marker {}
            let MODULE = load_extension("missing-test-module")
            let EXTERNAL: fn(int) -> int = MODULE["external"]

            fn invoke_external() -> int {
                return EXTERNAL(1)
            }

            system UseExternal(marker: Marker) {
                let _ = invoke_external()
            }
            "#,
        );
        assert!(report.resolve("UseExternal").unwrap().transitive.unknown);
        let violation = errors
            .iter()
            .find(|error| {
                error
                    .message
                    .contains("System 'UseExternal' exceeds its declared authority")
                    && error.message.contains("invokes an unbounded function value")
            })
            .expect("unbounded dispatch must fail closed");
        assert!(violation.hint.as_deref().is_some_and(|hint| {
            hint.contains("UseExternal -> invoke_external -> EXTERNAL")
        }));
    }

    #[test]
    fn resource_access_and_transitions_have_exact_effects() {
        let (errors, report) = authority_src(
            r#"
            component Marker {}
            resource Audit { count: int = 0 }
            state Door {
                Closed { on open -> Open }
                Open { on close -> Closed }
            }

            fn advance(current: Door) -> Door {
                return transition(current, "open") |> unwrap
            }

            fn record() -> nil {
                set_resource(Audit, Audit { count: res(Audit).count + 1 })
            }

            system Tick(marker: Marker) {
                let _ = advance(Door::Closed)
                record()
            }
            "#,
        );
        let tick = report.resolve("Tick").unwrap();
        assert_eq!(tick.transitive.reads, vec!["Audit", "Marker"]);
        assert_eq!(tick.transitive.writes, vec!["Audit"]);
        assert_eq!(tick.transitive.emits, vec!["$transition"]);
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("System 'Tick' exceeds its declared authority")
                && error.message.contains("reads [Audit]")
                && error.message.contains("writes [Audit]")
                && error.message.contains("emits [$transition]")
        }));
    }

    #[test]
    fn builtin_component_positions_match_runtime_signatures() {
        let (_, report) = authority_src(
            r#"
            component WireIdentity {}

            fn explain(target: entity) -> str {
                return why(target, WireIdentity)
            }

            fn explain_fork(snapshot: world_fork, target: entity) -> any {
                return peek(snapshot, target, WireIdentity)
            }
            "#,
        );
        assert_eq!(
            report.resolve("explain").unwrap().transitive.reads,
            vec!["WireIdentity"]
        );
        assert_eq!(
            report.resolve("explain_fork").unwrap().transitive.reads,
            vec!["WireIdentity"]
        );
    }

    #[test]
    fn simulation_schedule_is_statically_bounded_in_the_graph() {
        let (_, report) = authority_src(
            r#"
            component Score { value: int = 0 }
            let ROLLOUT = [system::Tick]

            system Tick(score: mut Score) {
                score.value = score.value + 1
            }

            fn rollout() -> world_fork {
                return simulate(fork(), ROLLOUT, 1)
            }
            "#,
        );
        let rollout = report.resolve("rollout").unwrap();
        assert!(rollout.transitive.reads.contains(&"Score".to_string()));
        assert!(rollout.transitive.writes.contains(&"Score".to_string()));
        assert_eq!(
            report.path("rollout", "Tick").unwrap().unwrap(),
            vec!["rollout", "Tick"]
        );
    }

    #[test]
    fn named_phase_schedule_expands_to_deferred_system_edges() {
        let (_, report) = authority_src(
            r#"
            component Score { value: int = 0 }

            system Tick(score: mut Score) {
                score.value = score.value + 1
            }
            phase Frame [Tick]

            fn mission_frame() -> nil {
                schedule [Frame]
            }
            "#,
        );
        let frame = report.resolve("mission_frame").unwrap();
        assert!(!frame.synchronous.writes.contains(&"Score".to_string()));
        assert!(frame.transitive.writes.contains(&"Score".to_string()));
        assert!(!frame.transitive.unknown);
        assert_eq!(
            report.path("mission_frame", "Tick").unwrap().unwrap(),
            vec!["mission_frame", "Tick"]
        );
    }

    #[test]
    fn flush_events_reaches_every_statically_installed_handler() {
        let (errors, report) = authority_src(
            r#"
            component FirstWrite {}
            component SecondWrite {}
            event Ping { target: entity }
            event Pong { target: entity }

            on Ping(received) {
                set(received.target, FirstWrite {})
            }
            on Pong(received) {
                set(received.target, SecondWrite {})
            }

            fn flush() -> nil {
                flush_events()
            }
            "#,
        );
        assert_eq!(
            report.resolve("flush").unwrap().transitive.writes,
            vec!["FirstWrite", "SecondWrite"],
            "errors={errors:?}; report={report:?}"
        );
    }

    #[test]
    fn explicit_flush_keeps_handlers_inside_the_system_sandbox() {
        let (errors, _) = authority_src(
            r#"
            component Trigger {}
            component HiddenWrite {}
            event Ping { target: entity }

            on Ping(received) {
                set(received.target, HiddenWrite {})
            }

            system FlushNow(trigger: Trigger) {
                flush_events()
            }
            "#,
        );
        assert!(errors.iter().any(|error| {
            error.message.contains("System 'FlushNow' exceeds its declared authority")
                && error.message.contains("writes [HiddenWrite]")
        }));
    }

    #[test]
    fn flush_event_handlers_require_synchronous_io_and_async_grants() {
        let (errors, report) = authority_src(
            r#"
            component Trigger {}
            event Ping {}
            async on Ping(received) { print("handled") }

            system FlushNow(trigger: Trigger, emits "$events") {
                flush_events()
            }
            "#,
        );
        let flush = report.resolve("FlushNow").unwrap();
        assert!(flush.synchronous.io);
        assert!(flush.synchronous.async_effect);
        let violation = errors
            .iter()
            .find(|error| {
                error
                    .message
                    .contains("System 'FlushNow' exceeds its declared authority")
            })
            .expect("flushed handler effects must stay inside the system sandbox");
        assert!(violation.message.contains("performs IO"));
        assert!(violation.message.contains("reaches async execution"));
        assert!(!violation.message.contains("emits [$events]"));
    }

    #[test]
    fn entity_name_and_identity_indexes_have_narrow_explicit_authority() {
        let (errors, report) = authority_src(
            r#"
            component Root {}
            component Named {}

            system Lookup(root: Root, reads "$entity_names") {
                let _ = require_entity("boss")
            }
            system Reverse(root: Root, reads "$entity_identity") {
                let _ = name_of(self)
            }
            system Publish(
                root: Root,
                writes Named,
                writes "$entity_names",
                writes "$entity_identity"
            ) {
                let _ = spawn("boss", Named {})
            }
            "#,
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("exceeds its declared authority")),
            "narrow synthetic grants must cover indexed operations: {errors:?}"
        );
        assert_eq!(
            report.resolve("Lookup").unwrap().synchronous.reads,
            vec!["$entity_names", "Root"]
        );
        assert_eq!(
            report.resolve("Reverse").unwrap().synchronous.reads,
            vec!["$entity_identity", "Root"]
        );
        assert_eq!(
            report.resolve("Publish").unwrap().synchronous.writes,
            vec!["$entity_identity", "$entity_names", "Named"]
        );
    }

    #[test]
    fn top_level_closure_binding_is_a_transitive_callable() {
        let (_, report) = authority_src(
            r#"
            component WireIdentity {}
            let rewrite = fn(target: entity) -> nil {
                set(target, WireIdentity {})
            }

            fn owner(target: entity) -> nil { rewrite(target) }
            "#,
        );
        let rewrite = report.resolve("rewrite").unwrap();
        assert_eq!(rewrite.kind, crate::types::AuthorityCallableKind::Closure);
        assert_eq!(rewrite.direct.writes, vec!["WireIdentity"]);
        assert_eq!(
            report.resolve("owner").unwrap().transitive.writes,
            vec!["WireIdentity"]
        );
    }

    #[test]
    fn broad_world_access_is_included_in_named_reverse_queries() {
        let (_, report) = authority_src(
            r#"
            component WireIdentity {}
            fn restore(encoded: str) -> nil { let _ = load_world(encoded) }
            "#,
        );
        assert!(report
            .writers_of("WireIdentity")
            .contains(&"restore".to_string()));
    }
