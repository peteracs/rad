//! Compiler-owned evidence for language-surface coverage.
//!
//! This is deliberately an AST/token projection, not a source-text scanner.
//! Every enum match is exhaustive so adding a parser node forces this report
//! to classify it before the compiler builds again.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use crate::ast::{
    BinOp, CallableContracts, Decl, Expr, FnTypePurity, ModuleAlias, Pattern, Program, SourceMap,
    Stmt, TypeExpr, UnaryOp,
};
use crate::lexer::Lexer;
use crate::visitor::{self, AstVisitor};

/// Exact compiler nodes and runtime builtins observed in one or more loaded
/// source graphs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageSurfaceCoverage {
    pub tokens: BTreeSet<String>,
    pub declarations: BTreeSet<String>,
    pub statements: BTreeSet<String>,
    pub expressions: BTreeSet<String>,
    pub binary_operators: BTreeSet<String>,
    pub unary_operators: BTreeSet<String>,
    pub patterns: BTreeSet<String>,
    pub type_expressions: BTreeSet<String>,
    pub function_type_purities: BTreeSet<String>,
    pub callable_contracts: BTreeSet<String>,
    pub builtins: BTreeSet<String>,
    pub source_files: BTreeSet<String>,
    pub lexical_errors: BTreeMap<String, Vec<String>>,
}

impl LanguageSurfaceCoverage {
    /// Collect one complete loaded module graph. Namespaced modules are
    /// visited once by semantic identity; flattened imports already occur in
    /// `program` and are not counted twice.
    pub fn loaded(
        program: &Program,
        aliases: &HashMap<String, ModuleAlias>,
        source_map: &SourceMap,
    ) -> Self {
        let mut coverage = Self::default();
        coverage.collect_program(program);
        for binding in crate::ast::canonical_module_bindings(aliases) {
            for declaration in binding.declarations() {
                coverage.visit_decl(declaration);
            }
        }
        for source_file in source_map.files() {
            coverage.source_files.insert(source_file.path.clone());
            let mut lexer = Lexer::new(&source_file.source);
            lexer.preserve_comments = true;
            let (tokens, errors) = lexer.tokenize();
            coverage
                .tokens
                .extend(tokens.into_iter().map(|token| format!("{:?}", token.ty)));
            if !errors.is_empty() {
                coverage.lexical_errors.insert(
                    source_file.path.clone(),
                    errors
                        .into_iter()
                        .map(|error| format!("{}:{}: {}", error.line, error.col, error.message))
                        .collect(),
                );
            }
        }
        coverage
    }

    pub fn collect_program(&mut self, program: &Program) {
        self.visit_program(program);
    }

    pub fn merge(&mut self, other: Self) {
        self.tokens.extend(other.tokens);
        self.declarations.extend(other.declarations);
        self.statements.extend(other.statements);
        self.expressions.extend(other.expressions);
        self.binary_operators.extend(other.binary_operators);
        self.unary_operators.extend(other.unary_operators);
        self.patterns.extend(other.patterns);
        self.type_expressions.extend(other.type_expressions);
        self.function_type_purities
            .extend(other.function_type_purities);
        self.callable_contracts.extend(other.callable_contracts);
        self.builtins.extend(other.builtins);
        self.source_files.extend(other.source_files);
        for (path, errors) in other.lexical_errors {
            self.lexical_errors.entry(path).or_default().extend(errors);
        }
    }
}

impl AstVisitor for LanguageSurfaceCoverage {
    fn visit_decl(&mut self, declaration: &Decl) {
        let name = match declaration {
            Decl::Component(_) => "Component",
            Decl::Resource(_) => "Resource",
            Decl::Struct(_) => "Struct",
            Decl::Intent(_) => "Intent",
            Decl::Law(_) => "Law",
            Decl::Resolver(_) => "Resolver",
            Decl::Constraint(_) => "Constraint",
            Decl::Entity(_) => "Entity",
            Decl::State(_) => "State",
            Decl::System(system) => {
                collect_contracts(&mut self.callable_contracts, &system.contracts);
                "System"
            }
            Decl::Event(_) => "Event",
            Decl::OnHandler(handler) => {
                collect_contracts(&mut self.callable_contracts, &handler.contracts);
                "OnHandler"
            }
            Decl::Migration(_) => "Migration",
            Decl::Phase(_) => "Phase",
            Decl::Fn(_) => "Fn",
            Decl::Type(_) => "Type",
            Decl::TypeAlias(_) => "TypeAlias",
            Decl::NativeType(_) => "NativeType",
            Decl::MaterializedView(_) => "MaterializedView",
            Decl::Use(_) => "Use",
            Decl::Test(_) => "Test",
            Decl::Model(_) => {
                // A model declaration lowers to the runtime-owned
                // `model_check` builtin even though the call is not written
                // as an ordinary source expression.
                self.builtins.insert("model_check".to_string());
                "Model"
            }
            Decl::Stmt(_) => "Stmt",
            Decl::Error => "Error",
        };
        self.declarations.insert(name.to_string());
        visitor::walk_decl(self, declaration);
    }

    fn visit_stmt(&mut self, statement: &Stmt) {
        let name = match statement {
            Stmt::Let(_) => "Let",
            Stmt::LetElse(_) => "LetElse",
            Stmt::Assign(_) => "Assign",
            Stmt::If(_) => "If",
            Stmt::While(_) => "While",
            Stmt::For(_) => "For",
            Stmt::Return(_) => "Return",
            Stmt::Break(_) => "Break",
            Stmt::Continue(_) => "Continue",
            Stmt::Emit(_) => "Emit",
            Stmt::Schedule(_) => "Schedule",
            Stmt::Update(_) => "Update",
            Stmt::Transaction(_) => "Transaction",
            Stmt::Settle(_) => "Settle",
            Stmt::Propose(_) => "Propose",
            Stmt::Next(_) => "Next",
            Stmt::Require(_) => "Require",
            Stmt::Match(_) => "Match",
            Stmt::Expr(_) => "Expr",
            Stmt::OnceGuardPass(_) => "OnceGuardPass",
            Stmt::Error(_) => "Error",
        };
        self.statements.insert(name.to_string());
        visitor::walk_stmt(self, statement);
    }

    fn visit_pattern(&mut self, pattern: &Pattern) {
        let name = match pattern {
            Pattern::Wildcard => "Wildcard",
            Pattern::Literal(_) => "Literal",
            Pattern::Variant { .. } => "Variant",
            Pattern::HasComponent { .. } => "HasComponent",
        };
        self.patterns.insert(name.to_string());
        visitor::walk_pattern(self, pattern);
    }

    fn visit_expr(&mut self, expression: &Expr) {
        let name = match expression {
            Expr::IntLit(_, _) => "IntLit",
            Expr::FloatLit(_, _) => "FloatLit",
            Expr::StrLit(_, _) => "StrLit",
            Expr::BoolLit(_, _) => "BoolLit",
            Expr::NilLit(_) => "NilLit",
            Expr::ListLit(_, _) => "ListLit",
            Expr::MapLit(_, _) => "MapLit",
            Expr::TupleLit(_, _) => "TupleLit",
            Expr::FStringExpr(_, _) => "FStringExpr",
            Expr::Ident(_, _) => "Ident",
            Expr::Binary(_, operator, _, _) => {
                self.binary_operators
                    .insert(binary_operator_name(*operator).to_string());
                "Binary"
            }
            Expr::Unary(operator, _, _) => {
                self.unary_operators
                    .insert(unary_operator_name(*operator).to_string());
                "Unary"
            }
            Expr::Pipe(_, _, _) => "Pipe",
            Expr::Call(callee, _, _) => {
                collect_builtin_call(&mut self.builtins, callee);
                "Call"
            }
            Expr::Field(_, _, _) => "Field",
            Expr::Index(_, _, _) => "Index",
            Expr::ComponentExpr(_, _, _, _) => "ComponentExpr",
            Expr::StateRef(_, _, _) => "StateRef",
            Expr::SystemRef(_, _) => "SystemRef",
            Expr::VariantExpr(_, _, _, _) => "VariantExpr",
            Expr::MatchExpr(_, _) => "MatchExpr",
            Expr::IfExpr(_, _, _, _) => "IfExpr",
            Expr::FnExpr(_, _, _, _, _, _, _) => "FnExpr",
            Expr::QueryExpr(_, _) => "QueryExpr",
            Expr::Await(_, _) => "Await",
            Expr::AsyncCall(callee, _, _) => {
                collect_builtin_call(&mut self.builtins, callee);
                "AsyncCall"
            }
            Expr::Try(_, _) => "Try",
            Expr::Spread(_, _) => "Spread",
            Expr::EntityLiteral(_, _, _) => "EntityLiteral",
            Expr::Error(_) => "Error",
        };
        self.expressions.insert(name.to_string());
        visitor::walk_expr(self, expression);
    }

    fn visit_type_expr(&mut self, type_expression: &TypeExpr) {
        let name = match type_expression {
            TypeExpr::Named(_) => "Named",
            TypeExpr::Generic(_, _) => "Generic",
            TypeExpr::Tuple(_) => "Tuple",
            TypeExpr::FnType(_, _, purity) => {
                self.function_type_purities
                    .insert(function_type_purity_name(*purity).to_string());
                "FnType"
            }
            TypeExpr::Union(_) => "Union",
        };
        self.type_expressions.insert(name.to_string());
        visitor::walk_type_expr(self, type_expression);
    }
}

fn collect_builtin_call(output: &mut BTreeSet<String>, callee: &Expr) {
    if let Expr::Ident(name, _) = callee {
        if crate::builtins::is_builtin(name) {
            output.insert(name.clone());
        }
    }
}

fn binary_operator_name(operator: BinOp) -> &'static str {
    match operator {
        BinOp::Add => "Add",
        BinOp::Sub => "Sub",
        BinOp::Mul => "Mul",
        BinOp::Div => "Div",
        BinOp::Mod => "Mod",
        BinOp::Eq => "Eq",
        BinOp::Ne => "Ne",
        BinOp::Lt => "Lt",
        BinOp::Le => "Le",
        BinOp::Gt => "Gt",
        BinOp::Ge => "Ge",
        BinOp::And => "And",
        BinOp::Or => "Or",
        BinOp::Is => "Is",
        BinOp::BitAnd => "BitAnd",
        BinOp::BitOr => "BitOr",
        BinOp::BitXor => "BitXor",
        BinOp::Shl => "Shl",
        BinOp::Shr => "Shr",
    }
}

fn unary_operator_name(operator: UnaryOp) -> &'static str {
    match operator {
        UnaryOp::Neg => "Neg",
        UnaryOp::Not => "Not",
        UnaryOp::BitNot => "BitNot",
    }
}

fn function_type_purity_name(purity: FnTypePurity) -> &'static str {
    match purity {
        FnTypePurity::Default => "Default",
        FnTypePurity::Pure => "Pure",
        FnTypePurity::Readonly => "Readonly",
    }
}

fn collect_contracts(output: &mut BTreeSet<String>, contracts: &CallableContracts) {
    let pairs = [
        (contracts.frame, "frame"),
        (contracts.tick, "tick"),
        (contracts.render, "render"),
        (contracts.no_full_scan, "no_full_scan"),
        (contracts.no_guest_allocation, "no_guest_allocation"),
        (contracts.no_runtime_allocation, "no_runtime_allocation"),
        (contracts.no_host_allocation, "no_host_allocation"),
        (contracts.no_nested_flush, "no_nested_flush"),
        (contracts.non_reentrant, "non_reentrant"),
        (contracts.exactly_once, "exactly_once"),
    ];
    output.extend(
        pairs
            .into_iter()
            .filter_map(|(enabled, name)| enabled.then_some(name.to_string())),
    );
    if contracts.instruction_budget.is_some() {
        output.insert("budget".to_string());
    }
    if contracts.allow_full_scan_reason.is_some() {
        output.insert("allow_full_scan".to_string());
    }
    if contracts.must_complete_before.is_some() {
        output.insert("must_complete_before".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    #[test]
    fn reports_nested_nodes_operators_contracts_and_builtins() {
        let source = r#"
            component Item { value: int }
            @frame @no_full_scan @budget(instructions: 100)
            system Tick(item: mut Item) {
                let result: (int, int) = (1 + 2, ~3)
                assert(len([result]) == 1)
            }
        "#;
        let (tokens, lex_errors) = Lexer::new(source).tokenize();
        assert!(lex_errors.is_empty(), "{lex_errors:?}");
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(parser.errors().is_empty(), "{:?}", parser.errors());
        let mut source_map = SourceMap::new();
        source_map.add_file("surface.rad".to_string(), source.to_string());
        let coverage = LanguageSurfaceCoverage::loaded(&program, &HashMap::new(), &source_map);
        assert!(coverage.declarations.contains("Component"));
        assert!(coverage.declarations.contains("System"));
        assert!(coverage.expressions.contains("TupleLit"));
        assert!(coverage.binary_operators.contains("Add"));
        assert!(coverage.unary_operators.contains("BitNot"));
        assert!(coverage.callable_contracts.contains("frame"));
        assert!(coverage.callable_contracts.contains("no_full_scan"));
        assert!(coverage.callable_contracts.contains("budget"));
        assert!(coverage.builtins.contains("assert"));
        assert!(coverage.builtins.contains("len"));
    }
}
