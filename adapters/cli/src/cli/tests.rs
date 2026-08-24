#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_flags_are_recognized() {
        assert!(wants_help(&["rad".to_string(), "--help".to_string()]));
        assert!(wants_help(&["rad".to_string(), "-h".to_string()]));
        assert!(!wants_help(&["rad".to_string()]));
        assert!(!wants_help(&[
            "rad".to_string(),
            "script.rad".to_string(),
            "--help".to_string(),
        ]));
    }

    #[test]
    fn parse_cli_args_parses_authority_effects_with_json() {
        let args = vec![
            "rad".to_string(),
            "effects".to_string(),
            "RemoveEntity".to_string(),
            "--json".to_string(),
            "--file".to_string(),
            "mission.rad".to_string(),
        ];

        match parse_cli_args(&args).unwrap() {
            CliCommand::Authority {
                query: AuthorityQuery::Effects { symbol },
                filepath,
                json,
            } => {
                assert_eq!(symbol, "RemoveEntity");
                assert_eq!(filepath, "mission.rad");
                assert!(json);
            }
            command => panic!("expected effects command, got {command:?}"),
        }
    }

    #[test]
    fn parse_cli_args_parses_authority_indexes_and_path() {
        let cases = [("writers", "LiveMembership"), ("readers", "WireIdentity")];
        for (name, authority) in cases {
            let args = vec![
                "rad".to_string(),
                name.to_string(),
                authority.to_string(),
                "--file=mission.rad".to_string(),
            ];
            match parse_cli_args(&args).unwrap() {
                CliCommand::Authority {
                    query: AuthorityQuery::Writers { authority: actual },
                    filepath,
                    json: false,
                } if name == "writers" => {
                    assert_eq!(actual, authority);
                    assert_eq!(filepath, "mission.rad");
                }
                CliCommand::Authority {
                    query: AuthorityQuery::Readers { authority: actual },
                    filepath,
                    json: false,
                } if name == "readers" => {
                    assert_eq!(actual, authority);
                    assert_eq!(filepath, "mission.rad");
                }
                command => panic!("expected {name} command, got {command:?}"),
            }
        }

        let args = vec![
            "rad".to_string(),
            "path".to_string(),
            "mission_frame".to_string(),
            "->".to_string(),
            "full_scan".to_string(),
            "--file".to_string(),
            "mission.rad".to_string(),
            "--json".to_string(),
        ];
        match parse_cli_args(&args).unwrap() {
            CliCommand::Authority {
                query: AuthorityQuery::Path { from, to },
                filepath,
                json: true,
            } => {
                assert_eq!(from, "mission_frame");
                assert_eq!(to, "full_scan");
                assert_eq!(filepath, "mission.rad");
            }
            command => panic!("expected path command, got {command:?}"),
        }
    }

    #[test]
    fn parse_cli_args_parses_operational_inspection_commands() {
        let single_target = [
            (
                "query-plan",
                OperationalQuery::QueryPlan {
                    target: "Advance".into(),
                },
            ),
            (
                "cost-path",
                OperationalQuery::CostPath {
                    target: "Advance".into(),
                },
            ),
        ];
        for (command, expected) in single_target {
            let args = vec![
                "rad".to_string(),
                command.to_string(),
                "Advance".to_string(),
                "--file=service.rad".to_string(),
                "--json".to_string(),
            ];
            match parse_cli_args(&args).unwrap() {
                CliCommand::Operational {
                    query,
                    filepath,
                    json,
                } => {
                    assert_eq!(query, expected);
                    assert_eq!(filepath, "service.rad");
                    assert!(json);
                }
                parsed => panic!("expected {command}, got {parsed:?}"),
            }
        }

        let cases = [
            (
                "why",
                vec!["42", "Health"],
                OperationalQuery::Why {
                    entity: "42".into(),
                    component: "Health".into(),
                },
            ),
            (
                "why-removed",
                vec!["42", "Alive"],
                OperationalQuery::WhyRemoved {
                    entity: "42".into(),
                    component: "Alive".into(),
                },
            ),
            (
                "why-not-in-view",
                vec!["AlivePlayers", "42"],
                OperationalQuery::WhyNotInView {
                    view: "AlivePlayers".into(),
                    entity: "42".into(),
                },
            ),
        ];
        for (command, positional, expected) in cases {
            let mut args = vec!["rad".to_string(), command.to_string()];
            args.extend(positional.into_iter().map(str::to_string));
            args.extend(["--file".to_string(), "service.rad".to_string()]);
            match parse_cli_args(&args).unwrap() {
                CliCommand::Operational {
                    query,
                    filepath,
                    json: false,
                } => {
                    assert_eq!(query, expected);
                    assert_eq!(filepath, "service.rad");
                }
                parsed => panic!("expected {command}, got {parsed:?}"),
            }
        }

        let args = vec![
            "rad".to_string(),
            "why-field".to_string(),
            "alice".to_string(),
            "DeviceTrust".to_string(),
            "level".to_string(),
            "--file=service.rad".to_string(),
        ];
        match parse_cli_args(&args).unwrap() {
            CliCommand::Operational {
                query:
                    OperationalQuery::WhyField {
                        entity,
                        component,
                        field,
                    },
                filepath,
                json: false,
            } => {
                assert_eq!(entity, "alice");
                assert_eq!(component, "DeviceTrust");
                assert_eq!(field, "level");
                assert_eq!(filepath, "service.rad");
            }
            parsed => panic!("expected why-field, got {parsed:?}"),
        }
    }

    #[test]
    fn parse_cli_args_parses_bench_with_runtime_metrics() {
        let args = vec![
            "rad".to_string(),
            "bench".to_string(),
            "projects/dogfood/dispatch60/bench.rad".to_string(),
            "--json".to_string(),
            "--".to_string(),
            "10000".to_string(),
        ];
        match parse_cli_args(&args).expect("bench command") {
            CliCommand::Bench {
                filepath,
                json,
                program_args,
            } => {
                assert_eq!(filepath, "projects/dogfood/dispatch60/bench.rad");
                assert!(json);
                assert_eq!(program_args, ["10000"]);
            }
            parsed => panic!("expected bench command, got {parsed:?}"),
        }
    }

    #[test]
    fn parse_cli_args_parses_compiler_owned_surface_report() {
        let args = ["rad", "surface", "projects/dogfood/sovereign-grid/main.rad", "--json"]
            .map(str::to_string)
            .to_vec();
        match parse_cli_args(&args).expect("surface command") {
            CliCommand::Surface { filepath, json } => {
                assert_eq!(filepath, "projects/dogfood/sovereign-grid/main.rad");
                assert!(json);
            }
            parsed => panic!("expected surface command, got {parsed:?}"),
        }
    }

    #[test]
    fn parse_cli_args_parses_model_check_campaign() {
        let args = [
            "rad",
            "model-check",
            "projects/dogfood/workpulse/tests/job_model.rad",
            "--model",
            "JobLifecycleModel",
            "--runs",
            "10000",
            "--max-commands",
            "200",
            "--seed",
            "1234",
            "--artifact-dir",
            "artifacts/workpulse/failures",
            "--json",
        ]
        .map(str::to_string)
        .to_vec();

        match parse_cli_args(&args).expect("model-check command") {
            CliCommand::ModelCheck {
                filepath,
                model,
                runs,
                max_commands,
                seed,
                artifact_directory,
                json,
            } => {
                assert_eq!(
                    filepath,
                    "projects/dogfood/workpulse/tests/job_model.rad"
                );
                assert_eq!(model.as_deref(), Some("JobLifecycleModel"));
                assert_eq!(runs, 10_000);
                assert_eq!(max_commands, 200);
                assert_eq!(seed, 1234);
                assert_eq!(
                    artifact_directory.as_deref(),
                    Some("artifacts/workpulse/failures")
                );
                assert!(json);
            }
            parsed => panic!("expected model-check command, got {parsed:?}"),
        }
    }

    #[test]
    fn parse_cli_args_parses_model_failure_shrink() {
        let args = [
            "rad",
            "shrink",
            "artifacts/workpulse/failures/stale_completion.radr",
        ]
        .map(str::to_string)
        .to_vec();

        match parse_cli_args(&args).expect("shrink command") {
            CliCommand::ShrinkModel { artifact } => assert_eq!(
                artifact,
                "artifacts/workpulse/failures/stale_completion.radr"
            ),
            parsed => panic!("expected shrink command, got {parsed:?}"),
        }
    }

    #[test]
    fn parse_cli_args_rejects_empty_model_campaigns() {
        for option in ["--runs", "--max-commands"] {
            let args = [
                "rad",
                "model-check",
                "model.rad",
                option,
                "0",
            ]
            .map(str::to_string)
            .to_vec();
            let error = parse_cli_args(&args).expect_err("zero-sized campaign must fail");
            assert!(error.contains("must be positive"), "{error}");
        }
    }

    #[test]
    fn parse_cli_args_parses_ffi_verification() {
        let args = [
            "rad",
            "ffi",
            "verify",
            "plugins/risk_model.dll",
            "--json",
        ]
        .map(str::to_string)
        .to_vec();
        match parse_cli_args(&args).expect("ffi verify command") {
            CliCommand::FfiVerify {
                plugin,
                contract,
                json,
            } => {
                assert_eq!(plugin, "plugins/risk_model.dll");
                assert_eq!(contract, None);
                assert!(json);
            }
            parsed => panic!("expected ffi verify command, got {parsed:?}"),
        }
    }

    #[test]
    fn operational_commands_use_checked_plans_and_executed_provenance() {
        let path = std::env::temp_dir().join(format!(
            "rad-operational-command-{}.rad",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"
            component State { active: bool = false }
            materialized view Active {
                depends [State]
                where State.active == true
            }

            fn inspect(target: entity) -> nil {
                assert(has(target, State), "view only yields State")
            }

            @frame
            @no_full_scan
            @no_guest_allocation
            @budget(instructions: 100)
            system Inspect(reads State) {
                visit_view(Active, inspect)
            }

            fn main() -> nil {
                let target = spawn(State { active: false })
                remove(target, State)
            }
            "#,
        )
        .expect("write operational fixture");
        let path_text = path.to_string_lossy().into_owned();

        let inspected = load_operational_program(&path_text).expect("checked operational fixture");
        let view_plan = render_query_plan("Active", &inspected, false).expect("view plan");
        assert!(view_plan.contains("maintenance: incremental, same commit"));
        assert!(view_plan.contains("full scans: 0"));
        let callable_plan = render_query_plan("Inspect", &inspected, false).expect("callable plan");
        assert!(callable_plan.contains("visit_view Active: O(k), allocates false"));
        assert!(callable_plan.contains("@budget=100"));
        let cost_path =
            render_cost_path("Inspect", &inspected.authority, false).expect("cost path");
        assert!(cost_path.contains("full scan: none"));
        assert!(cost_path.contains("allocation: none"));

        let vm = run_inspected_program(inspected).expect("run operational fixture");
        let entity = resolve_operational_entity(&vm, "0").expect("numeric entity selector");
        let (member, reason) = vm
            .get_world()
            .materialized_view_reason("Active", entity)
            .expect("view explanation");
        assert!(!member);
        assert_eq!(reason, "missing State");
        let removed = vm.causality_ledger().explain_removed(entity, "State");
        assert!(removed.contains("remove"), "{removed}");

        std::fs::remove_file(path).expect("remove operational fixture");
    }

    #[test]
    fn parse_cli_args_parses_types_generation_with_semantic_features() {
        let args = vec![
            "rad".to_string(),
            "types".to_string(),
            "api.rad".to_string(),
            "api.d.ts".to_string(),
            "--feature".to_string(),
            "causal-laws".to_string(),
        ];
        match parse_cli_args(&args).unwrap() {
            CliCommand::Types {
                input_rad,
                output_typescript,
                features,
            } => {
                assert_eq!(input_rad, "api.rad");
                assert_eq!(output_typescript, "api.d.ts");
                assert_eq!(features, vec!["causal-laws"]);
            }
            command => panic!("expected types command, got {command:?}"),
        }
    }

    fn assert_parses_run_with_no_check(args: Vec<String>) {
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run {
                filepath,
                skip_check,
                ..
            } => {
                assert_eq!(filepath, "script.rad");
                assert!(skip_check);
            }
            CliCommand::Version
            | CliCommand::Surface { .. }
            | CliCommand::Authority { .. }
            | CliCommand::Operational { .. }
            | CliCommand::Bench { .. } => {
                panic!("expected run command")
            }
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Types { .. }
            | CliCommand::Replay { .. }
            | CliCommand::ModelCheck { .. }
            | CliCommand::ShrinkModel { .. }
            | CliCommand::FfiVerify { .. } => todo!(),
        }
    }

    #[test]
    fn parse_cli_args_accepts_no_check_before_and_after_file() {
        let cases = vec![
            vec![
                "rad".to_string(),
                "--no-check".to_string(),
                "script.rad".to_string(),
            ],
            vec![
                "rad".to_string(),
                "script.rad".to_string(),
                "--no-check".to_string(),
            ],
        ];

        for args in cases {
            assert_parses_run_with_no_check(args);
        }
    }

    #[test]
    fn parse_cli_args_rejects_recording_without_semantic_analysis() {
        let args = vec![
            "rad".to_string(),
            "script.rad".to_string(),
            "--no-check".to_string(),
            "--record".to_string(),
            "trace.radr".to_string(),
        ];
        let error = parse_cli_args(&args).expect_err("unchecked traces are not replay-safe");
        assert!(
            error.contains("replay requires the checked semantic product"),
            "{error}"
        );
    }

    #[test]
    fn parse_cli_args_accepts_bounded_relations_check() {
        let args = vec![
            "rad".to_string(),
            "relations".to_string(),
            "check".to_string(),
            "facts.rad".to_string(),
            "--experimental-relations".to_string(),
            "--module".to_string(),
            "game::facts".to_string(),
        ];
        match parse_cli_args(&args).unwrap() {
            CliCommand::RelationsCheck {
                filepath,
                module_id,
                experimental_relations,
            } => {
                assert_eq!(filepath, "facts.rad");
                assert_eq!(module_id, "game::facts");
                assert!(experimental_relations);
            }
            _ => panic!("expected relations check command"),
        }
    }

    #[test]
    fn parse_cli_args_feature_gates_relation_lsp_support() {
        let args = vec![
            "rad".to_string(),
            "lsp".to_string(),
            "--experimental-relations".to_string(),
        ];
        assert!(matches!(
            parse_cli_args(&args).unwrap(),
            CliCommand::Lsp {
                experimental_relations: true
            }
        ));
    }

    #[test]
    fn parse_cli_args_supports_version_anywhere_without_file() {
        let args = vec![
            "rad".to_string(),
            "--no-check".to_string(),
            "--version".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        assert!(matches!(parsed, CliCommand::Version));
    }

    #[test]
    fn parse_cli_args_rejects_removed_compatibility_flags() {
        for removed in ["--compat-v0.5-dx", "--no-compat-v0.5-dx", "--warn-compat"] {
            let args = vec![
                "rad".to_string(),
                removed.to_string(),
                "script.rad".to_string(),
            ];
            let error = parse_cli_args(&args).expect_err("removed flag must not remain accepted");
            assert!(error.contains("Unknown option"), "{removed}: {error}");
        }
    }

    #[test]
    fn parse_cli_args_accepts_warning_policy_flags() {
        let args = vec![
            "rad".to_string(),
            "--deny-warnings".to_string(),
            "script.rad".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run {
                filepath,
                skip_check,
                deny_warnings,
                strict_types,
                write_lock,
                ..
            } => {
                assert_eq!(filepath, "script.rad");
                assert!(!skip_check);
                assert!(deny_warnings);
                assert!(!strict_types);
                assert!(!write_lock);
            }
            CliCommand::Version
            | CliCommand::Surface { .. }
            | CliCommand::Authority { .. }
            | CliCommand::Operational { .. }
            | CliCommand::Bench { .. } => {
                panic!("expected run command")
            }
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Types { .. }
            | CliCommand::Replay { .. }
            | CliCommand::ModelCheck { .. }
            | CliCommand::ShrinkModel { .. }
            | CliCommand::FfiVerify { .. } => todo!(),
        }
    }

    #[test]
    fn parse_cli_args_accepts_strict_and_lock_flags() {
        let args = vec![
            "rad".to_string(),
            "--strict-types".to_string(),
            "--write-lock".to_string(),
            "script.rad".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run {
                strict_types,
                write_lock,
                ..
            } => {
                assert!(strict_types);
                assert!(write_lock);
            }
            CliCommand::Version
            | CliCommand::Surface { .. }
            | CliCommand::Authority { .. }
            | CliCommand::Operational { .. }
            | CliCommand::Bench { .. } => {
                panic!("expected run command")
            }
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Types { .. }
            | CliCommand::Replay { .. }
            | CliCommand::ModelCheck { .. }
            | CliCommand::ShrinkModel { .. }
            | CliCommand::FfiVerify { .. } => todo!(),
        }
    }

    #[test]
    fn parse_cli_args_accepts_profile_copies_flag() {
        let args = vec![
            "rad".to_string(),
            "--profile-copies".to_string(),
            "script.rad".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Run { profile_copies, .. } => {
                assert!(profile_copies);
            }
            CliCommand::Version
            | CliCommand::Surface { .. }
            | CliCommand::Authority { .. }
            | CliCommand::Operational { .. }
            | CliCommand::Bench { .. } => {
                panic!("expected run command")
            }
            CliCommand::Fmt { .. } => panic!("expected run command"),
            CliCommand::Lint { .. } => panic!("expected run command"),
            CliCommand::Test { .. } => panic!("expected run command"),
            CliCommand::Lsp { .. } | CliCommand::RelationsCheck { .. } => {
                panic!("expected run command")
            }
            CliCommand::Build { .. } => panic!("expected run command"),
            CliCommand::New { .. }
            | CliCommand::Snapshot { .. }
            | CliCommand::Play { .. }
            | CliCommand::SandboxServe { .. }
            | CliCommand::Types { .. }
            | CliCommand::Replay { .. }
            | CliCommand::ModelCheck { .. }
            | CliCommand::ShrinkModel { .. }
            | CliCommand::FfiVerify { .. } => {
                todo!()
            }
        }
    }

    #[test]
    fn parse_cli_args_accepts_record_flag() {
        for args in [
            vec![
                "rad".to_string(),
                "script.rad".to_string(),
                "--record".to_string(),
                "trace.radr".to_string(),
            ],
            vec![
                "rad".to_string(),
                "--record=trace.radr".to_string(),
                "script.rad".to_string(),
            ],
        ] {
            let parsed = parse_cli_args(&args).unwrap();
            match parsed {
                CliCommand::Run { record, .. } => {
                    assert_eq!(record.as_deref(), Some("trace.radr"));
                }
                other => panic!("expected run command, got {:?}", other),
            }
        }
        let missing = vec![
            "rad".to_string(),
            "script.rad".to_string(),
            "--record".to_string(),
        ];
        assert!(parse_cli_args(&missing).is_err());
    }

    /// `rad run` outside a project directory must explain itself instead of
    /// trying to open a file literally named "run".
    #[test]
    fn parse_cli_args_run_without_rad_toml_is_a_helpful_error() {
        let args: Vec<String> = ["rad", "run"].iter().map(|s| s.to_string()).collect();
        // cargo test runs in core/vm/, which has no rad.toml
        let err = parse_cli_args(&args).unwrap_err();
        assert!(err.contains("no rad.toml"), "got: {}", err);
        assert!(err.contains("rad new"), "got: {}", err);
    }

    /// Everything after `--` belongs to the program: flags are not parsed,
    /// and sys_args() receives exactly these strings.
    #[test]
    fn parse_cli_args_passes_program_args_after_double_dash() {
        let args: Vec<String> = ["rad", "script.rad", "--", "alice", "work/dir", "--record"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        match parse_cli_args(&args).unwrap() {
            CliCommand::Run {
                filepath,
                record,
                program_args,
                ..
            } => {
                assert_eq!(filepath, "script.rad");
                // `--record` after `--` is data, not a rad flag
                assert_eq!(record, None);
                assert_eq!(program_args, vec!["alice", "work/dir", "--record"]);
            }
            other => panic!("expected run command, got {:?}", other),
        }
    }

    #[test]
    fn parse_cli_args_parses_replay_command() {
        let args = vec![
            "rad".to_string(),
            "replay".to_string(),
            "trace.radr".to_string(),
            "--to-frame".to_string(),
            "42".to_string(),
            "--force".to_string(),
        ];
        match parse_cli_args(&args).unwrap() {
            CliCommand::Replay {
                trace_path,
                to_frame,
                force,
                serve,
                with_source,
            } => {
                assert_eq!(trace_path, "trace.radr");
                assert_eq!(to_frame, Some(42));
                assert!(force);
                assert!(!serve);
                assert!(with_source.is_none());
            }
            other => panic!("expected replay command, got {:?}", other),
        }
        // Missing trace path is an error.
        let missing = vec!["rad".to_string(), "replay".to_string()];
        assert!(parse_cli_args(&missing).is_err());
        // Bad frame number is an error.
        let bad = vec![
            "rad".to_string(),
            "replay".to_string(),
            "t.radr".to_string(),
            "--to-frame=abc".to_string(),
        ];
        assert!(parse_cli_args(&bad).is_err());
    }

    #[test]
    fn parse_cli_args_parses_explicit_build_targets() {
        let args = vec![
            "rad".to_string(),
            "build".to_string(),
            "--target".to_string(),
            "compiler-wasm".to_string(),
            "a.rad".to_string(),
            "out.wasm".to_string(),
        ];
        let parsed = parse_cli_args(&args).unwrap();
        match parsed {
            CliCommand::Build {
                input_rad,
                output,
                target,
            } => {
                assert_eq!(input_rad, "a.rad");
                assert_eq!(output, "out.wasm");
                assert_eq!(target, BuildTarget::CompilerWasm);
            }
            _ => panic!("expected build"),
        }

        let browser = vec![
            "rad".to_string(),
            "build".to_string(),
            "--target".to_string(),
            "browser-package".to_string(),
            "a.rad".to_string(),
            "out.radpkg.json".to_string(),
        ];
        assert!(matches!(
            parse_cli_args(&browser).unwrap(),
            CliCommand::Build {
                target: BuildTarget::BrowserPackage,
                ..
            }
        ));
    }

    #[test]
    fn parse_cli_args_rejects_unknown_option() {
        let args = vec!["rad".to_string(), "--wat".to_string()];
        let err = parse_cli_args(&args).unwrap_err();
        assert!(err.contains("Unknown option"));
    }

    #[test]
    fn parse_cli_args_rejects_version_with_input_file() {
        let args = vec![
            "rad".to_string(),
            "--version".to_string(),
            "script.rad".to_string(),
        ];
        let err = parse_cli_args(&args).unwrap_err();
        assert!(err.contains("--version cannot be combined"));
    }

    #[test]
    fn parse_cli_args_requires_single_input_file() {
        let args = vec!["rad".to_string()];
        let err = parse_cli_args(&args).unwrap_err();
        assert!(err.contains("Usage:"));
    }

    #[test]
    fn format_error_caret_aligns_with_column() {
        let out = format_error("ab\nxyz", "file.rad", "bad", 2, 2);
        assert!(out.contains(">>    2 | xyz"));
        assert!(out.contains("^"));
    }

    #[test]
    fn allocation_contract_categories_fail_independently() {
        let runtime_contracts = rad_vm::types::AuthorityContracts {
            no_runtime_allocation: true,
            ..Default::default()
        };
        let runtime_metric = rad_vm::vm::SystemExecutionMetrics {
            native_allocation_meter_supported: true,
            max_runtime_allocations: 1,
            ..Default::default()
        };
        let runtime =
            assess_allocation_contracts(Some(&runtime_contracts), false, &runtime_metric);
        assert!(!runtime.runtime_pass);
        assert!(runtime.guest_pass);
        assert!(runtime.host_pass);

        let guest_contracts = rad_vm::types::AuthorityContracts {
            no_guest_allocation: true,
            ..Default::default()
        };
        let guest_metric = rad_vm::vm::SystemExecutionMetrics {
            native_allocation_meter_supported: true,
            max_guest_allocations: 1,
            ..Default::default()
        };
        let guest = assess_allocation_contracts(Some(&guest_contracts), false, &guest_metric);
        assert!(!guest.guest_pass);
        assert!(guest.runtime_pass);

        let host_contracts = rad_vm::types::AuthorityContracts {
            no_host_allocation: true,
            ..Default::default()
        };
        let host_metric = rad_vm::vm::SystemExecutionMetrics {
            native_allocation_meter_supported: true,
            max_host_boundary_allocations: 1,
            ..Default::default()
        };
        let host = assess_allocation_contracts(Some(&host_contracts), false, &host_metric);
        assert!(!host.host_pass);
    }

    #[test]
    fn parallel_test_summary_retains_all_failure_categories() {
        assert_eq!(
            parse_test_summary(
                "7 passed, 2 failed, 9 total (3 file(s) with no tests) (4 file(s) failed to run)"
            ),
            Some((7, 2, 4, 3))
        );
        assert_eq!(parse_test_summary("not a test summary"), None);
    }
}
