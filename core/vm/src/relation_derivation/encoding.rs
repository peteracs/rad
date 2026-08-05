use super::model::{BindingState, DerivationError, DerivationResult, ProofAlternative, SupportRef};
use crate::relation_runtime::{FactKey, FactValue};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn checked_add(left: usize, right: usize) -> DerivationResult<usize> {
    left.checked_add(right).ok_or_else(|| {
        DerivationError::new("derivation.canonical_byte_limit", "encoded length overflow")
    })
}

pub(crate) fn text_len(value: &str) -> DerivationResult<usize> {
    checked_add(8, value.len())
}

pub(crate) fn value_len(value: &FactValue) -> DerivationResult<usize> {
    match value {
        FactValue::Int(_) | FactValue::Count(_) | FactValue::Entity(_) => Ok(9),
        FactValue::Text(value) => checked_add(1, text_len(value)?),
    }
}

pub(crate) fn fact_len(key: &FactKey) -> DerivationResult<usize> {
    let mut length = checked_add(text_len(&key.relation)?, 8)?;
    for value in &key.tuple {
        length = checked_add(length, value_len(value)?)?;
    }
    Ok(length)
}

fn capability_set_len(capabilities: &BTreeSet<String>) -> DerivationResult<usize> {
    capabilities.iter().try_fold(8, |length, capability| {
        checked_add(length, text_len(capability)?)
    })
}

pub(crate) fn support_len(support: &SupportRef) -> DerivationResult<usize> {
    let mut length = checked_add(fact_len(support.key())?, 1)?;
    length = checked_add(
        length,
        match support {
            SupportRef::Authoritative { .. } => 8,
            SupportRef::Derived { proof_id, .. } => text_len(proof_id)?,
        },
    )?;
    length = checked_add(length, capability_set_len(support.required_capabilities())?)?;
    if matches!(support, SupportRef::Derived { .. }) {
        length = checked_add(length, 8)?;
    }
    Ok(length)
}

pub(crate) fn support_set_len(supports: &BTreeSet<SupportRef>) -> DerivationResult<usize> {
    supports.iter().try_fold(8, |length, support| {
        checked_add(length, support_len(support)?)
    })
}

pub(crate) fn combined_capabilities_len(
    supports: &BTreeSet<SupportRef>,
) -> DerivationResult<usize> {
    let mut length = 8;
    for support in supports {
        for capability in support.required_capabilities() {
            length = checked_add(length, text_len(capability)?)?;
        }
    }
    Ok(length)
}

pub(crate) fn binding_map_len(bindings: &BTreeMap<String, FactValue>) -> DerivationResult<usize> {
    let mut length = 8;
    for (name, value) in bindings {
        length = checked_add(length, text_len(name)?)?;
        length = checked_add(length, value_len(value)?)?;
    }
    Ok(length)
}

pub(crate) fn binding_state_len(state: &BindingState) -> DerivationResult<usize> {
    checked_add(
        binding_map_len(&state.bindings)?,
        support_set_len(&state.supports)?,
    )
}

pub(crate) fn proof_len(proof: &ProofAlternative) -> DerivationResult<usize> {
    let mut length = checked_add(text_len(&proof.rule)?, binding_map_len(&proof.bindings)?)?;
    length = checked_add(length, 1)?;
    if let Some(group) = &proof.aggregate_group {
        length = checked_add(length, 8)?;
        for value in group {
            length = checked_add(length, value_len(value)?)?;
        }
    }
    length = checked_add(length, 8)?;
    for support in &proof.supports {
        length = checked_add(length, fact_len(support.key())?)?;
        length = checked_add(length, 1)?;
        length = checked_add(
            length,
            match support {
                SupportRef::Authoritative { .. } => 8,
                SupportRef::Derived { proof_id, .. } => text_len(proof_id)?,
            },
        )?;
    }
    length = checked_add(length, capability_set_len(&proof.required_capabilities)?)?;
    checked_add(length, 8)
}

pub(crate) fn authoritative_row_len(
    key: &FactKey,
    capabilities: &BTreeSet<String>,
) -> DerivationResult<usize> {
    let mut length = checked_add(fact_len(key)?, 8)?;
    for value in &key.tuple {
        length = checked_add(length, value_len(value)?)?;
    }
    length = checked_add(length, 8)?;
    checked_add(length, capability_set_len(capabilities)?)
}

pub(crate) fn derived_row_len(key: &FactKey, proof: &ProofAlternative) -> DerivationResult<usize> {
    let mut length = checked_add(fact_len(key)?, 8)?;
    for value in &key.tuple {
        length = checked_add(length, value_len(value)?)?;
    }
    length = checked_add(length, proof_len(proof)?)?;
    length = checked_add(length, 8 + 64)?;
    length = checked_add(length, capability_set_len(&proof.required_capabilities)?)?;
    checked_add(length, 8)
}

pub(crate) fn proof_bytes(proof: &ProofAlternative) -> Vec<u8> {
    let mut out = Vec::new();
    write_text(&mut out, &proof.rule);
    write_u64(&mut out, proof.bindings.len() as u64);
    for (name, value) in &proof.bindings {
        write_text(&mut out, name);
        write_value(&mut out, value);
    }
    match &proof.aggregate_group {
        Some(group) => {
            out.push(1);
            write_u64(&mut out, group.len() as u64);
            for value in group {
                write_value(&mut out, value);
            }
        }
        None => out.push(0),
    }
    write_u64(&mut out, proof.supports.len() as u64);
    for support in &proof.supports {
        write_fact(&mut out, support.key());
        match support {
            SupportRef::Authoritative { assertion_id, .. } => {
                out.push(b'A');
                write_u64(&mut out, *assertion_id);
            }
            SupportRef::Derived { proof_id, .. } => {
                out.push(b'D');
                write_text(&mut out, proof_id);
            }
        }
    }
    write_u64(&mut out, proof.required_capabilities.len() as u64);
    for capability in &proof.required_capabilities {
        write_text(&mut out, capability);
    }
    write_u64(&mut out, proof.depth as u64);
    out
}

pub(crate) fn derivation_bytes(derived: &BTreeMap<FactKey, BTreeSet<ProofAlternative>>) -> Vec<u8> {
    let mut out = b"rfc0003.derivation.v1".to_vec();
    write_u64(&mut out, derived.len() as u64);
    for (fact, proofs) in derived {
        write_fact(&mut out, fact);
        write_u64(&mut out, proofs.len() as u64);
        for proof in proofs {
            let proof = proof_bytes(proof);
            write_u64(&mut out, proof.len() as u64);
            out.extend_from_slice(&proof);
        }
    }
    out
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_text(out: &mut Vec<u8>, value: &str) {
    write_u64(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn write_fact(out: &mut Vec<u8>, fact: &FactKey) {
    write_text(out, &fact.relation);
    write_u64(out, fact.tuple.len() as u64);
    for value in &fact.tuple {
        write_value(out, value);
    }
}

fn write_value(out: &mut Vec<u8>, value: &FactValue) {
    match value {
        FactValue::Int(value) => {
            out.push(b'i');
            out.extend_from_slice(&value.to_be_bytes());
        }
        FactValue::Count(value) => {
            out.push(b'c');
            write_u64(out, *value);
        }
        FactValue::Entity(entity) => {
            out.push(b'e');
            out.extend_from_slice(&entity.slot.to_be_bytes());
            out.extend_from_slice(&entity.generation.to_be_bytes());
        }
        FactValue::Text(value) => {
            out.push(b't');
            write_text(out, value);
        }
    }
}
