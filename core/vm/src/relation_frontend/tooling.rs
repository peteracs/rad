use super::ast::*;
use super::{FrontendArtifacts, FrontendSymbol, FrontendSymbolKind};

pub(crate) fn format_program(program: &BoundedRawProgram) -> String {
    let mut output = String::new();
    let module = program.module_ids.first().map(String::as_str).unwrap_or("");
    for schema in &program.relations {
        output.push_str("relation ");
        output.push_str(display_identity(&schema.identity, module));
        output.push('(');
        for (index, column) in schema.columns.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            output.push_str(&column.name);
            output.push_str(": ");
            output.push_str(type_name(column.value_type));
            if column.value_type == RelationType::Entity
                && column.on_delete == Some(OnDelete::Cascade)
            {
                output.push_str(" on delete cascade");
            }
        }
        output.push_str(")\n");
        let mut unique = schema.unique.clone();
        unique.sort();
        for constraint in unique {
            output.push_str("    unique ");
            output.push_str(&constraint.columns.join(", "));
            let default_name = constraint.columns.join("_");
            if constraint.name != default_name {
                output.push_str(" as ");
                output.push_str(&constraint.name);
            }
            output.push('\n');
        }
        if schema.symmetric {
            output.push_str("    symmetric\n");
        }
        output.push('\n');
    }
    for rule in &program.rules {
        format_rule(&mut output, &rule.ast, module);
        output.push('\n');
    }
    let mut operations = program.operations.clone();
    operations.sort();
    for operation in &operations {
        format_operation(&mut output, operation, module);
        output.push('\n');
    }
    output
}

pub(crate) fn symbols(artifacts: &FrontendArtifacts) -> Vec<FrontendSymbol> {
    let mut output = artifacts
        .relations
        .schemas()
        .iter()
        .map(|schema| FrontendSymbol {
            identity: schema.identity.clone(),
            kind: match schema.kind {
                RelationKind::Authoritative => FrontendSymbolKind::AuthoritativeRelation,
                RelationKind::Derived => FrontendSymbolKind::DerivedRelation,
            },
        })
        .collect::<Vec<_>>();
    output.extend(artifacts.rules.iter().map(|rule| FrontendSymbol {
        identity: rule.identity().to_string(),
        kind: FrontendSymbolKind::Rule,
    }));
    output.sort_by(|left, right| {
        (&left.identity, symbol_tag(left.kind)).cmp(&(&right.identity, symbol_tag(right.kind)))
    });
    output
}

fn format_rule(output: &mut String, rule: &RawRuleAst, module: &str) {
    output.push_str("derive ");
    output.push_str(display_identity(&rule.head_relation, module));
    output.push('(');
    for (index, term) in rule.head.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        if let (RawTerm::Variable(name), Some(aggregate)) = (term, &rule.aggregate) {
            if name == &aggregate.output {
                output.push_str(aggregate_name(aggregate.kind));
                output.push('(');
                if let Some(input) = &aggregate.input {
                    output.push_str(input);
                }
                output.push(')');
                continue;
            }
        }
        format_term(output, term);
    }
    output.push_str(")\n");
    for (index, atom) in rule.atoms.iter().enumerate() {
        output.push_str(if index == 0 { "    when " } else { "    and " });
        output.push_str(display_identity(&atom.relation, module));
        output.push('(');
        for (term_index, term) in atom.terms.iter().enumerate() {
            if term_index != 0 {
                output.push_str(", ");
            }
            format_term(output, term);
        }
        output.push_str(")\n");
    }
    for predicate in &rule.predicates {
        let RawPredicate::Greater(left, right) = predicate;
        output.push_str("    and ");
        output.push_str(left);
        output.push_str(" > ");
        output.push_str(right);
        output.push('\n');
    }
}

fn format_operation(output: &mut String, operation: &RelationOperation, module: &str) {
    match &operation.kind {
        RelationOperationKind::Insert => output.push_str("Insert("),
        RelationOperationKind::Remove => output.push_str("Remove("),
        RelationOperationKind::ReplaceBy { .. } => output.push_str("ReplaceBy("),
    }
    output.push_str(display_identity(&operation.relation, module));
    output.push_str(", ");
    if let RelationOperationKind::ReplaceBy { constraint, key } = &operation.kind {
        output.push_str(constraint);
        output.push_str(", ");
        if key.len() != 1 {
            output.push('(');
        }
        for (index, value) in key.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            format_operation_value(output, value);
        }
        if key.len() != 1 {
            output.push(')');
        }
        output.push_str(", ");
    }
    output.push('(');
    for (index, value) in operation.tuple.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        format_operation_value(output, value);
    }
    output.push_str("))");
}

fn format_term(output: &mut String, term: &RawTerm) {
    match term {
        RawTerm::Variable(name) => output.push_str(name),
        RawTerm::Literal(value) => format_literal(output, value),
    }
}

fn format_operation_value(output: &mut String, value: &RawOperationValue) {
    match value {
        RawOperationValue::EntitySymbol(name) => output.push_str(name),
        RawOperationValue::Literal(value) => format_literal(output, value),
    }
}

fn format_literal(output: &mut String, value: &Literal) {
    match value {
        Literal::Int(value) => output.push_str(&value.to_string()),
        Literal::Count(value) => output.push_str(&value.to_string()),
        Literal::Text(value) => {
            output.push('"');
            output.push_str(&value.replace('\\', "\\\\").replace('"', "\\\""));
            output.push('"');
        }
    }
}

fn display_identity<'a>(identity: &'a str, module: &str) -> &'a str {
    identity
        .strip_prefix(module)
        .and_then(|identity| identity.strip_prefix("::"))
        .unwrap_or(identity)
}

fn type_name(value: RelationType) -> &'static str {
    match value {
        RelationType::Entity => "entity",
        RelationType::Int => "int",
        RelationType::Count => "count",
        RelationType::Text => "str",
    }
}

fn aggregate_name(kind: AggregateKind) -> &'static str {
    match kind {
        AggregateKind::Count => "count",
        AggregateKind::Sum => "sum",
        AggregateKind::Min => "min",
        AggregateKind::Max => "max",
    }
}

fn symbol_tag(kind: FrontendSymbolKind) -> u8 {
    match kind {
        FrontendSymbolKind::AuthoritativeRelation => 0,
        FrontendSymbolKind::DerivedRelation => 1,
        FrontendSymbolKind::Rule => 2,
    }
}
