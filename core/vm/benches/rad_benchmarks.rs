use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

use rad_vm::checker::{Checker, CheckerOptions};
use rad_vm::compiler::Compiler;
use rad_vm::host_value::FrozenValue;
use rad_vm::lexer::Lexer;
use rad_vm::module_loader::load_program_with_uses;
use rad_vm::parser::Parser;
use rad_vm::settlement_reference::{
    settle_reference, ReferenceProposal, ReferenceResolver, ReferenceValue, ReferenceWorld,
    ReferenceWrite,
};
use rad_vm::vm::VM;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
}

fn read_example(name: &str) -> String {
    let path = examples_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e))
}

fn full_pipeline(filepath: &str) -> Result<(), String> {
    let (program, _source, _had_imports) =
        load_program_with_uses(filepath).map_err(|e| e[0].message.clone())?;
    let mut checker = Checker::new();
    let errors = checker.check(&program);
    if !errors.is_empty() {
        return Err(errors[0].message.clone());
    }
    let for_iter_hints = checker.for_iter_kinds();
    let compiler = Compiler::new().with_for_iter_kinds(for_iter_hints);
    let compile_result = compiler.compile(&program).map_err(|e| e.message)?;
    let mut vm = VM::new();
    vm.suppress_output();
    vm.load_compile_result(compile_result);
    vm.run(0).map_err(|e| e.to_string())
}

fn bench_lexer(c: &mut Criterion) {
    let source = read_example("demo.rad");
    c.bench_function("lexer/demo.rad", |b| {
        b.iter(|| {
            let mut lexer = Lexer::new(black_box(&source));
            let _ = lexer.tokenize();
        })
    });

    let pipeline_src = read_example("pipeline.rad");
    c.bench_function("lexer/pipeline.rad", |b| {
        b.iter(|| {
            let mut lexer = Lexer::new(black_box(&pipeline_src));
            let _ = lexer.tokenize();
        })
    });
}

fn bench_parser(c: &mut Criterion) {
    let source = read_example("demo.rad");
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().0;

    c.bench_function("parser/demo.rad", |b| {
        b.iter(|| {
            let mut parser = Parser::new(black_box(tokens.clone()));
            let _ = parser.parse();
        })
    });
}

fn bench_compile(c: &mut Criterion) {
    let source = read_example("demo.rad");
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();

    c.bench_function("compile/demo.rad", |b| {
        b.iter(|| {
            let mut checker = Checker::new();
            let _ = checker.check(black_box(&program));
            let compiler = Compiler::new();
            let _ = compiler.compile(&program).unwrap();
        })
    });
}

fn bench_vm_execution(c: &mut Criterion) {
    let examples = [
        "sorting.rad",
        "pipeline.rad",
        "ecs_benchmark.rad",
        "ecs_query_bench.rad",
        "pipeline_bench.rad",
    ];
    for name in &examples {
        let filepath = examples_dir().join(name);
        let filepath_str = filepath.to_str().unwrap().to_string();
        c.bench_function(&format!("core/vm/{}", name), |b| {
            b.iter(|| {
                let _ = full_pipeline(black_box(&filepath_str));
            })
        });
    }
}

fn bench_startup(c: &mut Criterion) {
    let source = "let x = 1";
    c.bench_function("startup/empty", |b| {
        b.iter(|| {
            let mut lexer = Lexer::new(black_box(source));
            let tokens = lexer.tokenize().0;
            let mut parser = Parser::new(tokens);
            let program = parser.parse();
            let mut checker = Checker::new();
            let _ = checker.check(&program);
            let compiler = Compiler::new();
            let compile_result = compiler.compile(&program).unwrap();
            let mut vm = VM::new();
            vm.suppress_output();
            vm.load_compile_result(compile_result);
            let _ = vm.run(0);
        })
    });
}

fn bench_fib(c: &mut Criterion) {
    let source = r#"
fn fib(n) {
    if n < 2 { return n }
    return fib(n - 1) + fib(n - 2)
}
fib(20)
"#;
    c.bench_function("core/vm/fib(20)", |b| {
        b.iter(|| {
            let mut lexer = Lexer::new(black_box(source));
            let tokens = lexer.tokenize().0;
            let mut parser = Parser::new(tokens);
            let program = parser.parse();
            let mut checker = Checker::new();
            let _ = checker.check(&program);
            let for_iter_hints = checker.for_iter_kinds();
            let compiler = Compiler::new().with_for_iter_kinds(for_iter_hints);
            let compile_result = compiler.compile(&program).unwrap();
            let mut vm = VM::new();
            vm.suppress_output();
            vm.load_compile_result(compile_result);
            let _ = vm.run(0);
        })
    });
}

const CAUSAL_SOURCE: &str = r#"
component Health { hp: int = 100000000, max: int = 100000000 }
intent Damage { key target: entity, amount: int, sequence: int }
law Hit(target: entity, amount: int, sequence: int) {
    propose Damage { target: target, amount: amount, sequence: sequence }
}
resolver ResolveDamage for Damage(target, proposals) {
    let health = require(target, Health)
    let total = proposals |> map(fn(p) { return p.amount }) |> sum()
    next(target, Health { hp: max(0, health.hp - total), max: health.max })
}
entity hero { Health {} }
fn attack(count: int) {
    settle {
        for sequence in range(0, count) { Hit(hero, 1, sequence) }
    }
}
"#;

fn causal_vm() -> VM {
    let mut lexer = Lexer::new(CAUSAL_SOURCE);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(parser.errors().is_empty(), "{:?}", parser.errors());
    let features = vec!["causal_laws".to_string()];
    let mut checker = Checker::new_with_options(CheckerOptions {
        features: features.clone(),
        ..CheckerOptions::default()
    });
    let errors = checker.check(&program);
    assert!(errors.is_empty(), "{errors:?}");
    let result = Compiler::new()
        .with_checker_output(checker.output())
        .with_features(features)
        .compile(&program)
        .expect("compile causal benchmark");
    let mut vm = VM::new();
    vm.suppress_output();
    vm.load_compile_result(result);
    vm.run(0).expect("initialize causal benchmark");
    vm.set_causality_retention_cap(10_100);
    vm
}

fn bench_causal_settlement(c: &mut Criterion) {
    let mut group = c.benchmark_group("causal/settlement_end_to_end");
    group.sample_size(10);
    for count in [1i64, 10, 100, 1_000, 10_000] {
        let mut vm = causal_vm();
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter(|| {
                vm.call_global("attack", &[FrozenValue::Int(black_box(count))])
                    .expect("benchmark settlement")
            })
        });
    }
    group.finish();
}

fn reference_damage(
    key: u32,
    proposals: &[ReferenceProposal],
    base: &ReferenceWorld,
) -> Result<Vec<ReferenceWrite>, String> {
    let health = base
        .component(key, "Health")
        .and_then(|component| component.get("hp"))
        .and_then(|value| match value {
            ReferenceValue::Int(value) => Some(*value),
            _ => None,
        })
        .ok_or_else(|| "missing Health.hp".to_string())?;
    let total =
        proposals
            .iter()
            .try_fold(0i64, |sum, proposal| match proposal.payload.get("amount") {
                Some(ReferenceValue::Int(amount)) => Ok(sum + amount),
                _ => Err("missing Damage.amount".to_string()),
            })?;
    Ok(vec![ReferenceWrite {
        entity: key,
        component: "Health".to_string(),
        value: BTreeMap::from([("hp".to_string(), ReferenceValue::Int(health - total))]),
    }])
}

fn reference_input(count: usize) -> (ReferenceWorld, Vec<ReferenceProposal>) {
    let base = ReferenceWorld {
        components: BTreeMap::from([(
            (0, "Health".to_string()),
            BTreeMap::from([("hp".to_string(), ReferenceValue::Int(100_000_000))]),
        )]),
    };
    let proposals = (0..count)
        .rev()
        .map(|sequence| ReferenceProposal {
            intent: "Damage".to_string(),
            key: 0,
            payload: BTreeMap::from([
                ("amount".to_string(), ReferenceValue::Int(1)),
                ("sequence".to_string(), ReferenceValue::Int(sequence as i64)),
            ]),
            canonical: format!("{sequence:020}"),
            producer: "Hit".to_string(),
            source_line: 1,
        })
        .collect();
    (base, proposals)
}

fn bench_causal_reference_and_provenance(c: &mut Criterion) {
    let resolvers = BTreeMap::from([(
        "Damage".to_string(),
        ReferenceResolver {
            name: "ResolveDamage",
            resolve: reference_damage,
        },
    )]);
    let mut reference = c.benchmark_group("causal/reference_group_sort_resolve_patch");
    reference.sample_size(10);
    for count in [1usize, 10, 100, 1_000, 10_000] {
        let (base, proposals) = reference_input(count);
        reference.throughput(Throughput::Elements(count as u64));
        reference.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                settle_reference(
                    black_box(&base),
                    black_box(proposals.clone()),
                    black_box(&resolvers),
                )
                .expect("reference settlement")
            })
        });
    }
    reference.finish();

    let mut provenance = c.benchmark_group("causal/provenance");
    provenance.sample_size(10);
    for count in [1i64, 100, 10_000] {
        let mut vm = causal_vm();
        vm.call_global("attack", &[FrozenValue::Int(count)])
            .unwrap();
        provenance.bench_with_input(BenchmarkId::new("why_render", count), &count, |b, _| {
            b.iter(|| {
                vm.causality_ledger().explain_named(
                    black_box("hero"),
                    black_box("Health"),
                    u64::MAX,
                )
            })
        });
        let closure = vm.causality_ledger().provenance_closure(|_| true, &[]);
        provenance.bench_with_input(BenchmarkId::new("wire_encode", count), &count, |b, _| {
            b.iter(|| {
                let mut encoded = String::new();
                rad_vm::wire::encode_prov_into(black_box(&closure), &mut encoded);
                encoded
            })
        });
    }
    provenance.finish();
}

fn bench_causal_phase_baselines(c: &mut Criterion) {
    {
        let mut group = c.benchmark_group("causal/phase/proposal_creation");
        group.sample_size(10);
        for count in [1usize, 10, 100, 1_000, 10_000] {
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                b.iter(|| reference_input(black_box(count)).1)
            });
        }
        group.finish();
    }
    {
        let mut group = c.benchmark_group("causal/phase/canonical_sort");
        group.sample_size(10);
        for count in [1usize, 10, 100, 1_000, 10_000] {
            let (_, proposals) = reference_input(count);
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
                b.iter_batched(
                    || proposals.clone(),
                    |mut sorted| {
                        sorted.sort_by(|left, right| {
                            (
                                &left.intent,
                                left.key,
                                &left.canonical,
                                &left.producer,
                                left.source_line,
                            )
                                .cmp(&(
                                    &right.intent,
                                    right.key,
                                    &right.canonical,
                                    &right.producer,
                                    right.source_line,
                                ))
                        });
                        sorted
                    },
                    BatchSize::SmallInput,
                )
            });
        }
        group.finish();
    }
    {
        let mut group = c.benchmark_group("causal/phase/resolver_candidate");
        group.sample_size(10);
        for count in [1usize, 10, 100, 1_000, 10_000] {
            let (base, mut proposals) = reference_input(count);
            proposals.sort_by(|left, right| left.canonical.cmp(&right.canonical));
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
                b.iter(|| {
                    reference_damage(black_box(0), black_box(&proposals), black_box(&base)).unwrap()
                })
            });
        }
        group.finish();
    }
    {
        let mut group = c.benchmark_group("causal/phase/candidate_adoption");
        group.sample_size(10);
        for count in [1usize, 10, 100, 1_000, 10_000] {
            let (base, proposals) = reference_input(count);
            let writes = reference_damage(0, &proposals, &base).unwrap();
            group.throughput(Throughput::Elements(writes.len() as u64));
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
                b.iter(|| {
                    let mut candidate = base.clone();
                    for write in &writes {
                        candidate
                            .components
                            .insert((write.entity, write.component.clone()), write.value.clone());
                    }
                    candidate
                })
            });
        }
        group.finish();
    }
}

criterion_group!(
    benches,
    bench_lexer,
    bench_parser,
    bench_compile,
    bench_vm_execution,
    bench_startup,
    bench_fib,
    bench_causal_settlement,
    bench_causal_reference_and_provenance,
    bench_causal_phase_baselines,
);
criterion_main!(benches);
