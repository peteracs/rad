use super::ast::*;
use super::lexer::{self, Token, TokenKind};
use super::limits::{RawInputLimits, RawInputMeter};
use super::{DiagnosticCode, FrontendDiagnostic};
use std::sync::Arc;

pub(crate) fn parse(
    source: &str,
    module_id: &str,
    limits: RawInputLimits,
) -> Result<BoundedRawProgram, Vec<FrontendDiagnostic>> {
    let mut meter = RawInputMeter::new(source.len(), limits).map_err(|error| vec![error])?;
    let lexed = lexer::lex(source, &mut meter).map_err(|error| vec![error])?;
    if lexed.maximum_identifier_length > limits.max_identifier_bytes {
        return Err(vec![FrontendDiagnostic::limit(
            DiagnosticCode::RawIdentifierByteLimit,
            lexed.maximum_identifier_length,
            limits.max_identifier_bytes,
        )]);
    }
    if module_id.is_empty() || module_id.split("::").any(|part| !valid_module_part(part)) {
        return Err(vec![FrontendDiagnostic::new(
            DiagnosticCode::UnqualifiedModule,
            "relation module identity must contain nonempty path segments",
            0,
            0,
            module_id.as_bytes(),
        )]);
    }
    if module_id.len() > limits.max_identifier_bytes {
        return Err(vec![FrontendDiagnostic::limit(
            DiagnosticCode::RawIdentifierByteLimit,
            module_id.len(),
            limits.max_identifier_bytes,
        )]);
    }
    meter
        .structural(module_id.len().saturating_add(8))
        .map_err(|error| vec![error])?;
    let parser = Parser {
        source,
        module_id: Arc::from(module_id),
        tokens: lexed.tokens,
        position: 0,
        meter,
        relations: Vec::new(),
        rules: Vec::new(),
        operations: Vec::new(),
    };
    parser.parse_program()
}

struct Parser<'a> {
    source: &'a str,
    module_id: Arc<str>,
    tokens: Vec<Token>,
    position: usize,
    meter: RawInputMeter,
    relations: Vec<RelationSchema>,
    rules: Vec<BoundedRawRule>,
    operations: Vec<RelationOperation>,
}

impl Parser<'_> {
    fn parse_program(mut self) -> Result<BoundedRawProgram, Vec<FrontendDiagnostic>> {
        while !self.at(TokenKind::Eof) {
            self.skip_newlines();
            if self.at(TokenKind::Eof) {
                break;
            }
            let keyword = self.peek_text().to_string();
            let result = match keyword.as_str() {
                "relation" => self.parse_relation(),
                "derive" => self.parse_rule(),
                "Insert" | "insert" => self.parse_operation(OperationTag::Insert),
                "Remove" | "remove" => self.parse_operation(OperationTag::Remove),
                "ReplaceBy" | "replace_by" => self.parse_operation(OperationTag::ReplaceBy),
                _ => Err(self.syntax(format!(
                    "expected relation, derive, Insert, Remove, or ReplaceBy; found '{keyword}'"
                ))),
            };
            if let Err(error) = result {
                return Err(vec![error]);
            }
        }

        let mut raw_diagnostics = Vec::new();
        for rule in &self.rules {
            raw_diagnostics.extend(self.meter.check_summary(rule.summary()));
        }
        let limits = self.limits();
        let stats = self.meter.finish();
        if stats.ast_nodes > limits.max_ast_nodes {
            raw_diagnostics.push(FrontendDiagnostic::limit(
                DiagnosticCode::RawAstNodeLimit,
                stats.ast_nodes,
                limits.max_ast_nodes,
            ));
        }
        if stats.structural_cost > limits.max_structural_cost {
            raw_diagnostics.push(FrontendDiagnostic::limit(
                DiagnosticCode::RawStructuralCostLimit,
                stats.structural_cost,
                limits.max_structural_cost,
            ));
        }
        if let Some(diagnostic) = raw_diagnostics.into_iter().min() {
            return Err(vec![diagnostic]);
        }
        Ok(BoundedRawProgram::new(
            vec![self.module_id.to_string()],
            self.relations,
            self.rules,
            self.operations,
            stats,
        ))
    }

    fn parse_relation(&mut self) -> Result<(), FrontendDiagnostic> {
        self.word("relation")?;
        self.meter.relation(self.relations.len() + 1)?;
        let name = self.qualified_identifier()?;
        self.expect(TokenKind::LParen)?;
        let mut columns = Vec::new();
        let mut column_count = 0usize;
        if !self.at(TokenKind::RParen) {
            loop {
                let column_name = self.identifier()?;
                self.expect(TokenKind::Colon)?;
                let value_type = self.parse_type()?;
                let mut on_delete =
                    (value_type == RelationType::Entity).then_some(OnDelete::Restrict);
                if self.at_word("on") {
                    self.word("on")?;
                    self.word("delete")?;
                    on_delete = Some(self.parse_delete_policy()?);
                }
                column_count = column_count.saturating_add(1);
                if column_count > self.limits().max_columns_per_relation {
                    return Err(FrontendDiagnostic::limit(
                        DiagnosticCode::RawColumnLimit,
                        column_count,
                        self.limits().max_columns_per_relation,
                    ));
                }
                self.meter.ast_node()?;
                columns.push(RelationColumn {
                    name: column_name,
                    value_type,
                    on_delete,
                });
                if !self.take(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        self.consume_line_end()?;

        let mut unique = Vec::new();
        let mut unique_count = 0usize;
        let mut symmetric = false;
        let mut relation_policy = None;
        loop {
            self.skip_newlines();
            if self.at_word("unique") {
                self.word("unique")?;
                let mut names = vec![self.identifier()?];
                while self.take(TokenKind::Comma) {
                    names.push(self.identifier()?);
                }
                let constraint_name = if self.at_word("as") {
                    self.word("as")?;
                    self.identifier()?
                } else {
                    names.join("_")
                };
                unique_count = unique_count.saturating_add(1);
                if unique_count > self.limits().max_unique_constraints_per_relation {
                    return Err(FrontendDiagnostic::limit(
                        DiagnosticCode::RawUniqueConstraintLimit,
                        unique_count,
                        self.limits().max_unique_constraints_per_relation,
                    ));
                }
                self.meter.ast_node()?;
                unique.push(UniqueConstraint {
                    name: constraint_name,
                    columns: names,
                });
                self.consume_line_end()?;
            } else if self.at_word("symmetric") {
                self.word("symmetric")?;
                symmetric = true;
                self.consume_line_end()?;
            } else if self.at_word("on") {
                self.word("on")?;
                self.word("delete")?;
                relation_policy = Some(self.parse_delete_policy()?);
                self.consume_line_end()?;
            } else {
                break;
            }
        }
        if let Some(policy) = relation_policy {
            for column in &mut columns {
                if column.value_type == RelationType::Entity {
                    column.on_delete = Some(policy);
                }
            }
        }
        self.meter.ast_node()?;
        let identity = self.qualify(&name)?;
        self.relations.push(RelationSchema {
            identity,
            columns,
            unique,
            symmetric,
        });
        Ok(())
    }

    fn parse_rule(&mut self) -> Result<(), FrontendDiagnostic> {
        self.word("derive")?;
        self.meter.rule(self.rules.len() + 1)?;
        let mut summary = RawRuleSummary::default();
        let head_relation = self.rule_identifier(&mut summary)?;
        self.expect(TokenKind::LParen)?;
        let mut head = Vec::new();
        let mut aggregate = None;
        let mut inferred_groups = Vec::new();
        let mut inferred_group_count = 0usize;
        if !self.at(TokenKind::RParen) {
            loop {
                if let Some(kind) = self.aggregate_kind() {
                    if aggregate.is_some() {
                        return Err(self.syntax("v0 permits one aggregate per rule head"));
                    }
                    self.advance();
                    self.expect(TokenKind::LParen)?;
                    let input = if self.at(TokenKind::RParen) {
                        None
                    } else {
                        Some(self.rule_identifier(&mut summary)?)
                    };
                    self.expect(TokenKind::RParen)?;
                    let output = format!("__{}_value", aggregate_name(kind));
                    self.observe_identifier(&mut summary, &output);
                    summary.total_terms = summary.total_terms.saturating_add(1);
                    self.rule_node(&mut summary)?;
                    self.push_rule_term(&mut head, RawTerm::Variable(output.clone()));
                    aggregate = Some(RawAggregate {
                        kind,
                        input,
                        output,
                        group_by: Vec::new(),
                    });
                } else {
                    let term = self.parse_rule_term(&mut summary)?;
                    if let RawTerm::Variable(name) = &term {
                        inferred_group_count = inferred_group_count.saturating_add(1);
                        if inferred_groups.len() < self.limits().max_aggregate_groups_per_rule {
                            inferred_groups.push(name.clone());
                        }
                    }
                    self.push_rule_term(&mut head, term);
                }
                if !self.take(TokenKind::Comma) {
                    break;
                }
            }
        }
        summary.head_terms = summary.total_terms;
        self.expect(TokenKind::RParen)?;
        self.consume_line_end()?;
        if let Some(aggregate) = &mut aggregate {
            aggregate.group_by = inferred_groups;
            summary.aggregate_groups = inferred_group_count;
        }

        let mut atoms = Vec::new();
        let mut predicates = Vec::new();
        let mut first = true;
        loop {
            self.skip_newlines();
            let marker = if first { "when" } else { "and" };
            if !self.at_word(marker) {
                break;
            }
            self.word(marker)?;
            first = false;
            let left = self.rule_identifier(&mut summary)?;
            if self.take(TokenKind::Greater) {
                let right = self.rule_identifier(&mut summary)?;
                summary.predicates = summary.predicates.saturating_add(1);
                self.rule_node(&mut summary)?;
                if predicates.len() < self.limits().max_predicates_per_rule {
                    predicates.push(RawPredicate::Greater(left, right));
                }
            } else {
                self.expect(TokenKind::LParen)?;
                let mut terms = Vec::new();
                if !self.at(TokenKind::RParen) {
                    loop {
                        let term = self.parse_rule_term(&mut summary)?;
                        if summary.total_terms <= self.limits().max_terms_per_rule {
                            terms.push(term);
                        }
                        if !self.take(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RParen)?;
                summary.atoms = summary.atoms.saturating_add(1);
                self.rule_node(&mut summary)?;
                if atoms.len() < self.limits().max_atoms_per_rule {
                    atoms.push(RawAtom {
                        relation: self.qualify(&left)?,
                        terms,
                    });
                }
            }
            self.consume_line_end()?;
        }
        self.rule_node(&mut summary)?;
        let ast = RawRuleAst {
            explicit_id: None,
            head_relation: self.qualify(&head_relation)?,
            head,
            atoms,
            predicates,
            aggregate,
        };
        self.rules.push(BoundedRawRule::new(
            ast,
            Arc::clone(&self.module_id),
            summary,
        ));
        Ok(())
    }

    fn parse_operation(&mut self, tag: OperationTag) -> Result<(), FrontendDiagnostic> {
        self.advance();
        self.meter.operation(self.operations.len() + 1)?;
        self.expect(TokenKind::LParen)?;
        let relation = self.qualified_identifier()?;
        self.expect(TokenKind::Comma)?;
        let kind = match tag {
            OperationTag::Insert => RelationOperationKind::Insert,
            OperationTag::Remove => RelationOperationKind::Remove,
            OperationTag::ReplaceBy => {
                let constraint = self.identifier()?;
                self.expect(TokenKind::Comma)?;
                let key = if self.at(TokenKind::LParen) {
                    self.parse_operation_tuple()?
                } else {
                    vec![self.parse_operation_value()?]
                };
                self.expect(TokenKind::Comma)?;
                RelationOperationKind::ReplaceBy { constraint, key }
            }
        };
        let tuple = self.parse_operation_tuple()?;
        self.expect(TokenKind::RParen)?;
        self.consume_line_end()?;
        self.meter.ast_node()?;
        let relation = self.qualify(&relation)?;
        self.operations.push(RelationOperation {
            kind,
            relation,
            tuple,
        });
        Ok(())
    }

    fn parse_operation_tuple(&mut self) -> Result<Vec<RawOperationValue>, FrontendDiagnostic> {
        self.expect(TokenKind::LParen)?;
        let mut tuple = Vec::new();
        let mut tuple_count = 0usize;
        if !self.at(TokenKind::RParen) {
            loop {
                let value = self.parse_operation_value()?;
                tuple_count = tuple_count.saturating_add(1);
                if tuple_count > self.limits().max_columns_per_relation {
                    return Err(FrontendDiagnostic::limit(
                        DiagnosticCode::RawTupleLimit,
                        tuple_count,
                        self.limits().max_columns_per_relation,
                    ));
                }
                tuple.push(value);
                if !self.take(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        Ok(tuple)
    }

    fn parse_operation_value(&mut self) -> Result<RawOperationValue, FrontendDiagnostic> {
        match self.peek().kind {
            TokenKind::Integer => {
                let value = self.parse_integer()?;
                self.meter.ast_node()?;
                Ok(RawOperationValue::Literal(Literal::Int(value)))
            }
            TokenKind::String => {
                let value = self.string_literal()?;
                self.meter.ast_node()?;
                Ok(RawOperationValue::Literal(Literal::Text(value)))
            }
            _ => {
                let name = self.qualified_identifier()?;
                self.meter.ast_node()?;
                Ok(RawOperationValue::Variable(name))
            }
        }
    }

    fn parse_rule_term(
        &mut self,
        summary: &mut RawRuleSummary,
    ) -> Result<RawTerm, FrontendDiagnostic> {
        summary.total_terms = summary.total_terms.saturating_add(1);
        match self.peek().kind {
            TokenKind::Integer => {
                let value = self.parse_integer()?;
                self.rule_node(summary)?;
                Ok(RawTerm::Literal(Literal::Int(value)))
            }
            TokenKind::String => {
                let value = self.string_literal()?;
                summary.structural_cost = summary.structural_cost.saturating_add(value.len() + 9);
                self.rule_node(summary)?;
                Ok(RawTerm::Literal(Literal::Text(value)))
            }
            _ => {
                let name = self.rule_identifier(summary)?;
                self.rule_node(summary)?;
                Ok(RawTerm::Variable(name))
            }
        }
    }

    fn push_rule_term(&mut self, terms: &mut Vec<RawTerm>, term: RawTerm) {
        if terms.len() < self.limits().max_terms_per_rule {
            terms.push(term);
        }
    }

    fn rule_identifier(
        &mut self,
        summary: &mut RawRuleSummary,
    ) -> Result<String, FrontendDiagnostic> {
        let value = self.qualified_identifier()?;
        self.observe_identifier(summary, &value);
        Ok(value)
    }

    fn observe_identifier(&mut self, summary: &mut RawRuleSummary, value: &str) {
        summary.maximum_identifier_length = summary.maximum_identifier_length.max(value.len());
        summary.structural_cost = summary.structural_cost.saturating_add(value.len() + 8);
    }

    fn rule_node(&mut self, summary: &mut RawRuleSummary) -> Result<(), FrontendDiagnostic> {
        summary.ast_nodes = summary.ast_nodes.saturating_add(1);
        self.meter.ast_node()
    }

    fn parse_type(&mut self) -> Result<RelationType, FrontendDiagnostic> {
        let token = self.advance();
        if token.kind != TokenKind::Ident {
            return Err(self.syntax_at(token, "expected relation column type"));
        }
        match token.text(self.source) {
            "entity" => Ok(RelationType::Entity),
            "int" => Ok(RelationType::Int),
            "count" => Ok(RelationType::Count),
            "str" | "text" => Ok(RelationType::Text),
            other => Err(self.syntax_at(token, format!("unsupported relation type '{other}'"))),
        }
    }

    fn parse_delete_policy(&mut self) -> Result<OnDelete, FrontendDiagnostic> {
        if self.at_word("restrict") {
            self.word("restrict")?;
            Ok(OnDelete::Restrict)
        } else if self.at_word("cascade") {
            self.word("cascade")?;
            Ok(OnDelete::Cascade)
        } else {
            Err(self.syntax("expected restrict or cascade"))
        }
    }

    fn aggregate_kind(&self) -> Option<AggregateKind> {
        match self.peek_text() {
            "count" => Some(AggregateKind::Count),
            "sum" => Some(AggregateKind::Sum),
            "min" => Some(AggregateKind::Min),
            "max" => Some(AggregateKind::Max),
            _ => None,
        }
    }

    fn parse_integer(&mut self) -> Result<i64, FrontendDiagnostic> {
        let token = self.expect(TokenKind::Integer)?;
        token
            .text(self.source)
            .parse()
            .map_err(|_| self.syntax_at(token, "integer literal is outside i64"))
    }

    fn string_literal(&mut self) -> Result<String, FrontendDiagnostic> {
        let token = self.expect(TokenKind::String)?;
        let raw = token.text(self.source);
        let inner = &raw[1..raw.len() - 1];
        self.meter.structural(inner.len() + 8)?;
        Ok(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
    }

    fn qualified_identifier(&mut self) -> Result<String, FrontendDiagnostic> {
        let mut output = self.identifier()?;
        while self.take(TokenKind::DColon) {
            let part = self.identifier()?;
            let next = output.len().saturating_add(2).saturating_add(part.len());
            if next > self.limits().max_identifier_bytes {
                return Err(FrontendDiagnostic::limit(
                    DiagnosticCode::RawIdentifierByteLimit,
                    next,
                    self.limits().max_identifier_bytes,
                ));
            }
            output.push_str("::");
            output.push_str(&part);
        }
        Ok(output)
    }

    fn identifier(&mut self) -> Result<String, FrontendDiagnostic> {
        let token = self.expect(TokenKind::Ident)?;
        let text = token.text(self.source);
        self.meter.structural(text.len() + 8)?;
        Ok(text.to_string())
    }

    fn qualify(&mut self, identity: &str) -> Result<String, FrontendDiagnostic> {
        if identity.contains("::") {
            Ok(identity.to_string())
        } else {
            let length = self
                .module_id
                .len()
                .saturating_add(2)
                .saturating_add(identity.len());
            if length > self.limits().max_identifier_bytes {
                Err(FrontendDiagnostic::limit(
                    DiagnosticCode::RawIdentifierByteLimit,
                    length,
                    self.limits().max_identifier_bytes,
                ))
            } else {
                self.meter
                    .structural(self.module_id.len().saturating_add(2))?;
                Ok(format!("{}::{identity}", self.module_id))
            }
        }
    }

    fn consume_line_end(&mut self) -> Result<(), FrontendDiagnostic> {
        if self.take(TokenKind::Newline) || self.at(TokenKind::Eof) {
            Ok(())
        } else {
            Err(self.syntax("expected end of line"))
        }
    }

    fn skip_newlines(&mut self) {
        while self.take(TokenKind::Newline) {}
    }

    fn word(&mut self, expected: &str) -> Result<Token, FrontendDiagnostic> {
        let token = self.expect(TokenKind::Ident)?;
        if token.text(self.source) == expected {
            Ok(token)
        } else {
            Err(self.syntax_at(token, format!("expected '{expected}'")))
        }
    }

    fn at_word(&self, expected: &str) -> bool {
        self.peek().kind == TokenKind::Ident && self.peek_text() == expected
    }

    fn peek_text(&self) -> &str {
        self.peek().text(self.source)
    }

    fn expect(&mut self, expected: TokenKind) -> Result<Token, FrontendDiagnostic> {
        let token = self.advance();
        if token.kind == expected {
            Ok(token)
        } else {
            Err(self.syntax_at(token, format!("expected {expected:?}")))
        }
    }

    fn take(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn peek(&self) -> Token {
        self.tokens[self.position]
    }

    fn advance(&mut self) -> Token {
        let token = self.peek();
        if token.kind != TokenKind::Eof {
            self.position += 1;
        }
        token
    }

    fn syntax(&self, message: impl Into<String>) -> FrontendDiagnostic {
        self.syntax_at(self.peek(), message)
    }

    fn syntax_at(&self, token: Token, message: impl Into<String>) -> FrontendDiagnostic {
        let detail = token.start.to_be_bytes();
        FrontendDiagnostic::new(
            DiagnosticCode::Syntax,
            message,
            token.line,
            token.column,
            &detail,
        )
    }

    fn limits(&self) -> RawInputLimits {
        self.meter.limits()
    }
}

#[derive(Clone, Copy)]
enum OperationTag {
    Insert,
    Remove,
    ReplaceBy,
}

fn aggregate_name(kind: AggregateKind) -> &'static str {
    match kind {
        AggregateKind::Count => "count",
        AggregateKind::Sum => "sum",
        AggregateKind::Min => "min",
        AggregateKind::Max => "max",
    }
}

fn valid_module_part(part: &str) -> bool {
    let mut bytes = part.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
