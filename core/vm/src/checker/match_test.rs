use crate::checker::typeck::TypeChecker;
use crate::parser::Parser;
use crate::ast::Stmt;

fn check(src: &str) -> TypeChecker {
    let mut parser = Parser::new(src, "test.lang");
    let mut stmts = Vec::new();
    while !parser.is_at_end() {
        stmts.push(parser.parse_stmt().unwrap());
    }
    let mut checker = TypeChecker::new();
    for stmt in &stmts {
        checker.check_stmt(stmt);
    }
    checker
}

#[test]
fn test_match_unreachable() {
    let src = "
    match 1 {
        _ => {}
        2 => {}
    }
    ";
    let checker = check(src);
    assert!(checker.errors.iter().any(|e| e.message.contains("Unreachable")));
}
