const BINARY_PUBLICATION_MAGIC: [u8; 4] = *b"RBS1";
pub const MAX_BINARY_PUBLICATION_CHANNELS: usize = 32;
pub const MAX_BINARY_PUBLICATION_RECORDS_PER_CHANNEL: usize = 1024;
pub const MAX_BINARY_PUBLICATION_RECORD_BYTES: usize = 1024 * 1024;
pub const MAX_BINARY_PUBLICATION_QUEUED_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_BINARY_PUBLICATION_PACKET_BYTES: usize = 16 * 1024 * 1024;
const MAX_BINARY_PUBLICATION_CHANNEL_BYTES: usize = 64;
const BINARY_PUBLICATION_HEADER_BYTES: usize = 8;
const BINARY_PUBLICATION_RECORD_HEADER_BYTES: usize = 4;

#[derive(Default)]
pub(crate) struct BinaryPublications {
    channels: HashMap<String, VecDeque<Vec<u8>>>,
    queued_bytes: usize,
}

fn validate_binary_publication_channel(channel: &str) -> Result<(), String> {
    let bytes = channel.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_BINARY_PUBLICATION_CHANNEL_BYTES {
        return Err(format!(
            "binary publication channel must contain 1..={MAX_BINARY_PUBLICATION_CHANNEL_BYTES} bytes"
        ));
    }
    if !bytes.iter().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-' | b'.' | b'/')
    }) {
        return Err(
            "binary publication channel may contain only ASCII letters, digits, '_', '-', '.', and '/'"
                .to_string(),
        );
    }
    Ok(())
}

impl BinaryPublications {
    pub(crate) fn publish(&mut self, channel: &str, payload: &[u8]) -> Result<(), String> {
        validate_binary_publication_channel(channel)?;
        if payload.len() > MAX_BINARY_PUBLICATION_RECORD_BYTES {
            return Err(format!(
                "publish_bytes(): payload has {} bytes; per-record limit is {}",
                payload.len(),
                MAX_BINARY_PUBLICATION_RECORD_BYTES
            ));
        }
        if !self.channels.contains_key(channel)
            && self.channels.len() == MAX_BINARY_PUBLICATION_CHANNELS
        {
            return Err(format!(
                "publish_bytes(): channel limit {} reached",
                MAX_BINARY_PUBLICATION_CHANNELS
            ));
        }
        let queue = self.channels.entry(channel.to_string()).or_default();
        if queue.len() == MAX_BINARY_PUBLICATION_RECORDS_PER_CHANNEL {
            return Err(format!(
                "publish_bytes(): channel '{}' contains {} queued records; drain it before publishing more",
                channel, MAX_BINARY_PUBLICATION_RECORDS_PER_CHANNEL
            ));
        }
        let queued_bytes = self
            .queued_bytes
            .checked_add(payload.len())
            .ok_or_else(|| "publish_bytes(): queued-byte accounting overflow".to_string())?;
        if queued_bytes > MAX_BINARY_PUBLICATION_QUEUED_BYTES {
            return Err(format!(
                "publish_bytes(): queued bytes would exceed the {}-byte runtime limit",
                MAX_BINARY_PUBLICATION_QUEUED_BYTES
            ));
        }
        queue.push_back(payload.to_vec());
        self.queued_bytes = queued_bytes;
        Ok(())
    }

    pub(crate) fn drain_packet(
        &mut self,
        channel: &str,
        max_records: usize,
        max_packet_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        validate_binary_publication_channel(channel)?;
        if max_records == 0 || max_records > MAX_BINARY_PUBLICATION_RECORDS_PER_CHANNEL {
            return Err(format!(
                "binary publication drain max_records must be in 1..={MAX_BINARY_PUBLICATION_RECORDS_PER_CHANNEL}"
            ));
        }
        if !(BINARY_PUBLICATION_HEADER_BYTES..=MAX_BINARY_PUBLICATION_PACKET_BYTES)
            .contains(&max_packet_bytes)
        {
            return Err(format!(
                "binary publication drain max_packet_bytes must be in {BINARY_PUBLICATION_HEADER_BYTES}..={MAX_BINARY_PUBLICATION_PACKET_BYTES}"
            ));
        }

        let Some(queue) = self.channels.get(channel) else {
            return Ok(empty_binary_publication_packet());
        };
        let mut packet_bytes = BINARY_PUBLICATION_HEADER_BYTES;
        let mut record_count = 0usize;
        for payload in queue.iter().take(max_records) {
            let needed = BINARY_PUBLICATION_RECORD_HEADER_BYTES
                .checked_add(payload.len())
                .and_then(|bytes| packet_bytes.checked_add(bytes))
                .ok_or_else(|| "binary publication packet-size overflow".to_string())?;
            if needed > max_packet_bytes {
                if record_count == 0 {
                    return Err(format!(
                        "binary publication drain limit {} cannot fit the next {}-byte record on channel '{}'",
                        max_packet_bytes,
                        payload.len(),
                        channel
                    ));
                }
                break;
            }
            packet_bytes = needed;
            record_count += 1;
        }

        let mut packet = Vec::with_capacity(packet_bytes);
        packet.extend_from_slice(&BINARY_PUBLICATION_MAGIC);
        packet.extend_from_slice(&(record_count as u32).to_le_bytes());
        let queue = self
            .channels
            .get_mut(channel)
            .expect("publication queue exists after immutable selection");
        for _ in 0..record_count {
            let payload = queue
                .pop_front()
                .expect("selected publication record remains queued");
            packet.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            packet.extend_from_slice(&payload);
            self.queued_bytes -= payload.len();
        }
        if queue.is_empty() {
            self.channels.remove(channel);
        }
        Ok(packet)
    }

    pub(crate) fn clear(&mut self) {
        self.channels.clear();
        self.queued_bytes = 0;
    }
}

fn empty_binary_publication_packet() -> Vec<u8> {
    let mut packet = Vec::with_capacity(BINARY_PUBLICATION_HEADER_BYTES);
    packet.extend_from_slice(&BINARY_PUBLICATION_MAGIC);
    packet.extend_from_slice(&0u32.to_le_bytes());
    packet
}

impl VM {
    pub fn drain_binary_publication_packet(
        &mut self,
        channel: &str,
        max_records: usize,
        max_packet_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        self.binary_publications
            .drain_packet(channel, max_records, max_packet_bytes)
    }

    pub(crate) fn clear_binary_publications(&mut self) {
        self.binary_publications.clear();
    }
}
