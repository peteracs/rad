    fn transaction_vm(source: &str) -> (VM, Result<(), String>) {
        let tokens = Lexer::new(source).tokenize().0;
        let program = Parser::new(tokens).parse();
        let result = Compiler::new().compile(&program).unwrap();
        let mut vm = VM::new();
        vm.load_compile_result(result);
        let outcome = vm.run(0);
        (vm, outcome)
    }

    fn transaction_component_int(vm: &VM, entity: u32, component: &str) -> i64 {
        vm.world
            .get_component(entity, component)
            .unwrap_or_else(|| panic!("entity {entity} is missing {component}"))
            .values[0]
            .as_int()
            .expect("test component field is int")
    }

    #[test]
    fn transaction_commits_atomically_then_runs_post_commit() {
        let output = run_source(
            r#"
            component Count { value: 0 }

            transaction Increment(target: entity) {
                requires has(target, Count)
                changes_only [Count]
                set(target, Count { value: 2 })
                ensures (get(target, Count) |> unwrap).value == 2
                post_commit { print("committed") }
            }

            let item = spawn(Count { value: 1 })
            Increment(item)
            print((get(item, Count) |> unwrap).value)
            print(why(item, Count))
            "#,
        );
        assert_eq!(&output[..2], ["committed", "2"]);
        assert!(output[2].contains("transaction Increment"), "{}", output[2]);
    }

    #[test]
    fn transaction_stages_state_machine_transition_without_emitting() {
        let output = run_source(
            r#"
            state Flow {
                Ready { on start -> Running }
                Running {}
            }
            component Job { flow: Flow::Ready }

            transaction Start(target: entity) {
                requires has(target, Job)
                changes_only [Job]
                let job = require(target, Job)
                set(target, Job {
                    flow: transition(job.flow, "start") |> unwrap,
                })
                ensures require(target, Job).flow == Flow::Running
            }

            let job = spawn(Job {})
            Start(job)
            print(require(job, Job).flow)
            "#,
        );
        assert_eq!(output, ["Flow::Running"]);
    }

    #[test]
    fn failed_precondition_restores_the_exact_base_world() {
        let (vm, outcome) = transaction_vm(
            r#"
            component Count { value: 0 }
            transaction Invalid(target: entity) {
                requires false
                changes_only [Count]
                set(target, Count { value: 9 })
                ensures true
            }
            let item = spawn(Count { value: 1 })
            Invalid(item)
            "#,
        );
        let error = outcome.expect_err("precondition must reject");
        assert!(error.contains("failed precondition requires #1"), "{error}");
        assert_eq!(transaction_component_int(&vm, 0, "Count"), 1);
        assert!(vm.transaction.is_none());
    }

    #[test]
    fn failed_postcondition_rolls_back_every_staged_write() {
        let (vm, outcome) = transaction_vm(
            r#"
            component Left { value: 0 }
            component Right { value: 0 }
            transaction Invalid(target: entity) {
                requires true
                changes_only [Left, Right]
                set(target, Left { value: 10 })
                set(target, Right { value: 20 })
                ensures false
            }
            let item = spawn(Left { value: 1 }, Right { value: 2 })
            Invalid(item)
            "#,
        );
        assert!(outcome.unwrap_err().contains("failed postcondition ensures #1"));
        assert_eq!(transaction_component_int(&vm, 0, "Left"), 1);
        assert_eq!(transaction_component_int(&vm, 0, "Right"), 2);
        assert!(vm.transaction.is_none());
    }

    #[test]
    fn failed_postcondition_reverses_repeated_component_and_resource_writes() {
        let (vm, outcome) = transaction_vm(
            r#"
            component Left { value: 0 }
            component Right { value: 0 }
            resource Counter { value: 3 }
            transaction Invalid(target: entity) {
                requires true
                changes_only [Left, Right, Counter]
                set(target, Left { value: 10 })
                set(target, Left { value: 11 })
                remove(target, Right)
                set_resource(Counter, Counter { value: 20 })
                set_resource(Counter, Counter { value: 21 })
                ensures false
            }
            let item = spawn(Left { value: 1 }, Right { value: 2 })
            Invalid(item)
            "#,
        );
        assert!(outcome.unwrap_err().contains("failed postcondition ensures #1"));
        assert_eq!(transaction_component_int(&vm, 0, "Left"), 1);
        assert_eq!(transaction_component_int(&vm, 0, "Right"), 2);
        assert_eq!(
            vm.world
                .get_resource("Counter")
                .expect("Counter resource")
                .values[0]
                .as_int(),
            Some(3),
        );
    }

    #[test]
    fn failed_postcondition_restores_despawn_spawn_name_and_allocator_identity() {
        let (mut vm, outcome) = transaction_vm(
            r#"
            component Marker { value: 0 }
            transaction Invalid(target: entity) {
                requires has(target, Marker)
                changes_only [*]
                despawn(target)
                let _replacement = spawn("anchor", Marker { value: 9 })
                ensures false
            }
            let item = spawn("anchor", Marker { value: 1 })
            Invalid(item)
            "#,
        );
        assert!(outcome.unwrap_err().contains("failed postcondition ensures #1"));
        assert_eq!(vm.world.all_entity_ids(), vec![0]);
        assert_eq!(vm.world.entity_name(0).as_deref(), Some("anchor"));
        assert_eq!(vm.world.entity_ref(0).expect("restored entity").generation, 0);
        assert_eq!(transaction_component_int(&vm, 0, "Marker"), 1);
        assert_eq!(vm.world.spawn_entity(None), Ok(1));
    }

    #[test]
    fn runtime_changes_only_rejects_tampered_but_structurally_valid_bytecode() {
        let source =
            r#"
            component Allowed { value: 0 }
            component Hidden { value: 0 }
            transaction Invalid(target: entity) {
                requires true
                changes_only [Allowed, Hidden]
                set(target, Allowed { value: 10 })
                set(target, Hidden { value: 20 })
                ensures true
            }
            let item = spawn(Allowed { value: 1 }, Hidden { value: 2 })
            Invalid(item)
            "#;
        let tokens = Lexer::new(source).tokenize().0;
        let program = Parser::new(tokens).parse();
        let mut result = Compiler::new().compile(&program).unwrap();
        let transaction_chunk = result
            .chunks
            .iter_mut()
            .find(|chunk| chunk.name == "Invalid")
            .expect("transaction function chunk");
        let allowed_index = transaction_chunk
            .constants
            .iter()
            .position(|constant| constant.as_str() == Some("Allowed"))
            .expect("Allowed authority constant") as u16;
        let begin = transaction_chunk
            .code
            .iter()
            .position(|byte| *byte == crate::opcode::Op::BeginTransaction as u8)
            .expect("BeginTransaction opcode");
        let count = u16::from_be_bytes([
            transaction_chunk.code[begin + 3],
            transaction_chunk.code[begin + 4],
        ]);
        assert_eq!(count, 2);
        let second_authority = begin + 7;
        transaction_chunk.code[second_authority..second_authority + 2]
            .copy_from_slice(&allowed_index.to_be_bytes());

        let mut vm = VM::new();
        vm.load_compile_result(result);
        let outcome = vm.run(0);
        assert!(outcome.unwrap_err().contains("outside changes_only"));
        assert_eq!(transaction_component_int(&vm, 0, "Allowed"), 1);
        assert_eq!(transaction_component_int(&vm, 0, "Hidden"), 2);
    }

    #[test]
    fn post_commit_cannot_extend_the_committed_state_patch() {
        let source =
            r#"
            component Count { value: 0 }
            fn no_effect(target: entity) -> nil {}
            fn hidden_write(target: entity) -> nil {
                set(target, Count { value: 3 })
            }
            transaction CommitThenFail(target: entity) {
                requires true
                changes_only [Count]
                set(target, Count { value: 2 })
                ensures true
                post_commit { no_effect(target) }
            }
            let item = spawn(Count { value: 1 })
            CommitThenFail(item)
            "#;
        let tokens = Lexer::new(source).tokenize().0;
        let program = Parser::new(tokens).parse();
        let mut result = Compiler::new().compile(&program).unwrap();
        let harmless = result
            .global_names
            .iter()
            .position(|name| name == "no_effect")
            .unwrap() as u16;
        let writer = result
            .global_names
            .iter()
            .position(|name| name == "hidden_write")
            .unwrap() as u16;
        let chunk = result
            .chunks
            .iter_mut()
            .find(|chunk| chunk.name == "CommitThenFail")
            .unwrap();
        let harmless_bytes = harmless.to_be_bytes();
        let writer_bytes = writer.to_be_bytes();
        let callsite = (0..chunk.code.len().saturating_sub(2))
            .find(|offset| {
                chunk.code[*offset] == crate::opcode::Op::GetGlobal as u8
                    && chunk.code[*offset + 1..*offset + 3] == harmless_bytes
            })
            .expect("post_commit helper callsite");
        chunk.code[callsite + 1..callsite + 3].copy_from_slice(&writer_bytes);
        let mut vm = VM::new();
        vm.load_compile_result(result);
        let outcome = vm.run(0);
        let error = outcome.expect_err("post_commit state write must be rejected");
        assert!(error.contains("post_commit builtin 'set'"), "{error}");
        assert_eq!(transaction_component_int(&vm, 0, "Count"), 2);
        assert!(vm.post_commit.is_none());
    }

    #[test]
    fn transaction_removes_components_and_updates_resources_atomically() {
        let output = run_source(
            r#"
            component Marker { value: 0 }
            resource Counter { value: 0 }
            transaction Consume(target: entity) {
                requires has(target, Marker)
                changes_only [Marker, Counter]
                remove(target, Marker)
                set_resource(Counter, Counter { value: 4 })
                ensures not has(target, Marker)
                ensures res(Counter).value == 4
            }
            let item = spawn(Marker { value: 9 })
            Consume(item)
            print(has(item, Marker))
            print(res(Counter).value)
            "#,
        );
        assert_eq!(output, vec!["false", "4"]);
    }

    #[test]
    fn transaction_can_spawn_and_despawn_with_explicit_structural_authority() {
        let output = run_source(
            r#"
            component Marker { value: 0 }
            transaction Create() {
                requires true
                changes_only [Marker, "$entities", "$entity_identity"]
                let _created = spawn(Marker { value: 7 })
                ensures true
            }
            transaction Destroy(target: entity) {
                requires has(target, Marker)
                changes_only [*]
                despawn(target)
                ensures not has(target, Marker)
            }
            Create()
            let item = spawn(Marker { value: 9 })
            Destroy(item)
            print(len(query { Marker }))
            "#,
        );
        assert_eq!(output, vec!["1"]);
    }

    #[test]
    fn transaction_undo_journal_matches_reference_histories() {
        #[derive(Clone)]
        struct EntityState {
            binding: String,
            live: bool,
            has_right: bool,
        }

        fn next(seed: &mut u64) -> u64 {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            *seed
        }

        fn program(transaction_body: Option<(&str, bool)>) -> String {
            let declarations = r#"
                component Left { value: 0 }
                component Right { value: 0 }
                resource Counter { value: 3 }
            "#;
            let base = r#"
                let e0 = spawn("base-0", Left { value: 10 }, Right { value: 20 })
                let e1 = spawn("base-1", Left { value: 11 }, Right { value: 21 })
                let e2 = spawn("base-2", Left { value: 12 }, Right { value: 22 })
                let e3 = spawn("base-3", Left { value: 13 }, Right { value: 23 })
            "#;
            match transaction_body {
                None => format!("{declarations}\n{base}"),
                Some((body, commit)) => {
                    let ensure = if commit { "true" } else { "false" };
                    format!(
                        "{declarations}\ntransaction Apply(e0: entity, e1: entity, e2: entity, e3: entity) {{\n  requires true\n  changes_only [*]\n{body}\n  ensures {ensure}\n}}\n{base}\nApply(e0, e1, e2, e3)"
                    )
                }
            }
        }

        let (mut baseline, baseline_result) = transaction_vm(&program(None));
        baseline_result.expect("base world initializes");
        let baseline_digest = baseline.world.content_digest();
        let baseline_next_entity = baseline.world.spawn_entity(None).unwrap();

        for history_index in 0..32_u64 {
            let mut seed = 0x9e37_79b9_7f4a_7c15_u64 ^ history_index;
            let mut entities = (0..4)
                .map(|index| EntityState {
                    binding: format!("e{index}"),
                    live: true,
                    has_right: true,
                })
                .collect::<Vec<_>>();
            let mut body = String::new();
            let mut spawned = 0_u32;

            for operation_index in 0..48_u32 {
                let live = entities
                    .iter()
                    .enumerate()
                    .filter(|(_, state)| state.live)
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                let selected = live[(next(&mut seed) as usize) % live.len()];
                let value = (history_index as i64) * 1000 + i64::from(operation_index);
                match next(&mut seed) % 6 {
                    0 => body.push_str(&format!(
                        "  set({}, Left {{ value: {value} }})\n",
                        entities[selected].binding
                    )),
                    1 => {
                        if entities[selected].has_right {
                            body.push_str(&format!(
                                "  remove({}, Right)\n",
                                entities[selected].binding
                            ));
                            entities[selected].has_right = false;
                        } else {
                            body.push_str(&format!(
                                "  set({}, Right {{ value: {value} }})\n",
                                entities[selected].binding
                            ));
                            entities[selected].has_right = true;
                        }
                    }
                    2 => body.push_str(&format!(
                        "  set_resource(Counter, Counter {{ value: {value} }})\n"
                    )),
                    3 => {
                        let binding = format!("tx{spawned}");
                        body.push_str(&format!(
                            "  let {binding} = spawn(\"history-{history_index}-{spawned}\", Left {{ value: {value} }}, Right {{ value: {value} }})\n"
                        ));
                        entities.push(EntityState {
                            binding,
                            live: true,
                            has_right: true,
                        });
                        spawned += 1;
                    }
                    4 if live.len() > 1 => {
                        body.push_str(&format!(
                            "  despawn({})\n",
                            entities[selected].binding
                        ));
                        entities[selected].live = false;
                    }
                    _ => body.push_str(&format!(
                        "  set({}, Left {{ value: {value} }})\n  set({}, Left {{ value: {} }})\n",
                        entities[selected].binding,
                        entities[selected].binding,
                        value + 1
                    )),
                }
            }

            let reference_source = format!(
                "{}\n{}",
                program(None),
                body.lines()
                    .map(|line| line.strip_prefix("  ").unwrap_or(line))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            let (reference, reference_result) = transaction_vm(&reference_source);
            reference_result.unwrap_or_else(|error| {
                panic!("reference history {history_index} failed: {error}\n{body}")
            });

            let (committed, committed_result) = transaction_vm(&program(Some((&body, true))));
            committed_result.unwrap_or_else(|error| {
                panic!("committed history {history_index} failed: {error}\n{body}")
            });
            assert_eq!(
                committed.world.content_digest(),
                reference.world.content_digest(),
                "committed history {history_index} diverged from the non-transactional reference\n{body}"
            );

            let (mut rejected, rejected_result) = transaction_vm(&program(Some((&body, false))));
            let error = rejected_result.expect_err("the differential rollback transaction rejects");
            assert!(error.contains("failed postcondition"), "{error}");
            assert_eq!(
                rejected.world.content_digest(),
                baseline_digest,
                "rollback history {history_index} did not restore the base world\n{body}"
            );
            assert_eq!(
                rejected.world.spawn_entity(None).unwrap(),
                baseline_next_entity,
                "rollback history {history_index} did not restore allocator identity"
            );
        }
    }
