//! Executable acceptance tests for RFC-0001's vertical slice.

use crate::causality::CausalityLedger;
use crate::checker::{Checker, CheckerOptions};
use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::opcode::{Chunk, Op};
use crate::parser::Parser;
use crate::replay::TraceReplayer;
use crate::sandbox::SandboxCaps;
use crate::settlement_reference::{
    settle_reference, ReferenceComponent, ReferenceProposal, ReferenceResolver, ReferenceValue,
    ReferenceWorld, ReferenceWrite,
};
use crate::value::{FnValue, Value};
use crate::vm::VM;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

const FEATURE: &str = "causal_laws";

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

fn generated_damage_source(calls: &[String]) -> String {
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
entity hero_a {{ Health {{}}, Shield {{}} }}
entity hero_b {{ Health {{}}, Shield {{}} }}
settle {{
{}
}}
"#,
        calls.join("\n")
    )
}

fn reference_int(component: &ReferenceComponent, field: &str) -> Result<i64, String> {
    match component.get(field) {
        Some(ReferenceValue::Int(value)) => Ok(*value),
        _ => Err(format!("missing integer field {field}")),
    }
}

fn reference_damage_resolver(
    key: u32,
    proposals: &[ReferenceProposal],
    base: &ReferenceWorld,
) -> Result<Vec<ReferenceWrite>, String> {
    let health = base
        .component(key, "Health")
        .ok_or_else(|| "missing Health".to_string())?;
    let shield = base
        .component(key, "Shield")
        .ok_or_else(|| "missing Shield".to_string())?;
    let raw = proposals.iter().try_fold(0i64, |total, proposal| {
        reference_int(&proposal.payload, "amount").map(|amount| total + amount)
    })?;
    let shield_hp = reference_int(shield, "hp")?;
    let absorbed = shield_hp.min(raw);
    let health_hp = reference_int(health, "hp")?;
    let health_max = reference_int(health, "max")?;
    Ok(vec![
        ReferenceWrite {
            entity: key,
            component: "Shield".to_string(),
            value: BTreeMap::from([("hp".to_string(), ReferenceValue::Int(shield_hp - absorbed))]),
        },
        ReferenceWrite {
            entity: key,
            component: "Health".to_string(),
            value: BTreeMap::from([
                (
                    "hp".to_string(),
                    ReferenceValue::Int((health_hp - (raw - absorbed)).max(0)),
                ),
                ("max".to_string(), ReferenceValue::Int(health_max)),
            ]),
        },
    ])
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
fn generated_proposal_multisets_match_the_pure_reference_model() {
    let mut state = 0xCA55_1A57_5E77_1E55u64;
    let mut random = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let component = |hp, max| {
        BTreeMap::from([
            ("hp".to_string(), ReferenceValue::Int(hp)),
            ("max".to_string(), ReferenceValue::Int(max)),
        ])
    };
    let base = ReferenceWorld {
        components: BTreeMap::from([
            ((3, "Health".to_string()), component(100, 100)),
            (
                (3, "Shield".to_string()),
                BTreeMap::from([("hp".to_string(), ReferenceValue::Int(10))]),
            ),
            ((4, "Health".to_string()), component(100, 100)),
            (
                (4, "Shield".to_string()),
                BTreeMap::from([("hp".to_string(), ReferenceValue::Int(10))]),
            ),
        ]),
    };
    let resolvers = BTreeMap::from([(
        "Damage".to_string(),
        ReferenceResolver {
            name: "ResolveDamage",
            resolve: reference_damage_resolver,
        },
    )]);
    let sources = [("attacker_a", 0u32), ("attacker_b", 1), ("environment", 2)];
    let targets = [("hero_a", 3u32), ("hero_b", 4)];
    let kinds = ["physical", "fire", "burn"];

    for case in 0..24usize {
        let count = 2 + (random() as usize % 15);
        let mut calls = Vec::with_capacity(count);
        let mut proposals = Vec::with_capacity(count);
        for index in 0..count {
            let source_index = if index < 3 {
                index
            } else {
                random() as usize % sources.len()
            };
            let target_index = if index < 2 {
                index
            } else {
                random() as usize % targets.len()
            };
            // Include duplicates and occasional large values without risking
            // arithmetic overflow in either implementation.
            let amount = if index % 7 == 0 {
                10_000
            } else {
                (random() % 41) as i64
            };
            let kind = kinds[source_index];
            calls.push(format!(
                "DirectHit({}, {}, {}, \"{}\")",
                sources[source_index].0, targets[target_index].0, amount, kind
            ));
            proposals.push(ReferenceProposal {
                intent: "Damage".to_string(),
                key: targets[target_index].1,
                payload: BTreeMap::from([
                    (
                        "target".to_string(),
                        ReferenceValue::Entity(targets[target_index].1),
                    ),
                    (
                        "source".to_string(),
                        ReferenceValue::Entity(sources[source_index].1),
                    ),
                    ("amount".to_string(), ReferenceValue::Int(amount)),
                    ("kind".to_string(), ReferenceValue::Text(kind.to_string())),
                ]),
                canonical: format!(
                    "target={};source={};amount={amount};kind={kind}",
                    targets[target_index].1, sources[source_index].1
                ),
                producer: "DirectHit".to_string(),
                source_line: index as u32,
            });
        }

        let reference = settle_reference(&base, proposals, &resolvers)
            .unwrap_or_else(|error| panic!("reference case {case} failed: {error:?}"));
        let mut orders = vec![calls.clone()];
        let mut reversed = calls.clone();
        reversed.reverse();
        orders.push(reversed);
        let mut rotated = calls.clone();
        let rotate_by = case % rotated.len();
        rotated.rotate_left(rotate_by);
        orders.push(rotated);

        let mut expected_digest = None;
        let mut expected_why = None;
        for order in orders {
            let mut vm = compile_vm(&generated_damage_source(&order));
            vm.run(0)
                .unwrap_or_else(|error| panic!("VM case {case} failed: {error}"));
            let digest = vm.get_world().content_digest();
            let why_a = vm
                .causality_ledger()
                .explain_named("hero_a", "Health", u64::MAX);
            let why_b = vm
                .causality_ledger()
                .explain_named("hero_b", "Health", u64::MAX);
            let why = format!("{why_a}\n---\n{why_b}");
            if let Some(expected) = &expected_digest {
                assert_eq!(&digest, expected, "order changed digest in case {case}");
                assert_eq!(Some(&why), expected_why.as_ref(), "case {case}");
            } else {
                expected_digest = Some(digest);
                expected_why = Some(why);
            }

            for (name, entity) in targets {
                for component_name in ["Health", "Shield"] {
                    let actual = vm
                        .get_world()
                        .get_component(entity, component_name)
                        .unwrap_or_else(|| panic!("missing VM {component_name} for {name}"));
                    let expected = reference
                        .world
                        .component(entity, component_name)
                        .unwrap_or_else(|| panic!("missing reference {component_name} for {name}"));
                    for (field, value) in expected {
                        let ReferenceValue::Int(expected_int) = value else {
                            continue;
                        };
                        let position = actual
                            .layout
                            .iter()
                            .position(|actual_field| actual_field == field)
                            .unwrap();
                        assert_eq!(
                            actual.values[position].as_int(),
                            Some(*expected_int),
                            "case {case}: {name}.{component_name}.{field}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn order_sensitive_resolver_matches_reference_canonical_payload_order() {
    fn choose_first(
        key: u32,
        proposals: &[ReferenceProposal],
        _base: &ReferenceWorld,
    ) -> Result<Vec<ReferenceWrite>, String> {
        let rank = reference_int(&proposals[0].payload, "rank")?;
        Ok(vec![ReferenceWrite {
            entity: key,
            component: "Health".to_string(),
            value: BTreeMap::from([("hp".to_string(), ReferenceValue::Int(rank))]),
        }])
    }

    let declarations = r#"
component Health { hp: int = 0 }
intent Choice { key target: entity, label: str, rank: int }
law Offer(target: entity, label: str, rank: int) {
    propose Choice { target: target, label: label, rank: rank }
}
resolver ResolveChoice for Choice(target, proposals) {
    let chosen = proposals[0]
    next(target, Health { hp: chosen.rank })
}
entity hero { Health {} }
"#;
    let calls = [
        "Offer(hero, \"z\", 2)",
        "Offer(hero, \"a\", 10)",
        "Offer(hero, \"m\", 100)",
    ];
    let reference = settle_reference(
        &ReferenceWorld {
            components: BTreeMap::from([(
                (0, "Health".to_string()),
                BTreeMap::from([("hp".to_string(), ReferenceValue::Int(0))]),
            )]),
        },
        [("z", 2), ("a", 10), ("m", 100)]
            .into_iter()
            .map(|(label, rank)| ReferenceProposal {
                intent: "Choice".to_string(),
                key: 0,
                payload: BTreeMap::from([
                    ("label".to_string(), ReferenceValue::Text(label.to_string())),
                    ("rank".to_string(), ReferenceValue::Int(rank)),
                ]),
                // For a fixed key, the production typed encoding reaches the
                // label before rank, so this is its semantic sort prefix.
                canonical: label.to_string(),
                producer: "Offer".to_string(),
                source_line: 1,
            })
            .collect(),
        &BTreeMap::from([(
            "Choice".to_string(),
            ReferenceResolver {
                name: "ResolveChoice",
                resolve: choose_first,
            },
        )]),
    )
    .expect("reference choice settlement");
    let expected_hp = match reference.world.component(0, "Health").unwrap()["hp"] {
        ReferenceValue::Int(value) => value,
        ref value => panic!("unexpected reference value {value:?}"),
    };
    assert_eq!(expected_hp, 10);

    let permutations = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut expected_why = None;
    for order in permutations {
        let body = order
            .into_iter()
            .map(|index| calls[index])
            .collect::<Vec<_>>()
            .join("\n");
        let mut vm = compile_vm(&format!("{declarations}\nsettle {{\n{body}\n}}"));
        vm.run(0).expect("choice settlement");
        assert_eq!(
            vm.get_world().get_component(0, "Health").unwrap().values[0].as_int(),
            Some(expected_hp)
        );
        let why = vm
            .causality_ledger()
            .explain_named("hero", "Health", u64::MAX);
        if let Some(expected) = &expected_why {
            assert_eq!(&why, expected);
        } else {
            expected_why = Some(why);
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
    assert!(vm.settlement.is_none());
}

#[test]
fn failed_law_unwinds_settlement_and_reused_vm_can_settle_again() {
    let source = r#"
component Health { hp: int = 100 }
component RequiredState { value: int = 0 }
intent Damage { key target: entity, amount: int }
law FailingHit(target: entity) {
    let unavailable = require(target, RequiredState)
    propose Damage { target: target, amount: unavailable.value }
}
law ValidHit(target: entity) {
    propose Damage { target: target, amount: 10 }
}
resolver ResolveDamage for Damage(target, proposals) {
    let health = require(target, Health)
    let total = proposals |> map(fn(p) { return p.amount }) |> sum()
    next(target, Health { hp: health.hp - total })
}
entity hero { Health {} }
fn fail() { settle { FailingHit(hero) } }
fn succeed() { settle { ValidHit(hero) } }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize reusable VM");
    let before_digest = vm.get_world().content_digest();
    let before_writes = vm.causality_ledger().writes.len();
    let before_settlements = vm.causality_ledger().settlements.len();

    let global = |vm: &VM, name: &str| {
        let slot = vm
            .global_names
            .iter()
            .position(|global| global == name)
            .unwrap_or_else(|| panic!("missing global {name}"));
        vm.globals[slot]
    };
    let fail = global(&vm, "fail");
    let error = vm
        .call_value(&fail, Vec::new())
        .expect_err("missing component must fail inside the law phase");
    assert!(error.contains("RequiredState"), "{error}");
    assert_eq!(vm.get_world().content_digest(), before_digest);
    assert_eq!(vm.causality_ledger().writes.len(), before_writes);
    assert_eq!(vm.causality_ledger().settlements.len(), before_settlements);
    assert!(
        vm.settlement.is_none(),
        "failed host call leaked settlement"
    );

    let succeed = global(&vm, "succeed");
    vm.call_value(&succeed, Vec::new())
        .expect("same VM must accept a later valid settlement");
    assert!(vm.settlement.is_none());
    assert_eq!(
        vm.get_world().get_component(0, "Health").unwrap().values[0].as_int(),
        Some(90)
    );
}

#[test]
fn failed_top_level_settlement_leaves_no_active_transaction() {
    let source = r#"
component Health { hp: int = 100 }
component RequiredState { value: int = 0 }
intent Damage { key target: entity, amount: int }
law FailingHit(target: entity) {
    let unavailable = require(target, RequiredState)
    propose Damage { target: target, amount: unavailable.value }
}
resolver ResolveDamage for Damage(target, proposals) {}
entity hero { Health {} }
settle { FailingHit(hero) }
"#;
    let mut vm = compile_vm(source);
    let error = vm.run(0).expect_err("top-level law must fail");
    assert!(error.contains("RequiredState"), "{error}");
    assert!(vm.settlement.is_none());
    assert!(vm.causality_ledger().settlements.is_empty());
}

#[test]
fn failed_event_handler_settlement_unwinds_at_the_host_call_boundary() {
    let source = r#"
component Health { hp: int = 100 }
component RequiredState { value: int = 0 }
intent Damage { key target: entity, amount: int }
readonly fn missing_amount(target: entity) -> int {
    return require(target, RequiredState).value
}
law FailingHit(target: entity) {
    propose Damage { target: target, amount: missing_amount(target) }
}
law ValidHit(target: entity) {
    propose Damage { target: target, amount: 10 }
}
resolver ResolveDamage for Damage(target, proposals) {
    let health = require(target, Health)
    let total = proposals |> map(fn(p) { return p.amount }) |> sum()
    next(target, Health { hp: health.hp - total })
}
event Trigger { target: entity }
on Trigger(e) { settle { FailingHit(e.target) } }
entity hero { Health {} }
event fn trigger_failure() { emit Trigger { target: hero } flush_events() }
fn succeed() { settle { ValidHit(hero) } }
"#;
    let mut vm = compile_vm(source);
    vm.run(0).expect("initialize event boundary VM");
    let before_digest = vm.get_world().content_digest();
    let before_settlements = vm.causality_ledger().settlements.len();
    let global = |vm: &VM, name: &str| {
        let slot = vm
            .global_names
            .iter()
            .position(|global| global == name)
            .unwrap();
        vm.globals[slot]
    };

    let trigger = global(&vm, "trigger_failure");
    let error = vm
        .call_value(&trigger, Vec::new())
        .expect_err("event handler law must fail");
    assert!(error.contains("RequiredState"), "{error}");
    assert_eq!(vm.get_world().content_digest(), before_digest);
    assert_eq!(vm.causality_ledger().settlements.len(), before_settlements);
    assert!(vm.settlement.is_none());

    let succeed = global(&vm, "succeed");
    vm.call_value(&succeed, Vec::new())
        .expect("same VM must remain reusable after handler failure");
    assert!(vm.settlement.is_none());
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

#[test]
fn provenance_fan_in_wire_growth_is_linear_and_default_rendering_is_bounded() {
    let mut previous_wire_bytes = 0usize;
    for count in [1usize, 10, 100, 1_000, 10_000] {
        let source = format!(
            r#"
component Health {{ hp: int = 1000000 }}
intent Damage {{ key target: entity, amount: int, sequence: int }}
law Hit(target: entity, sequence: int) {{
    propose Damage {{ target: target, amount: 1, sequence: sequence }}
}}
resolver ResolveDamage for Damage(target, proposals) {{
    let health = require(target, Health)
    next(target, Health {{ hp: health.hp - len(proposals) }})
}}
entity hero {{ Health {{}} }}
settle {{ for sequence in range(0, {count}) {{ Hit(hero, sequence) }} }}
"#
        );
        let mut vm = compile_vm(&source);
        vm.run(0).expect("fan-in baseline settlement");
        let closure = vm.causality_ledger().provenance_closure(|_| true, &[]);
        let mut wire = String::new();
        crate::wire::encode_prov_into(&closure, &mut wire);
        let why = vm
            .causality_ledger()
            .explain_named("hero", "Health", u64::MAX);
        assert!(wire.len() > previous_wire_bytes, "count {count}");
        assert!(why.matches("proposal Damage").count() <= 8, "{why}");
        if count > 8 {
            assert!(why.contains("additional proposals omitted"), "{why}");
        }
        previous_wire_bytes = wire.len();
        eprintln!(
            "causal fan-in {count:>5}: wire={:>9} bytes, rendered why={:>5} bytes",
            wire.len(),
            why.len()
        );
    }
}
