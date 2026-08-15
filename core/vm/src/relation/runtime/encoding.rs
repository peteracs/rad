use super::{AuthoritativeRelationState, FactAssertion, FactKey, FactValue, UniqueIndexKey};

pub(crate) fn fact_key_transport_hex(fact: &FactKey) -> String {
    let mut out = Vec::new();
    encode_fact(&mut out, fact);
    hex::encode(out)
}

pub(crate) fn fact_key_from_transport_hex(encoded: &str) -> super::RelationRuntimeResult<FactKey> {
    let bytes = hex::decode(encoded).map_err(|_| {
        super::RelationRuntimeError::new(
            "relation.transport_invalid_hex",
            "fact key is not valid hexadecimal",
        )
    })?;
    let mut input = Reader::new(&bytes);
    let fact = input.fact()?;
    input.finish()?;
    Ok(fact)
}

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
        self.encode_state_inventory(&mut out);
        out
    }

    fn encode_state_inventory(&self, out: &mut Vec<u8>) {
        put_u64(out, self.next_assertion_id());
        put_u64(out, self.assertions().len() as u64);
        for assertion in self.assertions().values() {
            encode_assertion(out, assertion);
        }
        put_u64(out, self.unique_indexes().len() as u64);
        for (index, fact) in self.unique_indexes() {
            encode_index(out, index);
            encode_fact(out, fact);
        }
    }

    pub fn semantic_content_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_bytes(&mut out, b"rad.authoritative-relation-content.v1");
        match self.manifest_digest() {
            None => out.push(0),
            Some(digest) => {
                out.push(1);
                out.extend_from_slice(&digest);
            }
        }
        put_u64(&mut out, self.assertions().len() as u64);
        for fact in self.assertions().keys() {
            encode_fact(&mut out, fact);
        }
        out
    }

    pub fn transport_hex(&self) -> Option<String> {
        self.manifest().map(|manifest| {
            let mut out = Vec::new();
            put_bytes(&mut out, b"rad.authoritative-relation-transport.v1");
            out.extend_from_slice(&manifest.digest());
            self.encode_state_inventory(&mut out);
            hex::encode(out)
        })
    }

    pub fn from_transport_hex(
        encoded: &str,
        expected_manifest: std::sync::Arc<super::RelationRuntimeManifest>,
    ) -> super::RelationRuntimeResult<Self> {
        let bytes = hex::decode(encoded).map_err(|_| {
            super::RelationRuntimeError::new(
                "relation.transport_invalid_hex",
                "relation state is not valid hexadecimal",
            )
        })?;
        decode_transport(&bytes, expected_manifest)
    }
}

fn decode_transport(
    bytes: &[u8],
    expected_manifest: std::sync::Arc<super::RelationRuntimeManifest>,
) -> super::RelationRuntimeResult<AuthoritativeRelationState> {
    let mut input = Reader::new(bytes);
    input.expect_bytes(b"rad.authoritative-relation-transport.v1")?;
    let digest = input.array::<32>()?;
    if digest != expected_manifest.digest() {
        return Err(super::RelationRuntimeError::new(
            "relation.transport_manifest_mismatch",
            "saved relation state belongs to a different sealed manifest",
        ));
    }
    let next_assertion_id = input.u64()?;
    let assertion_count = input.count()?;
    let mut assertions = std::collections::BTreeMap::new();
    for _ in 0..assertion_count {
        let assertion = input.assertion()?;
        if assertions
            .insert(assertion.fact_key.clone(), assertion)
            .is_some()
        {
            return Err(input.error("duplicate authoritative fact"));
        }
    }
    let index_count = input.count()?;
    let mut indexes = std::collections::BTreeMap::new();
    for _ in 0..index_count {
        let index = input.index()?;
        let fact = input.fact()?;
        if indexes.insert(index, fact).is_some() {
            return Err(input.error("duplicate unique index entry"));
        }
    }
    input.finish()?;
    AuthoritativeRelationState::from_transport_parts(
        expected_manifest,
        assertions,
        indexes,
        next_assertion_id,
    )
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn error(&self, detail: impl Into<String>) -> super::RelationRuntimeError {
        super::RelationRuntimeError::new("relation.transport_invalid", detail)
    }

    fn take(&mut self, count: usize) -> super::RelationRuntimeResult<&'a [u8]> {
        let end = self
            .cursor
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| self.error("truncated relation state"))?;
        let value = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(value)
    }

    fn byte(&mut self) -> super::RelationRuntimeResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn array<const N: usize>(&mut self) -> super::RelationRuntimeResult<[u8; N]> {
        self.take(N)?
            .try_into()
            .map_err(|_| self.error("invalid fixed-width value"))
    }

    fn u32(&mut self) -> super::RelationRuntimeResult<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> super::RelationRuntimeResult<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn count(&mut self) -> super::RelationRuntimeResult<usize> {
        usize::try_from(self.u64()?).map_err(|_| self.error("collection length is too large"))
    }

    fn bytes(&mut self) -> super::RelationRuntimeResult<&'a [u8]> {
        let count = self.count()?;
        self.take(count)
    }

    fn expect_bytes(&mut self, expected: &[u8]) -> super::RelationRuntimeResult<()> {
        if self.bytes()? == expected {
            Ok(())
        } else {
            Err(self.error("unsupported relation-state version"))
        }
    }

    fn text(&mut self) -> super::RelationRuntimeResult<String> {
        String::from_utf8(self.bytes()?.to_vec())
            .map_err(|_| self.error("relation text is not UTF-8"))
    }

    fn strings(&mut self) -> super::RelationRuntimeResult<std::collections::BTreeSet<String>> {
        let count = self.count()?;
        let mut values = std::collections::BTreeSet::new();
        for _ in 0..count {
            if !values.insert(self.text()?) {
                return Err(self.error("duplicate metadata string"));
            }
        }
        Ok(values)
    }

    fn value(&mut self) -> super::RelationRuntimeResult<FactValue> {
        Ok(match self.byte()? {
            0 => FactValue::Entity(super::EntityRef {
                slot: self.u32()?,
                generation: self.u32()?,
            }),
            1 => FactValue::Int(i64::from_be_bytes(self.array()?)),
            2 => FactValue::Count(self.u64()?),
            3 => FactValue::Text(self.text()?),
            _ => return Err(self.error("unknown relation value variant")),
        })
    }

    fn values(&mut self) -> super::RelationRuntimeResult<Vec<FactValue>> {
        let count = self.count()?;
        (0..count).map(|_| self.value()).collect()
    }

    fn fact(&mut self) -> super::RelationRuntimeResult<FactKey> {
        Ok(FactKey::new(self.text()?, self.values()?))
    }

    fn assertion(&mut self) -> super::RelationRuntimeResult<FactAssertion> {
        Ok(FactAssertion {
            assertion_id: self.u64()?,
            fact_key: self.fact()?,
            causes: self.strings()?,
            required_capabilities: self.strings()?,
        })
    }

    fn index(&mut self) -> super::RelationRuntimeResult<UniqueIndexKey> {
        Ok(UniqueIndexKey {
            relation: self.text()?,
            constraint: self.text()?,
            values: self.values()?,
        })
    }

    fn finish(&self) -> super::RelationRuntimeResult<()> {
        if self.cursor == self.bytes.len() {
            Ok(())
        } else {
            Err(self.error("trailing relation-state bytes"))
        }
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
