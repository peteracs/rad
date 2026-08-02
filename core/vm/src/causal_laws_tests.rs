//! Executable acceptance tests for RFC-0001's vertical slice.

use crate::causality::CausalityLedger;
use crate::checker::{Checker, CheckerOptions};
use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::replay::TraceReplayer;
use crate::sandbox::SandboxCaps;
use crate::vm::VM;
use std::collections::HashSet;
use std::sync::Arc;

const FEATURE: &str = "causal_laws";

fn compile_vm(source: &str) -> VM {
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
    let mut vm = VM::new();
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

fn damage_source(calls: &[&str]) -> String {
    format!(
        r#"
component Health {{ hp: int = 100, max: int = 100 }}
component Shield {{ hp: int = 10 }}
intent Damage {{
    key target: entity
    source: entity
    amount: int
    kind: str
}}
law DirectHit(source: entity, target: entity, amount: int, kind: str) {{
    propose Damage {{ target: target, source: source, amount: amount, kind: kind }}
}}
resolver ResolveDamage for Damage(target, proposals) {{
    let health = require(target, Health)
    let shield = require(target, Shield)
    let raw = proposals |> map(fn(p) {{ return p.amount }}) |> sum()
    let absorbed = min(shield.hp, raw)
    next(target, Shield {{ hp: shield.hp - absorbed }})
    next(target, Health {{ hp: max(0, health.hp - (raw - absorbed)), max: health.max }})
}}
entity attacker_a {{}}
entity attacker_b {{}}
entity environment {{}}
entity hero {{ Health {{}}, Shield {{}} }}
settle {{
{}
}}
"#,
        calls.join("\n")
    )
}

#[test]
fn all_damage_producer_permutations_have_identical_worlds_and_fan_in() {
    let calls = [
        "DirectHit(attacker_a, hero, 20, \"physical\")",
        "DirectHit(attacker_b, hero, 30, \"fire\")",
        "DirectHit(environment, hero, 5, \"burn\")",
    ];
    let permutations = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut expected_digest = None;
    let mut expected_explanation_shape = None;
    for order in permutations {
        let ordered = order.map(|index| calls[index]);
        let mut vm = compile_vm(&damage_source(&ordered));
        vm.run(0).expect("settlement succeeds");
        let digest = vm.get_world().content_digest();
        assert_eq!(
            vm.get_world().get_component(3, "Health").unwrap().values[0].as_int(),
            Some(55)
        );
        assert_eq!(
            vm.get_world().get_component(3, "Shield").unwrap().values[0].as_int(),
            Some(0)
        );
        let why = vm
            .causality_ledger()
            .explain_named("hero", "Health", u64::MAX);
        assert!(why.contains("resolver `ResolveDamage`"));
        assert_eq!(why.matches("proposal Damage").count(), 3);
        let shape = why
            .lines()
            .filter(|line| line.contains("proposal Damage"))
            .map(|line| line.trim().to_string())
            .collect::<Vec<_>>();
        if let Some(expected) = &expected_digest {
            assert_eq!(&digest, expected);
            assert_eq!(Some(&shape), expected_explanation_shape.as_ref());
        } else {
            expected_digest = Some(digest);
            expected_explanation_shape = Some(shape);
        }
    }
}

#[test]
fn conflicting_resolvers_abort_world_and_provenance_atomically() {
    let declarations = r#"
component Health { hp: int = 100 }
intent Damage { key target: entity, amount: int }
intent Healing { key target: entity, amount: int }
law Hit(target: entity) { propose Damage { target: target, amount: 10 } }
law Heal(target: entity) { propose Healing { target: target, amount: 10 } }
resolver ResolveDamage for Damage(target, proposals) {
    next(target, Health { hp: require(target, Health).hp - 10 })
}
resolver ResolveHealing for Healing(target, proposals) {
    next(target, Health { hp: require(target, Health).hp + 10 })
}
entity hero { Health {} }
"#;
    let mut baseline = compile_vm(declarations);
    baseline.run(0).expect("baseline setup");
    let expected_digest = baseline.get_world().content_digest();

    let mut conflicting = compile_vm(&format!(
        "{}\nsettle {{ Hit(hero) Heal(hero) }}\n",
        declarations
    ));
    let error = conflicting.run(0).expect_err("conflict must abort");
    assert!(error.contains("conflicting candidate writes"), "{error}");
    assert_eq!(conflicting.get_world().content_digest(), expected_digest);
    assert!(conflicting.causality_ledger().settlements.is_empty());
    assert!(conflicting.causality_ledger().resolutions.is_empty());
}

#[test]
fn event_origin_and_replay_reconstruct_the_same_causal_tree() {
    let source = r#"
component Health { hp: int = 100 }
intent Damage { key target: entity, source: entity, amount: int }
law Hit(source: entity, target: entity, amount: int) {
    propose Damage { target: target, source: source, amount: amount }
}
resolver ResolveDamage for Damage(target, proposals) {
    let health = require(target, Health)
    let total = proposals |> map(fn(p) { return p.amount }) |> sum()
    next(target, Health { hp: health.hp - total })
}
event CombatFrame { source: entity, target: entity }
on CombatFrame(e) { settle { Hit(e.source, e.target, 20) } }
entity attacker {}
entity hero { Health {} }
emit CombatFrame { source: attacker, target: hero }
flush_events()
"#;
    let mut recorded = compile_vm(source);
    recorded.enable_recording(source);
    recorded.run(0).expect("recorded settlement");
    let digest = recorded.get_world().content_digest();
    let why = recorded
        .causality_ledger()
        .explain_named("hero", "Health", u64::MAX);
    assert!(why.contains("proposal Damage"));
    assert!(why.contains("law `Hit`"));
    assert!(why.contains("`on CombatFrame` handler"));
    let trace = recorded.take_trace().expect("recorded trace");

    let replayer = TraceReplayer::parse(&trace, false).expect("parse trace");
    let mut replayed = compile_vm(replayer.source());
    replayed.enable_replay(replayer);
    replayed.run(0).expect("replayed settlement");
    assert_eq!(replayed.get_world().content_digest(), digest);
    assert_eq!(
        replayed
            .causality_ledger()
            .explain_named("hero", "Health", u64::MAX),
        why
    );
    let report = replayed.finish_replay().expect("report");
    assert_eq!(report.end_digest_match, Some(true));
    assert_eq!(report.leftover_io, 0);
}

#[test]
fn sandbox_acl_denies_next_without_changing_the_world() {
    let source = r#"
component Health { hp: int = 100 }
intent Damage { key target: entity, amount: int }
law Hit(target: entity) { propose Damage { target: target, amount: 10 } }
resolver ResolveDamage for Damage(target, proposals) {
    next(target, Health { hp: require(target, Health).hp - 10 })
}
entity hero { Health {} }
fn attack() { settle { Hit(hero) } }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("trusted setup");
    let before = vm.get_world().content_digest();
    vm.sandbox_caps = Some(Arc::new(SandboxCaps::new(
        HashSet::new(),
        u64::MAX,
        usize::MAX,
    )));
    let slot = vm
        .global_names
        .iter()
        .position(|name| name == "attack")
        .expect("attack global");
    let attack = vm.globals[slot];
    let error = vm
        .call_value(&attack, Vec::new())
        .expect_err("ACL must reject next");
    assert!(
        error.contains("write to component 'Health' denied"),
        "{error}"
    );
    assert_eq!(vm.get_world().content_digest(), before);
    assert!(vm.causality_ledger().settlements.is_empty());
}

#[test]
fn wire_provenance_preserves_settlement_fan_in() {
    let source = r#"
component Health { hp: int = 100 }
intent Damage { key target: entity, amount: int }
law Hit(target: entity, amount: int) {
    propose Damage { target: target, amount: amount }
}
resolver ResolveDamage for Damage(target, proposals) {
    let health = require(target, Health)
    let total = proposals |> map(fn(p) { return p.amount }) |> sum()
    next(target, Health { hp: health.hp - total })
}
event CombatFrame { target: entity }
on CombatFrame(e) { settle { Hit(e.target, 20) Hit(e.target, 5) } }
entity hero { Health {} }
emit CombatFrame { target: hero }
flush_events()
"#;
    let mut sender = compile_vm(source);
    sender.run(0).expect("sender settlement");
    let hero = sender.get_world().get_entity_by_name("hero").unwrap();
    let closure = sender.causality_ledger().provenance_closure(|_| true, &[]);
    let mut encoded = String::new();
    crate::wire::encode_prov_into(&closure, &mut encoded);
    let json = serde_json::from_str(&encoded).expect("encoded provenance JSON");
    let mut decoded = crate::wire::decode_prov(&json).expect("decode provenance");
    decoded.origin = "test-wire".to_string();

    let mut receiver = CausalityLedger::default();
    receiver.ingest(&decoded, &std::collections::HashMap::new());
    let why = receiver.explain_entity(hero, "Health", u64::MAX);
    assert!(why.contains("resolver `ResolveDamage`"), "{why}");
    assert_eq!(why.matches("proposal Damage").count(), 2, "{why}");
    assert!(why.contains("`on CombatFrame` handler"), "{why}");
}

#[test]
fn public_law_keeps_its_private_intent_and_resolver_in_aliased_module() {
    let module = r#"
intent Ping { key target: entity, amount: int }
pub law SendPing(target: entity) {
    propose Ping { target: target, amount: 1 }
}
resolver ResolvePing for Ping(target, proposals) {}
"#;
    let source = r#"
entity hero {}
settle { combat.SendPing(hero) }
"#;
    let mut vm = compile_vm_with_alias(source, "combat", module);
    vm.run(0).expect("aliased settlement");
    assert_eq!(vm.causality_ledger().settlements.len(), 1);
    assert_eq!(vm.causality_ledger().proposals.len(), 1);
    assert_eq!(vm.causality_ledger().resolutions.len(), 1);
}

#[test]
fn resolvers_cannot_observe_each_others_candidate_writes() {
    let source = r#"
component Health { hp: int = 100 }
component Shield { hp: int = 10 }
intent DrainShield { key target: entity }
intent ObserveShield { key target: entity }
law Drain(target: entity) { propose DrainShield { target: target } }
law Observe(target: entity) { propose ObserveShield { target: target } }
resolver ResolveDrain for DrainShield(target, proposals) {
    next(target, Shield { hp: 0 })
}
resolver ResolveObservation for ObserveShield(target, proposals) {
    let health = require(target, Health)
    let shield = require(target, Shield)
    next(target, Health { hp: health.hp - shield.hp })
}
entity hero { Health {}, Shield {} }
settle { Drain(hero) Observe(hero) }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("isolated resolution patches");
    assert_eq!(
        vm.get_world().get_component(0, "Health").unwrap().values[0].as_int(),
        Some(90)
    );
    assert_eq!(
        vm.get_world().get_component(0, "Shield").unwrap().values[0].as_int(),
        Some(0)
    );
}
