use super::ast::*;
use sha2::{Digest, Sha256};

pub(crate) fn raw_rule_bytes(rule: &RawRuleAst) -> Vec<u8> {
    let mut out = Vec::new();
    text(&mut out, "rad.raw-relation-rule.v1");
    optional_text(&mut out, rule.explicit_id.as_deref());
    text(&mut out, &rule.head_relation);
    terms(&mut out, &rule.head);

    let mut atoms = rule.atoms.iter().map(atom_bytes).collect::<Vec<_>>();
    framed_set(&mut out, &mut atoms);
    let mut predicates = rule
        .predicates
        .iter()
        .map(predicate_bytes)
        .collect::<Vec<_>>();
    framed_set(&mut out, &mut predicates);
    match &rule.aggregate {
        None => out.push(0),
        Some(aggregate) => {
            out.push(1);
            out.push(aggregate.kind.tag());
            optional_text(&mut out, aggregate.input.as_deref());
            text(&mut out, &aggregate.output);
            let mut groups = aggregate.group_by.clone();
            groups.sort();
            groups.dedup();
            u64_value(&mut out, groups.len() as u64);
            for group in groups {
                text(&mut out, &group);
            }
        }
    }
    out
}

pub(crate) fn sealed_rule_bytes(
    identity: &str,
    rule: &RawRuleAst,
    inferred_head: &RelationSchema,
) -> Vec<u8> {
    let mut out = Vec::new();
    text(&mut out, "rad.sealed-relation-rule.v2");
    text(&mut out, identity);
    bytes(&mut out, &raw_rule_bytes(rule));
    schema(&mut out, inferred_head);
    out
}

pub(crate) fn schema_bytes(value: &RelationSchema) -> Vec<u8> {
    let mut out = Vec::new();
    schema(&mut out, value);
    out
}

pub(crate) fn operation_bytes(value: &RelationOperation) -> Vec<u8> {
    let mut out = Vec::new();
    text(&mut out, &value.owner);
    text(&mut out, &value.relation);
    match &value.kind {
        RelationOperationKind::Insert => out.push(b'i'),
        RelationOperationKind::Remove => out.push(b'r'),
        RelationOperationKind::ReplaceBy { constraint, key } => {
            out.push(b'x');
            text(&mut out, constraint);
            u64_value(&mut out, key.len() as u64);
            for value in key {
                operation_value(&mut out, value);
            }
        }
    }
    u64_value(&mut out, value.tuple.len() as u64);
    for item in &value.tuple {
        operation_value(&mut out, item);
    }
    out
}

pub(crate) fn manifest_digest(
    modules: &[String],
    schemas: &[RelationSchema],
    rules: &[std::sync::Arc<SealedRulePlan>],
    edges: &[(String, String)],
    operations: &[RelationOperation],
) -> [u8; 32] {
    let mut out = Vec::new();
    text(&mut out, "rad.relation-frontend-manifest.v2");
    let mut module_encodings = modules
        .iter()
        .map(|module| {
            let mut encoded = Vec::new();
            text(&mut encoded, module);
            encoded
        })
        .collect::<Vec<_>>();
    framed_set(&mut out, &mut module_encodings);
    let mut schema_encodings = schemas.iter().map(schema_bytes).collect::<Vec<_>>();
    framed_set(&mut out, &mut schema_encodings);
    let mut rule_encodings = rules
        .iter()
        .map(|rule| rule.canonical_bytes().to_vec())
        .collect::<Vec<_>>();
    framed_set(&mut out, &mut rule_encodings);
    let mut edge_encodings = edges
        .iter()
        .map(|(source, target)| {
            let mut edge = Vec::new();
            text(&mut edge, source);
            text(&mut edge, target);
            edge
        })
        .collect::<Vec<_>>();
    framed_set(&mut out, &mut edge_encodings);
    let mut operation_encodings = operations.iter().map(operation_bytes).collect::<Vec<_>>();
    framed_set(&mut out, &mut operation_encodings);
    Sha256::digest(out).into()
}

pub(crate) fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn schema(out: &mut Vec<u8>, value: &RelationSchema) {
    text(out, &value.identity);
    text(out, &value.owner);
    out.push(value.kind.tag());
    out.push(u8::from(value.symmetric));
    u64_value(out, value.columns.len() as u64);
    for column in &value.columns {
        text(out, &column.name);
        out.push(column.value_type.tag());
        out.push(match column.on_delete {
            None => 0,
            Some(OnDelete::Restrict) => b'r',
            Some(OnDelete::Cascade) => b'c',
        });
    }
    let mut unique = value.unique.clone();
    unique.sort();
    u64_value(out, unique.len() as u64);
    for constraint in unique {
        text(out, &constraint.name);
        u64_value(out, constraint.columns.len() as u64);
        for column in constraint.columns {
            text(out, &column);
        }
    }
}

pub(super) fn atom_bytes(atom: &RawAtom) -> Vec<u8> {
    let mut out = Vec::new();
    text(&mut out, &atom.relation);
    terms(&mut out, &atom.terms);
    out
}

pub(super) fn predicate_bytes(predicate: &RawPredicate) -> Vec<u8> {
    let mut out = Vec::new();
    match predicate {
        RawPredicate::Greater(left, right) => {
            out.push(b'g');
            text(&mut out, left);
            text(&mut out, right);
        }
    }
    out
}

fn terms(out: &mut Vec<u8>, values: &[RawTerm]) {
    u64_value(out, values.len() as u64);
    for value in values {
        match value {
            RawTerm::Variable(name) => {
                out.push(b'v');
                text(out, name);
            }
            RawTerm::Literal(value) => {
                out.push(b'l');
                literal(out, value);
            }
        }
    }
}

fn operation_value(out: &mut Vec<u8>, value: &RawOperationValue) {
    match value {
        RawOperationValue::EntitySymbol(name) => {
            out.push(b'e');
            text(out, name);
        }
        RawOperationValue::Literal(value) => {
            out.push(b'l');
            literal(out, value);
        }
    }
}

fn literal(out: &mut Vec<u8>, value: &Literal) {
    match value {
        Literal::Int(value) => {
            out.push(b'i');
            out.extend_from_slice(&value.to_be_bytes());
        }
        Literal::Count(value) => {
            out.push(b'c');
            out.extend_from_slice(&value.to_be_bytes());
        }
        Literal::Text(value) => {
            out.push(b't');
            text(out, value);
        }
    }
}

fn framed_set(out: &mut Vec<u8>, values: &mut [Vec<u8>]) {
    values.sort();
    u64_value(out, values.len() as u64);
    for value in values {
        bytes(out, value);
    }
}

fn optional_text(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            text(out, value);
        }
    }
}

fn bytes(out: &mut Vec<u8>, value: &[u8]) {
    u64_value(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn text(out: &mut Vec<u8>, value: &str) {
    bytes(out, value.as_bytes());
}

fn u64_value(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}
