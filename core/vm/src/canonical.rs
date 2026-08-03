//! Small versioned binary writer for semantic identities.
//!
//! This is intentionally not a diagnostic formatter. Every value is framed,
//! every collection declares its length, and callers choose an explicit
//! schema version. The resulting bytes are suitable for durable hashes and
//! portable replay records.

#[derive(Default)]
pub(crate) struct CanonicalWriter {
    bytes: Vec<u8>,
}

impl CanonicalWriter {
    pub(crate) fn with_domain(domain: &str) -> Self {
        let mut writer = Self::default();
        writer.text(domain);
        writer
    }

    pub(crate) fn byte(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn bool(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    pub(crate) fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn usize(&mut self, value: usize) {
        self.u64(value as u64);
    }

    pub(crate) fn bytes(&mut self, value: &[u8]) {
        self.u64(value.len() as u64);
        self.bytes.extend_from_slice(value);
    }

    pub(crate) fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    pub(crate) fn optional_text(&mut self, value: Option<&str>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.text(value);
        }
    }

    pub(crate) fn optional_u64(&mut self, value: Option<u64>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.u64(value);
        }
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::CanonicalWriter;

    #[test]
    fn framing_distinguishes_adjacent_strings() {
        let mut left = CanonicalWriter::with_domain("test/v1");
        left.text("ab");
        left.text("c");

        let mut right = CanonicalWriter::with_domain("test/v1");
        right.text("a");
        right.text("bc");

        assert_ne!(left.finish(), right.finish());
    }
}
