use super::{AuthoritativeRelationState, FactAssertion, FactKey, FactValue, UniqueIndexKey};

impl AuthoritativeRelationState {
    /// Canonical inventory of every future-determining authoritative relation
    /// field. `WorldSnapshot` stores this exact state object and delegates its
    /// operational identity to these bytes, so restoration and hashing cannot
    /// acquire separate hand-maintained field lists.
    pub fn operational_checkpoint_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_bytes(&mut out, b"rad.authoritative-relation-state.v1");
        match self.manifest() {
            None => out.push(0),
            Some(manifest) => {
                out.push(1);
                put_bytes(&mut out, manifest.canonical_bytes());
                out.extend_from_slice(&manifest.digest());
            }
        }
        put_u64(&mut out, self.next_assertion_id());
        put_u64(&mut out, self.assertions().len() as u64);
        for assertion in self.assertions().values() {
            encode_assertion(&mut out, assertion);
        }
        put_u64(&mut out, self.unique_indexes().len() as u64);
        for (index, fact) in self.unique_indexes() {
            encode_index(&mut out, index);
            encode_fact(&mut out, fact);
        }
        out
    }
}

fn encode_assertion(out: &mut Vec<u8>, assertion: &FactAssertion) {
    put_u64(out, assertion.assertion_id);
    encode_fact(out, &assertion.fact_key);
    put_strings(out, &assertion.causes);
    put_strings(out, &assertion.required_capabilities);
}

fn encode_index(out: &mut Vec<u8>, index: &UniqueIndexKey) {
    put_text(out, &index.relation);
    put_text(out, &index.constraint);
    put_u64(out, index.values.len() as u64);
    for value in &index.values {
        encode_value(out, value);
    }
}

fn encode_fact(out: &mut Vec<u8>, fact: &FactKey) {
    put_text(out, &fact.relation);
    put_u64(out, fact.tuple.len() as u64);
    for value in &fact.tuple {
        encode_value(out, value);
    }
}

fn encode_value(out: &mut Vec<u8>, value: &FactValue) {
    match value {
        FactValue::Entity(entity) => {
            out.push(0);
            put_u32(out, entity.slot);
            put_u32(out, entity.generation);
        }
        FactValue::Int(value) => {
            out.push(1);
            out.extend_from_slice(&value.to_be_bytes());
        }
        FactValue::Count(value) => {
            out.push(2);
            put_u64(out, *value);
        }
        FactValue::Text(value) => {
            out.push(3);
            put_text(out, value);
        }
    }
}

fn put_strings(out: &mut Vec<u8>, values: &std::collections::BTreeSet<String>) {
    put_u64(out, values.len() as u64);
    for value in values {
        put_text(out, value);
    }
}

fn put_text(out: &mut Vec<u8>, value: &str) {
    put_bytes(out, value.as_bytes());
}

fn put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    put_u64(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}
