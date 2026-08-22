// Deterministic commitments for records evicted from the bounded ledger.

struct TruncationHasher(blake3::Hasher);

impl TruncationHasher {
    fn new(previous: &[u8; 32], previous_count: u64, kind: u8) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"rad/provenance-truncation/v1");
        hasher.update(previous);
        hasher.update(&previous_count.to_le_bytes());
        hasher.update(&[kind]);
        Self(hasher)
    }

    fn byte(&mut self, value: u8) {
        self.0.update(&[value]);
    }

    fn u32(&mut self, value: u32) {
        self.0.update(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.update(&value.to_le_bytes());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.u64(value.len() as u64);
        self.0.update(value);
    }

    fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn optional_text(&mut self, value: Option<&str>) {
        self.byte(u8::from(value.is_some()));
        if let Some(value) = value {
            self.text(value);
        }
    }

    fn optional_u64(&mut self, value: Option<u64>) {
        self.byte(u8::from(value.is_some()));
        if let Some(value) = value {
            self.u64(value);
        }
    }

    fn finish(self) -> [u8; 32] {
        *self.0.finalize().as_bytes()
    }
}

fn hash_cause(hasher: &mut TruncationHasher, cause: &Cause) {
    match cause {
        Cause::Main => hasher.byte(0),
        Cause::System { name } => {
            hasher.byte(1);
            hasher.text(name);
        }
        Cause::Handler { event, emit_id } => {
            hasher.byte(2);
            hasher.text(event);
            hasher.u64(*emit_id);
        }
        Cause::Transaction { name, parent } => {
            hasher.byte(3);
            hasher.text(name);
            hash_cause(hasher, parent);
        }
        Cause::HostCall {
            extension,
            generation,
            plugin_digest,
            export,
            input_digest,
            output_digest,
            parent,
        } => {
            hasher.byte(4);
            hasher.text(extension);
            hasher.text(generation);
            hasher.text(plugin_digest);
            hasher.text(export);
            hasher.text(input_digest);
            hasher.text(output_digest);
            hash_cause(hasher, parent);
        }
    }
}

fn hash_scalar(hasher: &mut TruncationHasher, value: &CausalScalar) {
    match value {
        CausalScalar::Nil => hasher.byte(0),
        CausalScalar::Bool(value) => {
            hasher.byte(1);
            hasher.byte(u8::from(*value));
        }
        CausalScalar::Int(value) => {
            hasher.byte(2);
            hasher.0.update(&value.to_le_bytes());
        }
        CausalScalar::Float(bits) => {
            hasher.byte(3);
            hasher.u64(*bits);
        }
        CausalScalar::Entity(entity) => {
            hasher.byte(4);
            hasher.u32(*entity);
        }
        CausalScalar::Text(value) => {
            hasher.byte(5);
            hasher.text(value);
        }
    }
}

impl CausalityLedger {
    fn absorb_evicted(
        &mut self,
        kind: u8,
        record_count: u64,
        encode: impl FnOnce(&mut TruncationHasher),
    ) {
        if record_count == 0 {
            return;
        }
        let mut hasher = TruncationHasher::new(
            &self.truncation.digest,
            self.truncation.evicted_records,
            kind,
        );
        hasher.u64(record_count);
        encode(&mut hasher);
        self.truncation.digest = hasher.finish();
        self.truncation.evicted_records = self
            .truncation
            .evicted_records
            .saturating_add(record_count);
    }

    fn absorb_evicted_write(&mut self, record: &WriteRecord) {
        self.absorb_evicted(0, 1, |hasher| {
            hasher.u64(record.frame);
            hasher.byte(u8::from(record.entity.is_some()));
            if let Some(entity) = record.entity {
                hasher.u32(entity);
            }
            hasher.optional_text(record.entity_name.as_deref());
            hasher.text(&record.component);
            match &record.summary {
                WriteSummary::Full { value, fields } => {
                    hasher.byte(0);
                    hasher.text(value);
                    hasher.u64(fields.len() as u64);
                    for (field, value) in fields {
                        hasher.text(field);
                        hash_scalar(hasher, value);
                    }
                }
                WriteSummary::Field((field, value)) => {
                    hasher.byte(1);
                    hasher.text(field);
                    hash_scalar(hasher, value);
                }
            }
            hasher.byte(match record.kind {
                WriteKind::Set => 0,
                WriteKind::Spawn => 1,
                WriteKind::Despawn => 2,
                WriteKind::Remove => 3,
                WriteKind::Resource => 4,
            });
            hash_cause(hasher, &record.by);
            hasher.optional_text(record.origin.as_deref());
            hasher.optional_u64(record.resolution_id);
        });
    }

    fn absorb_evicted_emit(&mut self, record: &EmitRecord) {
        self.absorb_evicted(1, 1, |hasher| {
            hasher.u64(record.id);
            hasher.text(&record.event);
            hasher.u64(record.frame);
            hasher.text(&record.payload);
            hash_cause(hasher, &record.by);
            hasher.optional_text(record.origin.as_deref());
        });
    }

    fn absorb_evicted_commit(&mut self, (frame, watermark): (u64, usize)) {
        self.absorb_evicted(2, 1, |hasher| {
            hasher.u64(frame);
            hasher.u64(watermark as u64);
        });
    }

    fn absorb_evicted_settlement(&mut self, record: &SettlementRecord) {
        self.absorb_evicted(3, 1, |hasher| {
            hasher.u64(record.id);
            hasher.u64(record.frame);
            hash_cause(hasher, &record.by);
        });
    }

    fn absorb_evicted_proposal(&mut self, record: &ProposalRecord) {
        self.absorb_evicted(4, 1, |hasher| {
            hasher.u64(record.id);
            hasher.u64(record.settlement_id);
            hasher.text(&record.intent);
            hasher.u32(record.key);
            hasher.text(&record.payload);
            hasher.text(&record.law);
            hasher.u32(record.source_line);
        });
    }

    fn absorb_evicted_resolution(&mut self, record: &ResolutionRecord) {
        self.absorb_evicted(5, 1, |hasher| {
            hasher.u64(record.id);
            hasher.u64(record.settlement_id);
            hasher.text(&record.intent);
            hasher.u32(record.key);
            hasher.text(&record.resolver);
            hasher.u64(record.proposal_ids.len() as u64);
            for proposal_id in &record.proposal_ids {
                hasher.u64(*proposal_id);
            }
        });
    }

    fn absorb_evicted_relation_assertion(&mut self, record: &RelationAssertionRecord) {
        self.absorb_evicted(6, 1, |hasher| {
            hasher.u64(record.frame);
            hasher.u64(record.assertion_id);
            hasher.text(&crate::relation::runtime::fact_key_transport_hex(
                &record.fact_key,
            ));
            hasher.u64(record.resolution_ids.len() as u64);
            for resolution_id in &record.resolution_ids {
                hasher.u64(*resolution_id);
            }
            hasher.optional_text(record.origin.as_deref());
        });
    }

    fn absorb_remote_truncation(&mut self, marker: &ProvenanceTruncation, origin: &str) {
        if !marker.is_truncated() {
            return;
        }
        self.absorb_evicted(7, marker.evicted_records, |hasher| {
            hasher.text(origin);
            hasher.bytes(&marker.digest);
        });
    }
}
