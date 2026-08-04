// Canonical semantic/operational encodings, bounded decoding, visibility
// rendering, and portable replay identity.

fn checked_len_add(left: usize, right: usize) -> OracleResult<usize> {
    left.checked_add(right)
        .ok_or("derivation.canonical_byte_limit")
}

fn encoded_text_len(value: &str) -> OracleResult<usize> {
    checked_len_add(8, value.len())
}

fn encoded_value_len(value: &FactValue) -> OracleResult<usize> {
    match value {
        FactValue::Int(_) | FactValue::Count(_) | FactValue::Entity(_) => Ok(9),
        FactValue::Text(value) => checked_len_add(1, encoded_text_len(value)?),
    }
}

fn encoded_fact_key_len(key: &FactKey) -> OracleResult<usize> {
    let mut length = checked_len_add(encoded_text_len(&key.relation)?, 8)?;
    for value in &key.tuple {
        length = checked_len_add(length, encoded_value_len(value)?)?;
    }
    Ok(length)
}

fn encoded_tuple_len(tuple: &[FactValue]) -> OracleResult<usize> {
    let mut length = 8;
    for value in tuple {
        length = checked_len_add(length, encoded_value_len(value)?)?;
    }
    Ok(length)
}

fn encoded_capability_set_len(capabilities: &BTreeSet<String>) -> OracleResult<usize> {
    let mut length = 8;
    for capability in capabilities {
        length = checked_len_add(length, encoded_text_len(capability)?)?;
    }
    Ok(length)
}

fn encoded_support_len(support: &SupportRef) -> OracleResult<usize> {
    let mut length = encoded_fact_key_len(support.key())?;
    length = checked_len_add(length, 1)?;
    length = checked_len_add(
        length,
        match support {
            SupportRef::Authoritative { .. } => 8,
            SupportRef::Derived { proof_id, .. } => encoded_text_len(proof_id)?,
        },
    )?;
    length = checked_len_add(
        length,
        encoded_capability_set_len(support.required_capabilities())?,
    )?;
    if matches!(support, SupportRef::Derived { .. }) {
        length = checked_len_add(length, 8)?;
    }
    Ok(length)
}

fn encoded_support_set_len(supports: &BTreeSet<SupportRef>) -> OracleResult<usize> {
    let mut length = 8;
    for support in supports {
        length = checked_len_add(length, encoded_support_len(support)?)?;
    }
    Ok(length)
}

fn encoded_combined_capabilities_len(supports: &BTreeSet<SupportRef>) -> OracleResult<usize> {
    let mut length = 8;
    for support in supports {
        for capability in support.required_capabilities() {
            // Duplicates deliberately overcharge: this is a conservative
            // pre-allocation quote for constructing the deduplicated set.
            length = checked_len_add(length, encoded_text_len(capability)?)?;
        }
    }
    Ok(length)
}

fn encoded_binding_map_len(bindings: &BTreeMap<String, FactValue>) -> OracleResult<usize> {
    let mut length = 8;
    for (name, value) in bindings {
        length = checked_len_add(length, encoded_text_len(name)?)?;
        length = checked_len_add(length, encoded_value_len(value)?)?;
    }
    Ok(length)
}

fn encoded_binding_state_len(state: &BindingState) -> OracleResult<usize> {
    checked_len_add(
        encoded_binding_map_len(&state.bindings)?,
        encoded_support_set_len(&state.supports)?,
    )
}

fn encoded_authoritative_row_len(assertion: &FactAssertion) -> OracleResult<usize> {
    let mut length = encoded_fact_key_len(&assertion.key)?;
    length = checked_len_add(length, encoded_tuple_len(&assertion.key.tuple)?)?;
    length = checked_len_add(length, 8)?;
    checked_len_add(
        length,
        encoded_capability_set_len(&assertion.required_capabilities)?,
    )
}

fn encoded_derived_row_len(key: &FactKey, proof: &ProofAlternative) -> OracleResult<usize> {
    let mut length = encoded_fact_key_len(key)?;
    length = checked_len_add(length, encoded_tuple_len(&key.tuple)?)?;
    length = checked_len_add(length, proof.canonical_len()?)?;
    // SHA-256 proof IDs are rendered as 64 lowercase hexadecimal bytes.
    length = checked_len_add(length, 8 + 64)?;
    length = checked_len_add(
        length,
        encoded_capability_set_len(&proof.required_capabilities)?,
    )?;
    checked_len_add(length, 8)
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_text(out: &mut Vec<u8>, value: &str) {
    write_u64(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn encode_value(out: &mut Vec<u8>, value: &FactValue) {
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

fn encode_fact_key(out: &mut Vec<u8>, key: &FactKey) {
    write_text(out, &key.relation);
    write_u64(out, key.tuple.len() as u64);
    for value in &key.tuple {
        encode_value(out, value);
    }
}

fn semantic_relation_bytes(store: &RelationStore) -> Vec<u8> {
    let mut out = b"rfc0003.semantic.v1".to_vec();
    write_u64(&mut out, store.assertions.len() as u64);
    for key in store.assertions.keys() {
        encode_fact_key(&mut out, key);
    }
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OperationalRelationState {
    entity_allocator: EntityTable,
    assertion_allocator_next: u64,
    components: BTreeMap<(EntityRef, String), FactValue>,
    assertions: BTreeMap<FactKey, FactAssertion>,
}

impl OperationalRelationState {
    fn capture(model: &WorldModel) -> Self {
        Self {
            entity_allocator: model.entities.clone(),
            assertion_allocator_next: model.relations.next_assertion_id,
            components: model.components.clone(),
            assertions: model.relations.assertions.clone(),
        }
    }

    fn restore(&self, schemas: BTreeMap<String, RelationSchema>) -> WorldModel {
        WorldModel {
            entities: self.entity_allocator.clone(),
            components: self.components.clone(),
            relations: RelationStore {
                schemas,
                assertions: self.assertions.clone(),
                next_assertion_id: self.assertion_allocator_next,
                last_changes: Vec::new(),
            },
        }
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = b"rfc0003.operational.v3".to_vec();
        write_u64(&mut out, self.entity_allocator.next_slot as u64);
        out.push(u8::from(self.entity_allocator.fresh_slots_exhausted));
        write_u64(&mut out, self.entity_allocator.generations.len() as u64);
        for (slot, generation) in &self.entity_allocator.generations {
            out.extend_from_slice(&slot.to_be_bytes());
            out.extend_from_slice(&generation.to_be_bytes());
        }
        write_u64(&mut out, self.entity_allocator.live.len() as u64);
        for entity in &self.entity_allocator.live {
            out.extend_from_slice(&entity.slot.to_be_bytes());
            out.extend_from_slice(&entity.generation.to_be_bytes());
        }
        write_u64(&mut out, self.entity_allocator.free_slots.len() as u64);
        for slot in &self.entity_allocator.free_slots {
            out.extend_from_slice(&slot.to_be_bytes());
        }
        write_u64(&mut out, self.entity_allocator.retired_slots.len() as u64);
        for slot in &self.entity_allocator.retired_slots {
            out.extend_from_slice(&slot.to_be_bytes());
        }
        write_u64(&mut out, self.assertion_allocator_next);
        write_u64(&mut out, self.components.len() as u64);
        for ((entity, component), value) in &self.components {
            out.extend_from_slice(&entity.slot.to_be_bytes());
            out.extend_from_slice(&entity.generation.to_be_bytes());
            write_text(&mut out, component);
            encode_value(&mut out, value);
        }
        write_u64(&mut out, self.assertions.len() as u64);
        for assertion in self.assertions.values() {
            encode_fact_key(&mut out, &assertion.key);
            write_u64(&mut out, assertion.id);
            write_u64(&mut out, assertion.causes.len() as u64);
            for cause in &assertion.causes {
                write_text(&mut out, cause);
            }
            write_u64(&mut out, assertion.required_capabilities.len() as u64);
            for capability in &assertion.required_capabilities {
                write_text(&mut out, capability);
            }
        }
        out
    }
}

fn operational_checkpoint_bytes(model: &WorldModel) -> Vec<u8> {
    OperationalRelationState::capture(model).canonical_bytes()
}

fn canonical_derivation_bytes(derived: &DerivationResult) -> Vec<u8> {
    let mut out = b"rfc0003.derivation.v1".to_vec();
    write_u64(&mut out, derived.len() as u64);
    for (fact, proofs) in derived {
        encode_fact_key(&mut out, fact);
        write_u64(&mut out, proofs.len() as u64);
        for proof in proofs {
            let proof = proof.canonical_bytes();
            write_u64(&mut out, proof.len() as u64);
            out.extend_from_slice(&proof);
        }
    }
    out
}

fn derivation_checkpoint_bytes(derived: &DerivationResult) -> Vec<u8> {
    canonical_derivation_bytes(derived)
}

fn portable_checkpoint_bytes(model: &WorldModel, derived: &DerivationResult) -> Vec<u8> {
    let mut out = b"rfc0003.portable.v1".to_vec();
    let world = operational_checkpoint_bytes(model);
    write_u64(&mut out, world.len() as u64);
    out.extend_from_slice(&world);
    let proofs = derivation_checkpoint_bytes(derived);
    write_u64(&mut out, proofs.len() as u64);
    out.extend_from_slice(&proofs);
    out
}

fn read_u64(input: &mut &[u8]) -> OracleResult<u64> {
    let bytes = input.get(..8).ok_or("wire.truncated")?;
    *input = &input[8..];
    Ok(u64::from_be_bytes(bytes.try_into().expect("eight bytes")))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DecodeLimits {
    max_input_bytes: usize,
    max_facts: usize,
    max_values: usize,
    max_text_bytes: usize,
    max_structural_bytes: usize,
}

impl DecodeLimits {
    fn generous() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_facts: 65_536,
            max_values: 1_048_576,
            max_text_bytes: 8 * 1024 * 1024,
            max_structural_bytes: 64 * 1024 * 1024,
        }
    }
}

struct DecodeMeter {
    limits: DecodeLimits,
    values: usize,
    text_bytes: usize,
    structural_bytes: usize,
}

impl DecodeMeter {
    fn new(input_bytes: usize, limits: DecodeLimits) -> OracleResult<Self> {
        if input_bytes > limits.max_input_bytes {
            return Err("wire.input_byte_limit");
        }
        Ok(Self {
            limits,
            values: 0,
            text_bytes: 0,
            structural_bytes: 0,
        })
    }

    fn charge_structural_bytes(&mut self, bytes: usize) -> OracleResult<()> {
        self.structural_bytes = self
            .structural_bytes
            .checked_add(bytes)
            .ok_or("wire.structural_byte_limit")?;
        if self.structural_bytes > self.limits.max_structural_bytes {
            return Err("wire.structural_byte_limit");
        }
        Ok(())
    }

    fn charge_text(&mut self, bytes: usize) -> OracleResult<()> {
        self.text_bytes = self
            .text_bytes
            .checked_add(bytes)
            .ok_or("wire.text_byte_limit")?;
        if self.text_bytes > self.limits.max_text_bytes {
            return Err("wire.text_byte_limit");
        }
        self.charge_structural_bytes(
            std::mem::size_of::<String>()
                .checked_add(bytes)
                .ok_or("wire.structural_byte_limit")?,
        )
    }

    fn charge_values(&mut self, count: usize) -> OracleResult<()> {
        self.values = self.values.checked_add(count).ok_or("wire.value_limit")?;
        if self.values > self.limits.max_values {
            return Err("wire.value_limit");
        }
        self.charge_structural_bytes(
            std::mem::size_of::<FactValue>()
                .checked_mul(count)
                .ok_or("wire.structural_byte_limit")?,
        )
    }
}

fn read_text(input: &mut &[u8], meter: &mut DecodeMeter) -> OracleResult<String> {
    let len = usize::try_from(read_u64(input)?).map_err(|_| "wire.length")?;
    let bytes = input.get(..len).ok_or("wire.truncated")?;
    meter.charge_text(len)?;
    *input = &input[len..];
    String::from_utf8(bytes.to_vec()).map_err(|_| "wire.utf8")
}

fn decode_value(input: &mut &[u8], meter: &mut DecodeMeter) -> OracleResult<FactValue> {
    let tag = *input.first().ok_or("wire.truncated")?;
    *input = &input[1..];
    match tag {
        b'i' => {
            let bytes = input.get(..8).ok_or("wire.truncated")?;
            *input = &input[8..];
            Ok(FactValue::Int(i64::from_be_bytes(
                bytes.try_into().expect("eight bytes"),
            )))
        }
        b'c' => Ok(FactValue::Count(read_u64(input)?)),
        b'e' => {
            let slot = input.get(..4).ok_or("wire.truncated")?;
            let generation = input.get(4..8).ok_or("wire.truncated")?;
            *input = &input[8..];
            Ok(FactValue::Entity(EntityRef {
                slot: u32::from_be_bytes(slot.try_into().expect("four bytes")),
                generation: u32::from_be_bytes(generation.try_into().expect("four bytes")),
            }))
        }
        b't' => Ok(FactValue::Text(read_text(input, meter)?)),
        _ => Err("wire.tag"),
    }
}

fn fact_key_bytes(key: &FactKey) -> Vec<u8> {
    let mut out = Vec::new();
    encode_fact_key(&mut out, key);
    out
}

fn decode_fact_key_from(input: &mut &[u8], meter: &mut DecodeMeter) -> OracleResult<FactKey> {
    let relation = read_text(input, meter)?;
    let count = usize::try_from(read_u64(input)?).map_err(|_| "wire.length")?;
    meter.charge_values(count)?;
    let tuple = (0..count)
        .map(|_| decode_value(input, meter))
        .collect::<OracleResult<Vec<_>>>()?;
    Ok(FactKey { relation, tuple })
}

fn decode_fact_key(mut input: &[u8]) -> OracleResult<FactKey> {
    let mut meter = DecodeMeter::new(input.len(), DecodeLimits::generous())?;
    let key = decode_fact_key_from(&mut input, &mut meter)?;
    if !input.is_empty() {
        return Err("wire.trailing");
    }
    Ok(key)
}

fn decode_semantic_relation_bytes(
    mut input: &[u8],
    schemas: &BTreeMap<String, RelationSchema>,
    entities: &EntityTable,
    limits: DecodeLimits,
) -> OracleResult<BTreeSet<FactKey>> {
    let mut meter = DecodeMeter::new(input.len(), limits)?;
    const DOMAIN: &[u8] = b"rfc0003.semantic.v1";
    if input.get(..DOMAIN.len()) != Some(DOMAIN) {
        return Err("wire.domain");
    }
    input = &input[DOMAIN.len()..];
    let count = usize::try_from(read_u64(&mut input)?).map_err(|_| "wire.length")?;
    if count > limits.max_facts {
        return Err("wire.fact_limit");
    }
    meter.charge_structural_bytes(
        std::mem::size_of::<FactKey>()
            .checked_mul(count)
            .ok_or("wire.structural_byte_limit")?,
    )?;
    let mut facts = BTreeSet::new();
    let mut previous = None;
    for _ in 0..count {
        let fact = decode_fact_key_from(&mut input, &mut meter)?;
        let schema = schemas.get(&fact.relation).ok_or("wire.unknown_relation")?;
        if fact.tuple.len() != schema.columns.len() {
            return Err("wire.relation_arity");
        }
        if fact
            .tuple
            .iter()
            .zip(&schema.columns)
            .any(|(value, column)| value.kind() != column.kind)
        {
            return Err("wire.relation_type");
        }
        if schema.canonical_tuple(fact.tuple.clone())? != fact.tuple {
            return Err("wire.noncanonical_tuple");
        }
        if fact
            .tuple
            .iter()
            .any(|value| matches!(value, FactValue::Entity(entity) if !entities.contains(*entity)))
        {
            return Err("wire.entity_not_live");
        }
        if previous.as_ref().is_some_and(|previous| previous >= &fact) {
            return Err("wire.noncanonical_fact_order");
        }
        previous = Some(fact.clone());
        facts.insert(fact);
    }
    if !input.is_empty() {
        return Err("wire.trailing");
    }
    Ok(facts)
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

fn render_visible(derived: &DerivationResult, capabilities: &BTreeSet<String>) -> Vec<String> {
    let mut rendered = Vec::new();
    for (fact, proofs) in derived {
        let visible = proofs
            .iter()
            .filter(|proof| proof.required_capabilities.is_subset(capabilities))
            .collect::<Vec<_>>();
        if visible.is_empty() {
            continue;
        }
        let mut bytes = fact_key_bytes(fact);
        for proof in visible {
            write_text(&mut bytes, &proof.identity());
        }
        rendered.push(hex(&bytes));
    }
    rendered
}

struct PortableAttempt {
    checkpoint: Vec<u8>,
}

fn replay_portable(
    model: &WorldModel,
    derived: &DerivationResult,
    attempt: &PortableAttempt,
    instructions_executed: &mut usize,
) -> OracleResult<()> {
    if portable_checkpoint_bytes(model, derived) != attempt.checkpoint {
        return Err("attempt.checkpoint_mismatch");
    }
    *instructions_executed += 1;
    Ok(())
}
