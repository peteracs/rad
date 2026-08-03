//! Structured metadata for concatenated source units.
//!
//! A replay trace embeds one self-contained source string. Imported modules
//! are concatenated into that string, but tokens must retain their original
//! per-file line numbers. `SourceLayout` carries those boundaries explicitly;
//! comments and other source text never acquire hidden lexer semantics.

use serde::{Deserialize, Serialize};

pub const SOURCE_LAYOUT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSection {
    pub byte_offset: usize,
    pub name: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLayout {
    pub sections: Vec<SourceSection>,
}

impl SourceLayout {
    pub fn single(name: impl Into<String>) -> Self {
        Self {
            sections: vec![SourceSection {
                byte_offset: 0,
                name: name.into(),
                line: 1,
                column: 1,
            }],
        }
    }

    pub fn push(&mut self, byte_offset: usize, name: impl Into<String>) {
        self.sections.push(SourceSection {
            byte_offset,
            name: name.into(),
            line: 1,
            column: 1,
        });
    }

    pub fn validate(&self, source: &str) -> Result<(), String> {
        let mut previous = None;
        for section in &self.sections {
            if section.line == 0 || section.column == 0 {
                return Err("source layout lines and columns are one-based".to_string());
            }
            if section.byte_offset > source.len() || !source.is_char_boundary(section.byte_offset) {
                return Err(format!(
                    "source layout offset {} is not a UTF-8 boundary",
                    section.byte_offset
                ));
            }
            if previous.is_some_and(|offset| section.byte_offset <= offset) {
                return Err("source layout sections must have strictly increasing offsets".into());
            }
            previous = Some(section.byte_offset);
        }
        Ok(())
    }

    pub fn digest(&self, source: &str) -> Result<String, String> {
        self.validate(source)?;
        let mut digest = blake3::Hasher::new();
        digest.update(b"rad-source-layout/v1\0");
        digest.update(&(source.len() as u64).to_le_bytes());
        digest.update(source.as_bytes());
        digest.update(&(self.sections.len() as u64).to_le_bytes());
        for section in &self.sections {
            digest.update(&(section.byte_offset as u64).to_le_bytes());
            digest.update(&(section.name.len() as u64).to_le_bytes());
            digest.update(section.name.as_bytes());
            digest.update(&section.line.to_le_bytes());
            digest.update(&section.column.to_le_bytes());
        }
        Ok(digest.finalize().to_hex().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_digest_binds_names_offsets_and_source() {
        let source = "let a = 1\nlet b = 2\n";
        let mut layout = SourceLayout::single("a.rad");
        layout.push(10, "b.rad");
        let baseline = layout.digest(source).unwrap();

        let mut renamed = layout.clone();
        renamed.sections[1].name = "other.rad".into();
        assert_ne!(baseline, renamed.digest(source).unwrap());

        let mut moved = layout.clone();
        moved.sections[1].byte_offset = 9;
        assert_ne!(baseline, moved.digest(source).unwrap());
    }

    #[test]
    fn layout_rejects_non_boundaries_and_duplicate_offsets() {
        let source = "é\n";
        let invalid = SourceLayout {
            sections: vec![SourceSection {
                byte_offset: 1,
                name: "bad.rad".into(),
                line: 1,
                column: 1,
            }],
        };
        assert!(invalid.validate(source).is_err());

        let duplicate = SourceLayout {
            sections: vec![
                SourceSection {
                    byte_offset: 0,
                    name: "a.rad".into(),
                    line: 1,
                    column: 1,
                },
                SourceSection {
                    byte_offset: 0,
                    name: "b.rad".into(),
                    line: 1,
                    column: 1,
                },
            ],
        };
        assert!(duplicate.validate(source).is_err());
    }
}

