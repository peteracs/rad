//! Structured metadata for concatenated source units.
//!
//! A replay trace embeds one self-contained source string. Imported modules
//! are concatenated into that string, but tokens must retain their original
//! per-file line numbers. `SourceLayout` carries those boundaries explicitly;
//! comments and other source text never acquire hidden lexer semantics.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const SOURCE_LAYOUT_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceImport {
    pub specifier: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSection {
    pub byte_offset: usize,
    pub name: String,
    pub line: u32,
    pub column: u32,
    #[serde(default)]
    pub imports: Vec<SourceImport>,
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
                imports: Vec::new(),
            }],
        }
    }

    pub fn push(&mut self, byte_offset: usize, name: impl Into<String>) {
        self.sections.push(SourceSection {
            byte_offset,
            name: name.into(),
            line: 1,
            column: 1,
            imports: Vec::new(),
        });
    }

    pub fn add_import(
        &mut self,
        section_index: usize,
        specifier: impl Into<String>,
        target: impl Into<String>,
    ) -> Result<(), String> {
        let section = self
            .sections
            .get_mut(section_index)
            .ok_or_else(|| "source bundle section index is out of range".to_string())?;
        let specifier = specifier.into();
        let target = target.into();
        if let Some(existing) = section
            .imports
            .iter()
            .find(|import| import.specifier == specifier)
        {
            return if existing.target == target {
                Ok(())
            } else {
                Err(format!(
                    "source bundle import '{specifier}' has conflicting targets"
                ))
            };
        }
        section.imports.push(SourceImport { specifier, target });
        Ok(())
    }

    /// Reconstruct the immutable source files and resolved import graph
    /// embedded in a trace. The returned paths are virtual bundle identities;
    /// callers must not consult the host filesystem for their contents.
    pub fn files(&self, source: &str) -> Result<SourceBundleFiles, String> {
        self.validate(source)?;
        let Some(entry) = self.sections.first() else {
            return Err("source bundle has no entry section".to_string());
        };
        let mut files = HashMap::new();
        for (index, section) in self.sections.iter().enumerate() {
            let end = self
                .sections
                .get(index + 1)
                .map_or(source.len(), |next| next.byte_offset);
            if files
                .insert(
                    PathBuf::from(&section.name),
                    source[section.byte_offset..end].to_string(),
                )
                .is_some()
            {
                return Err(format!(
                    "source bundle contains duplicate unit '{}'",
                    section.name
                ));
            }
        }
        let mut imports = HashMap::new();
        for section in &self.sections {
            for import in &section.imports {
                let target = PathBuf::from(&import.target);
                if !files.contains_key(&target) {
                    return Err(format!(
                        "source bundle import '{}' from '{}' targets missing unit '{}'",
                        import.specifier, section.name, import.target
                    ));
                }
                if imports
                    .insert(
                        (PathBuf::from(&section.name), import.specifier.clone()),
                        target,
                    )
                    .is_some()
                {
                    return Err(format!(
                        "source bundle unit '{}' repeats import '{}'",
                        section.name, import.specifier
                    ));
                }
            }
        }
        Ok(SourceBundleFiles {
            entry: PathBuf::from(&entry.name),
            files,
            imports,
        })
    }

    pub fn validate(&self, source: &str) -> Result<(), String> {
        if self
            .sections
            .first()
            .is_some_and(|section| section.byte_offset != 0)
        {
            return Err("source layout must assign byte zero to its entry unit".to_string());
        }
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
        digest.update(b"rad-source-layout/v2\0");
        digest.update(&(source.len() as u64).to_le_bytes());
        digest.update(source.as_bytes());
        digest.update(&(self.sections.len() as u64).to_le_bytes());
        for section in &self.sections {
            digest.update(&(section.byte_offset as u64).to_le_bytes());
            digest.update(&(section.name.len() as u64).to_le_bytes());
            digest.update(section.name.as_bytes());
            digest.update(&section.line.to_le_bytes());
            digest.update(&section.column.to_le_bytes());
            digest.update(&(section.imports.len() as u64).to_le_bytes());
            for import in &section.imports {
                digest.update(&(import.specifier.len() as u64).to_le_bytes());
                digest.update(import.specifier.as_bytes());
                digest.update(&(import.target.len() as u64).to_le_bytes());
                digest.update(import.target.as_bytes());
            }
        }
        Ok(digest.finalize().to_hex().to_string())
    }
}

#[derive(Debug)]
pub struct SourceBundleFiles {
    pub entry: PathBuf,
    pub files: HashMap<PathBuf, String>,
    pub imports: HashMap<(PathBuf, String), PathBuf>,
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

        let mut imported = layout.clone();
        imported
            .add_import(0, "./b.rad", "b.rad")
            .expect("record import edge");
        assert_ne!(baseline, imported.digest(source).unwrap());
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
                imports: Vec::new(),
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
                    imports: Vec::new(),
                },
                SourceSection {
                    byte_offset: 0,
                    name: "b.rad".into(),
                    line: 1,
                    column: 1,
                    imports: Vec::new(),
                },
            ],
        };
        assert!(duplicate.validate(source).is_err());

        let skipped_prefix = SourceLayout {
            sections: vec![SourceSection {
                byte_offset: 2,
                name: "a.rad".into(),
                line: 1,
                column: 1,
                imports: Vec::new(),
            }],
        };
        assert!(skipped_prefix.validate(source).is_err());
    }

    #[test]
    fn bundle_rejects_imports_outside_its_authenticated_units() {
        let source = "use \"missing.rad\"\n";
        let mut layout = SourceLayout::single("main.rad");
        layout
            .add_import(0, "missing.rad", "missing.rad")
            .expect("record import edge");

        let error = layout.files(source).expect_err("target must be embedded");
        assert!(error.contains("targets missing unit 'missing.rad'"));
    }
}
