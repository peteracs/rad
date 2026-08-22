// Encoding the ledger into the operational checkpoint byte stream.

impl CausalityLedger {

    /// Versioned semantic encoding used by portable attempt checkpoints.
    /// Diagnostic `Debug` output is deliberately not part of replay identity.
    pub(crate) fn encode_checkpoint(&self, out: &mut crate::canonical::CanonicalWriter) {
        fn cause(out: &mut crate::canonical::CanonicalWriter, input: &Cause) {
            match input {
                Cause::Main => out.byte(0),
                Cause::System { name } => {
                    out.byte(1);
                    out.text(name);
                }
                Cause::Handler { event, emit_id } => {
                    out.byte(2);
                    out.text(event);
                    out.u64(*emit_id);
                }
                Cause::Transaction { name, parent } => {
                    out.byte(3);
                    out.text(name);
                    cause(out, parent);
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
                    out.byte(4);
                    out.text(extension);
                    out.text(generation);
                    out.text(plugin_digest);
                    out.text(export);
                    out.text(input_digest);
                    out.text(output_digest);
                    cause(out, parent);
                }
            }
        }
        fn write_kind(out: &mut crate::canonical::CanonicalWriter, kind: WriteKind) {
            out.byte(match kind {
                WriteKind::Set => 0,
                WriteKind::Spawn => 1,
                WriteKind::Despawn => 2,
                WriteKind::Remove => 3,
                WriteKind::Resource => 4,
            });
        }

        out.usize(self.writes.len());
        for write in &self.writes {
            out.u64(write.frame);
            out.bool(write.entity.is_some());
            if let Some(entity) = write.entity {
                out.u32(entity);
            }
            out.optional_text(write.entity_name.as_deref());
            out.text(&write.component);
            out.text(&write.value_string());
            out.usize(write.fields().len());
            for (field, value) in write.fields() {
                out.text(field);
                out.text(&value.to_string());
            }
            write_kind(out, write.kind);
            cause(out, &write.by);
            out.optional_text(write.origin.as_deref());
            out.optional_u64(write.resolution_id);
        }

        out.usize(self.emits.len());
        for emit in &self.emits {
            out.u64(emit.id);
            out.text(&emit.event);
            out.u64(emit.frame);
            out.text(&emit.payload);
            cause(out, &emit.by);
            out.optional_text(emit.origin.as_deref());
        }

        out.usize(self.commits.len());
        for (frame, watermark) in &self.commits {
            out.u64(*frame);
            out.usize(*watermark);
        }

        out.usize(self.settlements.len());
        for settlement in &self.settlements {
            out.u64(settlement.id);
            out.u64(settlement.frame);
            cause(out, &settlement.by);
        }

        out.usize(self.proposals.len());
        for proposal in &self.proposals {
            out.u64(proposal.id);
            out.u64(proposal.settlement_id);
            out.text(&proposal.intent);
            out.u32(proposal.key);
            out.text(&proposal.payload);
            out.text(&proposal.law);
            out.u32(proposal.source_line);
        }

        out.usize(self.resolutions.len());
        for resolution in &self.resolutions {
            out.u64(resolution.id);
            out.u64(resolution.settlement_id);
            out.text(&resolution.intent);
            out.u32(resolution.key);
            out.text(&resolution.resolver);
            out.usize(resolution.proposal_ids.len());
            for proposal_id in &resolution.proposal_ids {
                out.u64(*proposal_id);
            }
        }

        out.usize(self.relation_assertions.len());
        for assertion in &self.relation_assertions {
            out.u64(assertion.frame);
            out.u64(assertion.assertion_id);
            out.text(&crate::relation::runtime::fact_key_transport_hex(
                &assertion.fact_key,
            ));
            out.usize(assertion.resolution_ids.len());
            for resolution_id in &assertion.resolution_ids {
                out.u64(*resolution_id);
            }
            out.optional_text(assertion.origin.as_deref());
        }

        out.u64(self.next_settlement_id);
        out.u64(self.next_proposal_id);
        out.u64(self.next_resolution_id);
        out.usize(self.cap);
        out.usize(self.write_base);
        out.usize(self.emit_base);
        out.u64(self.truncation.evicted_records);
        out.bytes(&self.truncation.digest);
    }

    pub(crate) fn encode_cause_checkpoint(
        cause: &Cause,
        out: &mut crate::canonical::CanonicalWriter,
    ) {
        match cause {
            Cause::Main => out.byte(0),
            Cause::System { name } => {
                out.byte(1);
                out.text(name);
            }
            Cause::Handler { event, emit_id } => {
                out.byte(2);
                out.text(event);
                out.u64(*emit_id);
            }
            Cause::Transaction { name, parent } => {
                out.byte(3);
                out.text(name);
                Self::encode_cause_checkpoint(parent, out);
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
                out.byte(4);
                out.text(extension);
                out.text(generation);
                out.text(plugin_digest);
                out.text(export);
                out.text(input_digest);
                out.text(output_digest);
                Self::encode_cause_checkpoint(parent, out);
            }
        }
    }
}
