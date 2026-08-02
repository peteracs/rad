use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::PathBuf;

use rad_vm::checker::Checker;
use rad_vm::compiler::Compiler;
use rad_vm::lexer::Lexer;
use rad_vm::module_loader::load_program_with_uses;
use rad_vm::parser::Parser;
use rad_vm::vm::VM;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

criterion_group!(
    benches,
    bench_lexer,
    bench_parser,
    bench_compile,
    bench_vm_execution,
    bench_startup,
    bench_fib,
);
criterion_main!(benches);
