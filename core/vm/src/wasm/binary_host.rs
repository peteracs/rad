const MAX_BINARY_EVENT_BYTES: usize = 1_048_576;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl RadRuntime {
    /// Enqueue one host-owned byte packet without JSON expansion. The target
    /// event must declare exactly one `bytebuf` field. Bounds and the complete
    /// event contract are validated before the live event queue is changed.
    pub fn session_emit_binary(&mut self, event: &str, payload: &[u8]) -> Result<(), String> {
        if payload.len() > MAX_BINARY_EVENT_BYTES {
            return Err(format!(
                "session_emit_binary: payload has {} bytes; limit is {}",
                payload.len(),
                MAX_BINARY_EVENT_BYTES
            ));
        }
        let layout = self
            .vm
            .component_layouts
            .get(event)
            .cloned()
            .ok_or_else(|| format!("session_emit_binary: unknown event '{}'", event))?;
        if layout.len() != 1 {
            return Err(format!(
                "session_emit_binary: event '{}' must declare exactly one bytebuf field",
                event
            ));
        }
        let declared = self
            .vm
            .component_field_types
            .get(event)
            .ok_or_else(|| {
                format!(
                    "session_emit_binary: event '{}' has no checked field metadata",
                    event
                )
            })?;
        if declared.len() != 1 || declared[0].1 != crate::types::Ty::ByteBuf {
            return Err(format!(
                "session_emit_binary: event '{}.{}' must have type bytebuf",
                event, layout[0]
            ));
        }

        let value = Value::bytebuf(self.vm.gc_mut(), payload.to_vec());
        let packet = Value::component(self.vm.gc_mut(), event.to_string(), layout, vec![value]);
        self.vm.enqueue_event(packet)
    }

    /// Drain one bounded binary publication packet from a RAD-owned channel.
    /// The packet is `RBS1`, a little-endian record count, then repeated
    /// little-endian byte lengths and payloads. A record is removed only when
    /// the caller's complete packet budget can contain it.
    pub fn session_drain_binary(
        &mut self,
        channel: &str,
        max_records: u32,
        max_packet_bytes: u32,
    ) -> Result<Vec<u8>, String> {
        self.vm.drain_binary_publication_packet(
            channel,
            max_records as usize,
            max_packet_bytes as usize,
        )
    }
}
