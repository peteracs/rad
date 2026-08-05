



fn check_causal_source(source: &str) -> Vec<crate::checker::TypeError> {
    let mut lexer = Lexer::new(source);
    let (tokens, lex_errors) = lexer.tokenize();
    assert!(lex_errors.is_empty(), "lex errors: {lex_errors:?}");
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(
        parser.errors().is_empty(),
        "parse errors: {:?}",
        parser.errors()
    );
    let mut checker = Checker::new_with_options(CheckerOptions {
        features: vec![FEATURE.to_string()],
        ..CheckerOptions::default()
    });
    checker.check(&program)
}

#[test]
fn causal_syntax_is_rejected_without_the_experimental_feature() {
    let source = r#"
intent Damage { key target: entity, amount: int }
law Hit(target: entity) { propose Damage { target: target, amount: 1 } }
resolver ResolveDamage for Damage(target, proposals) {}
component Position { x: int = 0 }
constraint WorldBounds for Position(subject, proposed) {
    require proposed.x >= 0 else "position.below_min"
}
"#;
    let mut lexer = Lexer::new(source);
    let (tokens, lex_errors) = lexer.tokenize();
    assert!(lex_errors.is_empty());
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(parser.errors().is_empty());
    let mut checker = Checker::new();
    let errors = checker.check(&program);
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("RAD Causal Laws is experimental")),
        "gate-off check must teach users to pass --experimental-laws: {errors:?}"
    );
    assert!(
        errors.iter().any(|error| {
            error.message.contains("constraint `WorldBounds`")
                && error.message.contains("experimental")
        }),
        "candidate constraints must remain behind the same gate: {errors:?}"
    );
}

#[test]
fn relation_fact_reads_are_constraint_only_and_statically_shaped() {
    let errors = check_causal_source(
        r#"
fn outside() { candidate_fact("game::facts::Marker", [1]) }
component Position { x: int = 0 }
constraint InvalidFactRead for Position(subject, proposed) {
    let relation = "game::facts::Marker"
    require base_fact(relation, proposed) else "fact.invalid"
}
"#,
    );
    assert!(errors
        .iter()
        .any(|error| error.message.contains("only valid inside a constraint")));
    assert!(errors.iter().any(|error| error
        .message
        .contains("module-qualified relation string literal")));
    assert!(errors
        .iter()
        .any(|error| error.message.contains("tuple list literal")));
}

#[test]
fn relation_fact_writes_are_resolver_only_and_statically_shaped() {
    let errors = check_causal_source(
        r#"
fn outside() { insert_fact("game::facts::Marker", [1]) }
intent Mark { key target: entity }
resolver ResolveMark for Mark(target, proposals) {
    let relation = "game::facts::Marker"
    insert_fact(relation, 1)
    replace_fact_by("game::facts::Marker", "", [1], 2)
}
"#,
    );
    assert!(errors
        .iter()
        .any(|error| error.message.contains("only valid inside a resolver")));
    assert!(errors.iter().any(|error| error
        .message
        .contains("module-qualified relation string literal")));
    assert!(errors
        .iter()
        .any(|error| error.message.contains("list literals")));
    assert!(errors
        .iter()
        .any(|error| error.message.contains("nonempty unique-constraint")));
}

#[test]
fn break_and_continue_cannot_escape_to_a_loop_outside_settlement() {
    let errors = check_causal_source(
        r#"
while true {
    settle { break }
}
while true {
    settle { continue }
}
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.message == "`break` cannot cross a settlement boundary"),
        "missing break boundary diagnostic: {errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|error| error.message == "`continue` cannot cross a settlement boundary"),
        "missing continue boundary diagnostic: {errors:?}"
    );
}

#[test]
fn loops_wholly_inside_settlement_keep_break_and_continue() {
    let source = r#"
settle {
    while true { break }
    let mut count = 0
    while count < 2 {
        count = count + 1
        continue
    }
}
"#;
    let errors = check_causal_source(source);
    assert!(
        errors.is_empty(),
        "settlement-local loop control must remain legal: {errors:?}"
    );
    let mut vm = compile_vm(source);
    vm.run(0).expect("settlement-local loops must execute");
    assert!(vm.settlement.is_none());
}

#[test]
fn causal_lowering_uses_functional_values_instead_of_inplace_heap_ops() {
    let source = r#"
settle {
    let unique mut xs = []
    xs = push(xs, 1)
    xs[0] = 2
    let mapped = xs |> map(fn(value) { value + 1 })
    let _ = mapped[0]
}
"#;
    let mut vm = compile_vm(source);
    vm.run(0)
        .expect("causal scratch computation must use functional lowering");
    for op in [
        Op::ListPushLocal,
        Op::ListSetLocal,
        Op::BitsetSetInplace,
        Op::BitsetClearInplace,
        Op::BufferAppendInplace,
        Op::ByteBufSetU8Inplace,
        Op::ByteBufSetU32LeInplace,
        Op::ByteBufSetI32LeInplace,
        Op::IterNext,
    ] {
        assert_eq!(
            vm.op_counts[op as usize], 0,
            "causal compiler executed forbidden opcode {op:?}"
        );
    }
}

#[test]
fn causal_lowering_is_transitive_through_pure_helpers() {
    let source = r#"
fn make_bits() -> bitset {
    let mut bits = bitset_new()
    bits = bitset_set(bits, 7)
    return bits
}

settle {
    let bits = make_bits()
    assert(bitset_has(bits, 7), "pure helper result")
}
"#;
    let mut vm = compile_vm(source);
    vm.run(0)
        .expect("pure helpers called by causal code must use functional lowering");
    assert_eq!(
        vm.op_counts[Op::BitsetSetInplace as usize],
        0,
        "pure helper hid an in-place heap mutation from its causal caller"
    );
}

#[test]
fn causal_lowering_rejects_heap_backed_map_iterators() {
    let source = r#"
settle {
    let values = { "a": 1 }
    for key, value in values {
        let _ = key
        let _ = value
    }
}
"#;
    let mut lexer = Lexer::new(source);
    let (tokens, lex_errors) = lexer.tokenize();
    assert!(lex_errors.is_empty(), "lex errors: {lex_errors:?}");
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(
        parser.errors().is_empty(),
        "parse errors: {:?}",
        parser.errors()
    );
    let mut checker = Checker::new_with_options(CheckerOptions {
        features: vec![FEATURE.to_string()],
        ..CheckerOptions::default()
    });
    let errors = checker.check(&program);
    assert!(errors.is_empty(), "check errors: {errors:?}");
    let error = Compiler::new()
        .with_checker_output(checker.output())
        .with_features(vec![FEATURE.to_string()])
        .compile(&program)
        .expect_err("causal map iterator needs mutable heap cursor state");
    assert!(
        error
            .message
            .contains("two-binding map iteration is not available in causal execution v0"),
        "{error:?}"
    );
}

#[test]
fn compiler_rejects_cross_settlement_loop_escape_without_checker_output() {
    let source = "while true { settle { break } }";
    let mut lexer = Lexer::new(source);
    let (tokens, lex_errors) = lexer.tokenize();
    assert!(lex_errors.is_empty());
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(parser.errors().is_empty());
    let error = Compiler::new()
        .with_features(vec![FEATURE.to_string()])
        .compile(&program)
        .expect_err("compiler must defend against a bypassed checker");
    assert_eq!(error.message, "`break` cannot cross a settlement boundary");
}

fn unbalanced_chunk(name: &str) -> Chunk {
    let mut chunk = Chunk::new(name);
    chunk.write_op(Op::BeginSettlement, 1);
    chunk.write_const(Value::NIL, 1);
    chunk.write_op(Op::Return, 1);
    chunk
}

fn balanced_return_chunk(name: &str) -> Chunk {
    let mut chunk = Chunk::new(name);
    chunk.write_const(Value::NIL, 1);
    chunk.write_op(Op::Return, 1);
    chunk
}

#[test]
fn malformed_top_level_bytecode_cannot_return_with_active_settlement() {
    let mut vm = VM::new();
    let before_world = vm.get_world().content_digest();
    let before_writes = vm.causality_ledger().writes.len();
    let before_settlements = vm.causality_ledger().settlements.len();
    let broken = vm.load_unchecked_chunk(unbalanced_chunk("unbalanced-main"));

    let error = vm
        .run(broken)
        .expect_err("successful escape past EndSettlement must become a fault");
    assert!(error.contains("would leave settlement"), "{error}");
    assert_eq!(vm.get_world().content_digest(), before_world);
    assert_eq!(vm.causality_ledger().writes.len(), before_writes);
    assert_eq!(vm.causality_ledger().settlements.len(), before_settlements);
    assert!(vm.settlement.is_none());

    let valid = vm
        .load_verified_chunk(balanced_return_chunk("valid-main"))
        .expect("balanced chunk should verify");
    vm.run(valid).expect("same VM must remain reusable");
    assert!(vm.settlement.is_none());
}

#[test]
fn malformed_host_function_cannot_return_with_active_settlement() {
    let mut vm = VM::new();
    let before_world = vm.get_world().content_digest();
    let before_settlements = vm.causality_ledger().settlements.len();
    let broken_chunk = vm.load_unchecked_chunk(unbalanced_chunk("unbalanced-host-call"));
    let broken = Value::from_fn(
        vm.gc_mut(),
        FnValue {
            name: "unbalanced".to_string(),
            arity: 0,
            chunk_id: broken_chunk,
        },
    );

    let error = vm
        .call_value(&broken, Vec::new())
        .expect_err("host-call escape past EndSettlement must become a fault");
    assert!(error.contains("would leave settlement"), "{error}");
    assert_eq!(vm.get_world().content_digest(), before_world);
    assert_eq!(vm.causality_ledger().settlements.len(), before_settlements);
    assert!(vm.settlement.is_none());

    let valid_chunk = vm
        .load_verified_chunk(balanced_return_chunk("valid-host-call"))
        .expect("balanced chunk should verify");
    let valid = Value::from_fn(
        vm.gc_mut(),
        FnValue {
            name: "valid".to_string(),
            arity: 0,
            chunk_id: valid_chunk,
        },
    );
    vm.call_value(&valid, Vec::new())
        .expect("same host VM must remain reusable");
    assert!(vm.settlement.is_none());
}

pub(crate) fn compile_vm(source: &str) -> VM {
    let mut lexer = Lexer::new(source);
    let (tokens, lex_errors) = lexer.tokenize();
    assert!(lex_errors.is_empty(), "lex errors: {lex_errors:?}");
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(
        parser.errors().is_empty(),
        "parse errors: {:?}",
        parser.errors()
    );
    let mut checker = Checker::new_with_options(CheckerOptions {
        features: vec![FEATURE.to_string()],
        ..CheckerOptions::default()
    });
    let errors = checker.check(&program);
    assert!(errors.is_empty(), "check errors: {errors:?}");
    let result = Compiler::new()
        .with_checker_output(checker.output())
        .with_features(vec![FEATURE.to_string()])
        .compile(&program)
        .expect("compile causal source");
    let mut vm = VM::new_with_seed(7);
    vm.suppress_output();
    vm.load_compile_result(result);
    vm
}

fn compile_vm_with_alias(source: &str, alias: &str, module_source: &str) -> VM {
    let parse = |text: &str| {
        let mut lexer = Lexer::new(text);
        let (tokens, lex_errors) = lexer.tokenize();
        assert!(lex_errors.is_empty(), "lex errors: {lex_errors:?}");
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        program
    };
    let program = parse(source);
    let module = parse(module_source);
    let aliases = std::collections::HashMap::from([(alias.to_string(), module.declarations)]);
    let mut checker = Checker::new_with_options(CheckerOptions {
        features: vec![FEATURE.to_string()],
        ..CheckerOptions::default()
    });
    checker.set_aliases(aliases.clone());
    let errors = checker.check(&program);
    assert!(errors.is_empty(), "check errors: {errors:?}");
    let result = Compiler::new()
        .with_aliases(aliases)
        .with_checker_output(checker.output())
        .with_features(vec![FEATURE.to_string()])
        .compile(&program)
        .expect("compile aliased causal source");
    let mut vm = VM::new();
    vm.suppress_output();
    vm.load_compile_result(result);
    vm
}

#[test]
fn candidate_constraints_commit_or_reject_atomically() {
    let source = r#"
component Position { x: int = 0 }
intent Displacement { key target: entity, amount: int }
law Move(target: entity, amount: int) {
    propose Displacement { target: target, amount: amount }
}
resolver ResolveMovement for Displacement(target, proposals) {
    let position = require(target, Position)
    let amount = proposals |> map(fn(p) { p.amount }) |> sum()
    next(target, Position { x: position.x + amount })
}
constraint WorldBounds for Position(subject, proposed) {
    let previous = base(subject, Position)
    require proposed.x >= previous.x else "position.backwards"
    require proposed.x <= 10 else "position.above_max"
}
entity hero { Position {} }
fn valid() { settle { Move(hero, 4) } }
fn invalid() { settle { Move(hero, 20) } }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize constraints");
    vm.call_global("valid", &[])
        .expect("valid candidate commits");
    let hero = vm
        .get_world()
        .get_entity_by_name("hero")
        .expect("hero entity");
    assert_eq!(
        vm.component_value(hero, "Position").unwrap().unwrap(),
        crate::host_value::FrozenValue::Component {
            type_name: "Position".into(),
            fields: BTreeMap::from([("x".into(), crate::host_value::FrozenValue::Int(4))]),
        }
    );
    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("invalid", &[]).unwrap_err();
    let rejection = match failure {
        crate::constraint_types::VmFailure::SettlementRejected(rejection) => rejection,
        other => panic!("expected typed settlement rejection, got {other}"),
    };
    assert_eq!(rejection.violations.len(), 1);
    assert_eq!(rejection.violations[0].code, "position.above_max");
    assert!(rejection.evaluation_failures.is_empty());
    assert_eq!(before, vm.observable_state_signature());
    let attempt = match vm
        .call_global_attempt("invalid", &[])
        .expect("record rejected attempt")
    {
        crate::constraint_types::SettlementAttemptOutcome::Rejected(attempt) => attempt,
        other => panic!("expected rejected attempt, got {other:?}"),
    };
    assert_eq!(before, vm.observable_state_signature());
    let replayed = vm
        .replay_failed_attempt(&attempt)
        .expect("same base and request reproduce rejection");
    assert_eq!(
        replayed.canonical_bytes(vm.constraint_limit_profile()),
        attempt
            .rejection
            .canonical_bytes(vm.constraint_limit_profile())
    );
    assert_eq!(before, vm.observable_state_signature());
    let mut narrower = crate::sandbox::SandboxCaps::new(HashSet::new(), 100_000, 1 << 20);
    narrower.readable_components.clear();
    vm.sandbox_caps = Some(std::sync::Arc::new(narrower));
    let before_capability_mismatch = vm.observable_state_signature();
    vm.replay_failed_attempt(&attempt)
        .expect("recorded replay retains its original capability checkpoint");
    let mismatch = vm
        .replay_portable_failed_attempt(&attempt.portable_recipe())
        .expect_err("attempt replay must fail closed before executing under different caps");
    assert!(matches!(
        mismatch,
        crate::constraint_types::VmFailure::Host(crate::constraint_types::HostFault {
            ref code,
            ..
        }) if code == "attempt.capability_mismatch"
    ));
    assert_eq!(before_capability_mismatch, vm.observable_state_signature());
    vm.sandbox_caps = None;
    vm.call_global("valid", &[])
        .expect("VM remains reusable after rejection");
}

#[test]
fn candidate_rejection_rendering_is_capability_filtered_and_deterministic() {
    let source = r#"
component SecretPosition { x: int = 0 }
intent Move { key target: entity, amount: int }
law Push(target: entity) { propose Move { target: target, amount: 99 } }
resolver ResolveMove for Move(target, proposals) {
    next(target, SecretPosition { x: proposals[0].amount })
}
constraint HiddenBounds for SecretPosition(subject, proposed) {
    require proposed.x <= 10 else "secret_position.above_max"
}
entity hero { SecretPosition {} }
fn attempt() { settle { Push(hero) } }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize redaction program");
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("typed rejection expected")
    };
    assert!(matches!(
        rejection.candidate_details[&rejection.violations[0].candidate],
        crate::constraint_types::RejectionValue::Visible(_)
    ));
    let capabilities = crate::constraint_types::RejectionCapabilityMetadata {
        profile_id: "reader:public".into(),
        readable_components: std::collections::BTreeSet::new(),
        origins_visible: false,
    };
    let first = rejection.redacted_for(capabilities.clone());
    let second = rejection.redacted_for(capabilities);
    assert!(matches!(
        first.candidate_details[&first.violations[0].candidate],
        crate::constraint_types::RejectionValue::Redacted
    ));
    assert!(first
        .explanation
        .candidates
        .values()
        .all(|candidate| matches!(
            candidate,
            crate::constraint_types::CandidateCausalExplanation::Redacted { .. }
        )));
    assert_eq!(
        first.canonical_bytes(vm.constraint_limit_profile()),
        second.canonical_bytes(vm.constraint_limit_profile())
    );
    let encoded = first
        .canonical_bytes(vm.constraint_limit_profile())
        .expect("redacted rejection encodes");
    let encoded = String::from_utf8(encoded).unwrap();
    assert!(!encoded.contains("Push"));
    assert!(!encoded.contains("ResolveMove"));
    assert!(!encoded.contains("\"Move\""));
    assert!(!encoded.contains("intent_key"));
}

#[test]
fn failed_attempt_replay_rejects_a_different_compiled_program() {
    fn source(limit: i64) -> String {
        format!(
            r#"
component Position {{ x: int = 0 }}
intent Move {{ key target: entity }}
law Push(target: entity) {{ propose Move {{ target: target }} }}
resolver ResolveMove for Move(target, proposals) {{ next(target, Position {{ x: 20 }}) }}
constraint Bounds for Position(subject, proposed) {{
    require proposed.x <= {limit} else "position.too_large"
}}
entity hero {{ Position {{}} }}
fn attempt() {{ settle {{ Push(hero) }} }}
"#
        )
    }

    let mut original = compile_vm(&source(10));
    original.run(0).expect("initialize original program");
    let attempt = match original.call_global_attempt("attempt", &[]).unwrap() {
        crate::constraint_types::SettlementAttemptOutcome::Rejected(attempt) => attempt,
        other => panic!("expected rejected attempt, got {other:?}"),
    };

    let mut changed = compile_vm(&source(11));
    changed.run(0).expect("initialize changed program");
    assert_eq!(
        original.get_world().content_digest(),
        changed.get_world().content_digest()
    );
    let before = changed.observable_state_signature();
    let failure = changed
        .replay_portable_failed_attempt(&attempt.portable_recipe())
        .unwrap_err();
    assert!(matches!(
        failure,
        crate::constraint_types::VmFailure::Host(crate::constraint_types::HostFault {
            ref code,
            ..
        }) if code == "attempt.program_mismatch"
    ));
    assert_eq!(before, changed.observable_state_signature());
}

#[test]
fn constraint_fuel_failure_does_not_suppress_independent_violations() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) {
    next(target, Position { x: 5 })
}
constraint AExhaustsFuel for Position(subject, proposed) {
    let mut count = 0
    while true { count = count + 1 }
}
constraint BStillRuns for Position(subject, proposed) {
    require false else "position.independent_violation"
}
entity hero { Position {} }
fn attempt() { settle { Push(hero) } }
fn ping() { }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize fuel program");
    vm.set_constraint_limit_profile(
        crate::constraint_types::ConstraintLimitProfile::try_new(
            CausalValueLimits::default(),
            64,
            16 * 1024,
            16,
            32,
            32 * 1024,
        )
        .unwrap(),
    );
    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("typed rejection expected")
    };
    assert!(rejection
        .evaluation_failures
        .iter()
        .any(|failure| failure.code == "constraint.fuel_exhausted"));
    assert!(rejection
        .violations
        .iter()
        .any(|violation| violation.code == "position.independent_violation"));
    assert_eq!(before, vm.observable_state_signature());
    vm.call_global("ping", &[])
        .expect("VM remains reusable after constraint evaluation failure");
}

#[test]
fn canonical_rejection_output_limit_is_exact_and_atomic() {
    let source = format!(
        r#"
component Position {{ x: int = 0 }}
intent Move {{ key target: entity, note: str }}
law Push(target: entity) {{
    propose Move {{ target: target, note: "{}" }}
}}
resolver ResolveMove for Move(target, proposals) {{
    next(target, Position {{ x: 20 }})
}}
constraint WorldBounds for Position(subject, proposed) {{
    require proposed.x <= 10 else "position.above_max"
}}
entity hero {{ Position {{}} }}
fn attempt() {{ settle {{ Push(hero) }} }}
"#,
        // Keep retained semantic data under the pre-retention outcome meter,
        // while JSON keys/escaping still push the exact canonical envelope
        // over the configured wire limit. This specifically exercises the
        // bounded writer as the independent final backstop.
        "payload".repeat(30)
    );
    let mut vm = compile_vm(&source);
    vm.run(0).expect("initialize output-limit program");
    vm.set_constraint_limit_profile(
        crate::constraint_types::ConstraintLimitProfile::try_new(
            CausalValueLimits::default(),
            100_000,
            1024 * 1024,
            16,
            32,
            1024,
        )
        .unwrap(),
    );
    let before = vm.observable_state_signature();
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("typed rejection expected")
    };
    assert_eq!(
        rejection.evaluation_failures[0].code,
        "constraint.outcome_byte_limit"
    );
    assert!(
        rejection
            .canonical_bytes(vm.constraint_limit_profile())
            .expect("bounded rejection encoding")
            .len()
            <= 1024,
        "bounded fallback itself must fit the configured exact byte cap"
    );
    assert_eq!(before, vm.observable_state_signature());
}

#[test]
fn constraint_heap_limit_becomes_evaluation_failure_and_restores_budget() {
    let source = r#"
component Position { x: int = 0 }
intent Move { key target: entity }
law Push(target: entity) { propose Move { target: target } }
resolver ResolveMove for Move(target, proposals) { next(target, Position { x: 1 }) }
constraint AllocatesTooMuch for Position(subject, proposed) {
    let count = proposed.x + 99999
    let large = "x" * count
    let length = len(large)
}
entity hero { Position {} }
fn attempt() { settle { Push(hero) } }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize memory-limit program");
    vm.set_constraint_limit_profile(
        crate::constraint_types::ConstraintLimitProfile::try_new(
            CausalValueLimits::default(),
            100_000,
            1024,
            16,
            32,
            32 * 1024,
        )
        .unwrap(),
    );
    let original_mem_limit = vm.mem_limit;
    let failure = vm.call_global_detailed("attempt", &[]).unwrap_err();
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("typed rejection expected")
    };
    assert!(rejection
        .evaluation_failures
        .iter()
        .any(|failure| failure.code == "constraint.memory_exhausted"));
    assert_eq!(vm.mem_limit, original_mem_limit);
    assert!(vm.settlement.is_none());
}

#[test]
fn watched_candidate_component_triggers_once_with_complete_patch_reads() {
    let source = r#"
component Position { x: int = 0 }
component Velocity { x: int = 0 }
intent Motion { key target: entity, position: int, velocity: int }
law Move(target: entity, position: int, velocity: int) {
    propose Motion { target: target, position: position, velocity: velocity }
}
resolver ResolveMotion for Motion(target, proposals) {
    let proposal = proposals[0]
    next(target, Position { x: proposal.position })
    next(target, Velocity { x: proposal.velocity })
}
constraint ValidMotion for Position(subject, proposed) watches Velocity {
    let velocity = candidate(subject, Velocity)
    require proposed.x == velocity.x else "motion.mismatch"
}
entity hero { Position {}, Velocity {} }
fn valid() { settle { Move(hero, 7, 7) } }
fn invalid() { settle { Move(hero, 8, 9) } }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize watched constraint");
    vm.call_global("valid", &[])
        .expect("constraint sees all resolver writes");
    let before = vm.observable_state_signature();
    let error = vm.call_global("invalid", &[]).unwrap_err();
    assert!(error.contains("motion.mismatch"), "{error}");
    assert_eq!(before, vm.observable_state_signature());
}

#[test]
fn movement_constraint_dogfood_runs_both_commit_and_rejection_paths() {
    let mut accepted = compile_vm(include_str!(
        "../../../../projects/dogfood/causal-constraints/main.rad"
    ));
    accepted.run(0).expect("movement dogfood must commit");
    let hero = accepted.get_world().get_entity_by_name("hero").unwrap();
    let position = accepted.component_value(hero, "Position").unwrap().unwrap();
    let crate::host_value::FrozenValue::Component { fields, .. } = position else {
        panic!("Position component expected")
    };
    assert_eq!(fields["x"], crate::host_value::FrozenValue::Int(12));

    let mut rejected = compile_vm(include_str!(
        "../../../../projects/dogfood/causal-constraints/rejected.rad"
    ));
    let failure = rejected
        .run_detailed(0)
        .expect_err("solid overlap must reject");
    let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
        panic!("typed settlement rejection expected")
    };
    assert_eq!(rejection.violations[0].code, "position.inside_solid");
    let hero = rejected.get_world().get_entity_by_name("hero").unwrap();
    let position = rejected.component_value(hero, "Position").unwrap().unwrap();
    let crate::host_value::FrozenValue::Component { fields, .. } = position else {
        panic!("Position component expected")
    };
    assert_eq!(fields["x"], crate::host_value::FrozenValue::Int(10));
    assert!(rejected.causality_ledger().settlements.is_empty());
}

#[test]
fn movement_constraints_are_canonical_across_producer_and_declaration_order() {
    let declarations = r#"
component Position { x: int = 0 }
component Velocity { dx: int = 0 }
intent Displacement { key target: entity, source: str, amount: int }
law Inertia(target: entity) {
    propose Displacement { target: target, source: "velocity", amount: require(target, Velocity).dx }
}

law Wind(target: entity) {
    propose Displacement { target: target, source: "wind", amount: 2 }
}
law Knockback(target: entity) {
    propose Displacement { target: target, source: "knockback", amount: 0 }
}
resolver ResolveDisplacement for Displacement(target, proposals) {
    let old = require(target, Position)
    let total = proposals |> map(fn(proposal) { proposal.amount }) |> sum()
    next(target, Position { x: old.x + total })
}
"#;
    let constraints = [
        r#"
constraint WorldBounds for Position(subject, proposed) {
    require proposed.x <= 100 else "position.above_world_max"
}
constraint NonPenetration for Position(subject, proposed) {
    require proposed.x != 13 else "position.inside_solid"
}
"#,
        r#"
constraint NonPenetration for Position(subject, proposed) {
    require proposed.x != 13 else "position.inside_solid"
}
constraint WorldBounds for Position(subject, proposed) {
    require proposed.x <= 100 else "position.above_world_max"
}
"#,
    ];
    let calls = ["Inertia(hero)", "Wind(hero)", "Knockback(hero)"];
    let permutations = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut canonical = None;
    for constraint_order in constraints {
        for order in permutations {
            let body = order
                .into_iter()
                .map(|index| calls[index])
                .collect::<Vec<_>>()
                .join("\n");
            let source = format!(
                "{declarations}\n{constraint_order}\nentity hero {{ Position {{ x: 10 }}, Velocity {{ dx: 1 }} }}\nsettle {{\n{body}\n}}"
            );
            let mut vm = compile_vm(&source);
            let failure = vm.run_detailed(0).expect_err("x = 13 must reject");
            let crate::constraint_types::VmFailure::SettlementRejected(rejection) = failure else {
                panic!("typed rejection expected")
            };
            let bytes = rejection.canonical_bytes(vm.constraint_limit_profile());
            if let Some(expected) = &canonical {
                assert_eq!(&bytes, expected);
            } else {
                canonical = Some(bytes);
            }
        }
    }
}

#[test]
fn fuzz_candidate_constraint_producer_order_is_semantically_invisible() {
    let source = r#"
component Position { x: int = 0 }
intent Displacement { key target: entity, amount: int }
law Push(target: entity, amount: int) {
    propose Displacement { target: target, amount: amount }
}
resolver ResolveDisplacement for Displacement(target, proposals) {
    let total = proposals |> map(fn(p) { return p.amount }) |> sum()
    next(target, Position { x: total })
}
constraint UpperBound for Position(subject, proposed) {
    require proposed.x <= 10 else "position.above_max"
}
constraint EvenForbidden for Position(subject, proposed) {
    require proposed.x % 2 == 1 else "position.even_forbidden"
}
entity hero { Position {} }
fn attempt(reverse: bool, first: int, second: int) {
    settle {
        if reverse {
            Push(hero, second)
            Push(hero, first)
        } else {
            Push(hero, first)
            Push(hero, second)
        }
    }
}
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize randomized constraint program");
    let cases = std::env::var("RAD_FUZZ_ITERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(120)
        .clamp(16, 512);
    let mut state = 0xC0FF_EE12_3456_7890_u64;

    for _ in 0..cases {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let first = 20 + (state % 10_000) as i64;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let second = 20 + (state % 10_000) as i64;
        let args = [
            FrozenValue::Bool(false),
            FrozenValue::Int(first),
            FrozenValue::Int(second),
        ];
        let left = vm.call_global_detailed("attempt", &args).unwrap_err();
        let args = [
            FrozenValue::Bool(true),
            FrozenValue::Int(first),
            FrozenValue::Int(second),
        ];
        let right = vm.call_global_detailed("attempt", &args).unwrap_err();
        let (
            crate::constraint_types::VmFailure::SettlementRejected(left),
            crate::constraint_types::VmFailure::SettlementRejected(right),
        ) = (left, right)
        else {
            panic!("both permutations must reject through the typed boundary")
        };
        assert_eq!(
            left.canonical_bytes(vm.constraint_limit_profile()),
            right.canonical_bytes(vm.constraint_limit_profile())
        );
    }
}
