    #[test]
    fn aliased_module_forward_callback_is_compiled_as_a_view_kernel() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            r#"
pub component Active {}
pub component Position { x: int = 0 }
pub materialized view Movers { depends [Active, Position] }

pub fn run() -> nil {
    let target = spawn(Active {}, Position { x: 4 })
    visit_view(Movers, advance)
    assert(require(target, Position).x == 5, "aliased callback ran once")
}

fn advance(target: entity) -> nil {
    let x = read_field(target, Position, "x")
    write_field(target, Position, "x", x + 1)
}
"#,
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { owner.run() }\n",
        )
        .unwrap();

        let compiled = compile_module_entry(&dir.join("main.rad"));
        assert_eq!(
            compiled.view_kernels.len(),
            1,
            "a later callback declaration in an aliased module must emit RunViewKernel"
        );
        let mut vm = VM::new();
        vm.load_compile_result(compiled);
        vm.run(0).expect("run fused aliased callback");
    }

    #[test]
    fn aliased_component_presence_pattern_uses_canonical_component_identity() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            r#"
pub component Cell { value: int = 0 }
pub fn run() -> nil {
    let target: entity = entity { Cell { value: 9 } }
    match target {
        has Cell(cell) => { assert(cell.value == 9, "canonical component matched") }
        _ => { assert(false, "component pattern fell through") }
    }
}
"#,
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { owner.run() }\n",
        )
        .unwrap();

        assert!(run_module_entry(&dir.join("main.rad")).is_empty());
    }

    fn write_phase_module(path: &Path) {
        fs::write(
            path,
            r#"
pub resource Counter { value: int = 0 }

pub system Tick(counter: mut Counter) {
    counter.value = counter.value + 1
}

pub phase Frame { Tick }

pub fn reset() -> nil {
    set_resource(Counter, Counter { value: 0 })
}

pub readonly fn count() -> int {
    return res(Counter).value
}
"#,
        )
        .unwrap();
    }

    #[test]
    fn aliased_public_phase_expands_to_its_canonical_system() {
        let dir = mk_temp_dir();
        write_phase_module(&dir.join("owner.rad"));
        fs::write(
            dir.join("main.rad"),
            concat!(
                "use \"owner.rad\" as owner\n",
                "fn main() -> nil { owner.reset() schedule [owner.Frame] print(owner.count()) }\n",
            ),
        )
        .unwrap();

        assert_eq!(run_module_entry(&dir.join("main.rad")), vec!["1"]);
    }

    #[test]
    fn aliased_serial_phase_stamps_its_canonical_members() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            concat!(
                "pub system First() {}\n",
                "pub system Second() {}\n",
                "pub serial phase Frame { First, Second }\n",
            ),
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { schedule [owner.Frame] }\n",
        )
        .unwrap();

        let compiled = compile_module_entry(&dir.join("main.rad"));
        let members = compiled
            .systems
            .iter()
            .filter(|system| system.name.ends_with("__First") || system.name.ends_with("__Second"))
            .collect::<Vec<_>>();
        assert_eq!(members.len(), 2);
        assert!(members[0].serial_group.is_some());
        assert_eq!(members[0].serial_group, members[1].serial_group);
    }

    #[test]
    fn aliased_phase_is_a_bounded_deferred_authority_edge() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            concat!(
                "pub component Score { value: int = 0 }\n",
                "pub system Tick(score: mut Score) { score.value = score.value + 1 }\n",
                "phase Frame { Tick }\n",
                "pub fn run_frame() -> nil { schedule [Frame] }\n",
            ),
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { owner.run_frame() }\n",
        )
        .unwrap();

        let loaded = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        let score = loaded.aliases["owner"].canonical_symbol("Score");
        let analysis = crate::pipeline::analyze_program(
            &loaded.program,
            &loaded.aliases,
            CheckerOptions::default(),
        );
        assert!(analysis.errors().is_empty(), "{:?}", analysis.errors());
        let report = analysis.output().authority();
        let run_frame = report.resolve("run_frame").unwrap();
        assert!(!run_frame.synchronous.writes.contains(&score));
        assert!(run_frame.transitive.writes.contains(&score));
        assert!(!run_frame.transitive.unknown);
        assert!(report.path("run_frame", "Tick").unwrap().is_some());
    }

    #[test]
    fn aliased_event_handlers_remain_inside_simulation_safety_analysis() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            concat!(
                "pub component Unit {}\n",
                "pub event Ping { value: int }\n",
                "pub system EmitPing(unit: Unit, emits Ping) { emit Ping { value: 1 } }\n",
                "on Ping(evt) { print(evt.value) }\n",
                "pub fn simulate_once() -> nil { let _ = spawn(Unit {}) let _ = simulate(fork(), [system::EmitPing], 1) }\n",
            ),
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { owner.simulate_once() }\n",
        )
        .unwrap();

        let loaded = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let analysis = crate::pipeline::analyze_program(
            &loaded.program,
            &loaded.aliases,
            CheckerOptions::default(),
        );
        assert!(
            analysis.errors().iter().any(|error| {
                error.message.contains("handler `on Ping`")
                    && error.message.contains("IO builtin 'print'")
            }),
            "{:?}",
            analysis.errors()
        );
    }

    #[test]
    fn two_aliases_of_one_phase_share_one_canonical_identity() {
        let dir = mk_temp_dir();
        write_phase_module(&dir.join("owner.rad"));
        fs::write(
            dir.join("main.rad"),
            concat!(
                "use \"owner.rad\" as left\n",
                "use \"owner.rad\" as right\n",
                "fn main() -> nil { left.reset() schedule [left.Frame, right.Frame] print(left.count()) }\n",
            ),
        )
        .unwrap();

        assert_eq!(
            run_module_entry(&dir.join("main.rad")),
            vec!["1"],
            "one canonical system appears once in a deduplicated schedule"
        );
    }

    #[test]
    fn bare_and_aliased_phase_import_share_the_flat_identity() {
        let dir = mk_temp_dir();
        write_phase_module(&dir.join("owner.rad"));
        fs::write(
            dir.join("main.rad"),
            concat!(
                "use \"owner.rad\"\n",
                "use \"owner.rad\" as owner\n",
                "fn main() -> nil { reset() schedule [owner.Frame] print(count()) }\n",
            ),
        )
        .unwrap();

        assert_eq!(run_module_entry(&dir.join("main.rad")), vec!["1"]);
    }

    #[test]
    fn identical_phase_sources_at_different_paths_remain_distinct() {
        let dir = mk_temp_dir();
        write_phase_module(&dir.join("left.rad"));
        write_phase_module(&dir.join("right.rad"));
        fs::write(
            dir.join("main.rad"),
            concat!(
                "use \"left.rad\" as left\n",
                "use \"right.rad\" as right\n",
                "fn main() -> nil { left.reset() right.reset() schedule [left.Frame] print(left.count()) print(right.count()) }\n",
            ),
        )
        .unwrap();

        assert_eq!(
            run_module_entry(&dir.join("main.rad")),
            vec!["1", "0"]
        );
    }

    #[test]
    fn normalized_phase_paths_share_one_canonical_identity() {
        let dir = mk_temp_dir();
        fs::create_dir(dir.join("mods")).unwrap();
        write_phase_module(&dir.join("owner.rad"));
        fs::write(
            dir.join("main.rad"),
            concat!(
                "use \"./owner.rad\" as direct\n",
                "use \"mods/../owner.rad\" as normalized\n",
                "fn main() -> nil { direct.reset() schedule [direct.Frame, normalized.Frame] print(normalized.count()) }\n",
            ),
        )
        .unwrap();

        assert_eq!(
            run_module_entry(&dir.join("main.rad")),
            vec!["1"],
            "normalized aliases expand to one canonical scheduled system"
        );
    }

    #[test]
    fn aliased_phase_members_are_checked_in_their_module_scope() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            "pub phase Broken { MissingSystem }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { schedule [owner.Broken] }\n",
        )
        .unwrap();

        let loaded = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        let analysis = crate::pipeline::analyze_program(
            &loaded.program,
            &loaded.aliases,
            CheckerOptions::default(),
        );
        assert!(analysis.errors().iter().any(|error| {
            error.message.contains("references unknown system 'MissingSystem'")
        }));
    }

    #[test]
    fn aliased_system_dependency_cycles_use_canonical_names() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            concat!(
                "pub system First() after Second {}\n",
                "pub system Second() after First {}\n",
            ),
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { schedule [owner.First] }\n",
        )
        .unwrap();

        let loaded = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        let analysis = crate::pipeline::analyze_program(
            &loaded.program,
            &loaded.aliases,
            CheckerOptions::default(),
        );
        assert!(analysis.errors().iter().any(|error| {
            error
                .message
                .contains("Circular system dependency detected")
        }));
    }

    #[test]
    fn private_aliased_phase_is_not_exported() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            "pub system Tick() {}\nphase Internal { Tick }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { schedule [owner.Internal] }\n",
        )
        .unwrap();

        let loaded = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        let analysis = crate::pipeline::analyze_program(
            &loaded.program,
            &loaded.aliases,
            CheckerOptions::default(),
        );
        assert!(analysis
            .errors()
            .iter()
            .any(|error| error.message.contains("Running unknown system 'owner.Internal'")));
    }

    #[test]
    fn public_phase_does_not_reexport_a_private_system() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            "system Hidden() {}\npub phase PublicFrame { Hidden }\n",
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { schedule [owner.PublicFrame] }\n",
        )
        .unwrap();

        let loaded = load_program_with_source_map(dir.join("main.rad").to_str().unwrap()).unwrap();
        let analysis = crate::pipeline::analyze_program(
            &loaded.program,
            &loaded.aliases,
            CheckerOptions::default(),
        );
        assert!(analysis
            .errors()
            .iter()
            .any(|error| error.message.contains("System '") && error.message.contains("private")));
    }

    fn write_private_default_module(path: &Path) {
        fs::write(
            path,
            concat!(
                "component HiddenComponent { value: int = 41 }\n",
                "resource HiddenResource { value: int = 7 }\n",
                "struct HiddenStruct { left: int = 3, right: int = 4 }\n",
                "pub fn exercise_defaults() -> nil {\n",
                "    let target = spawn(HiddenComponent {})\n",
                "    let pair = HiddenStruct {}\n",
                "    print(require(target, HiddenComponent).value)\n",
                "    print(res(HiddenResource).value)\n",
                "    print(pair.left + pair.right)\n",
                "}\n",
            ),
        )
        .unwrap();
    }

    #[test]
    fn aliased_private_declaration_defaults_use_canonical_type_metadata() {
        let dir = mk_temp_dir();
        write_private_default_module(&dir.join("owner.rad"));
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\" as owner\nfn main() -> nil { owner.exercise_defaults() }\n",
        )
        .unwrap();

        assert_eq!(
            run_module_entry(&dir.join("main.rad")),
            vec!["41", "7", "7"]
        );
    }

    #[test]
    fn bare_import_private_defaults_use_file_scoped_type_metadata() {
        let dir = mk_temp_dir();
        write_private_default_module(&dir.join("owner.rad"));
        fs::write(
            dir.join("main.rad"),
            "use \"owner.rad\"\nfn main() -> nil { exercise_defaults() }\n",
        )
        .unwrap();

        assert_eq!(
            run_module_entry(&dir.join("main.rad")),
            vec!["41", "7", "7"]
        );
    }

    #[test]
    fn aliased_transient_resource_stays_out_of_world_identity() {
        let dir = mk_temp_dir();
        fs::write(
            dir.join("owner.rad"),
            concat!(
                "pub transient resource Tape { value: int = 0 }\n",
                "pub fn mutate() -> nil { set_resource(Tape, Tape { value: 1 }) }\n",
            ),
        )
        .unwrap();
        fs::write(
            dir.join("main.rad"),
            concat!(
                "use \"owner.rad\" as owner\n",
                "fn main() -> nil { print(world_digest()) owner.mutate() print(world_digest()) }\n",
            ),
        )
        .unwrap();

        let output = run_module_entry(&dir.join("main.rad"));
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], output[1]);
    }
