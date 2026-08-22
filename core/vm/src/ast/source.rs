#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

pub struct NodeIdGen {
    next: u32,
}

impl NodeIdGen {
    pub fn new() -> Self {
        Self { next: 0 }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> NodeId {
        let id = NodeId(self.next);
        self.next += 1;
        id
    }
}

impl Default for NodeIdGen {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: String,
    pub source: String,
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(path: String, source: String) -> Self {
        let mut line_starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self {
            path,
            source,
            line_starts,
        }
    }

    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let col = offset - self.line_starts[line];
        ((line + 1) as u32, col + 1)
    }

    pub fn line_text(&self, line: u32) -> &str {
        let idx = (line as usize).saturating_sub(1);
        if idx >= self.line_starts.len() {
            return "";
        }
        let start = self.line_starts[idx] as usize;
        let end = if idx + 1 < self.line_starts.len() {
            (self.line_starts[idx + 1] as usize).saturating_sub(1)
        } else {
            self.source.len()
        };
        &self.source[start..end]
    }
}

#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn add_file(&mut self, path: String, source: String) -> FileId {
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile::new(path, source));
        id
    }

    pub fn get_file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn resolve_span(&self, span: &Span) -> Option<(&str, u32, u32)> {
        let file_id = span.file?;
        let file = self.get_file(file_id)?;
        Some((&file.path, span.line, span.col))
    }

    pub fn file_id_for_path(&self, path: &str) -> Option<FileId> {
        self.files
            .iter()
            .position(|f| f.path == path)
            .map(|i| FileId(i as u32))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub file: Option<FileId>,
}

impl Span {
    pub fn with_file(mut self, file: FileId) -> Self {
        self.file = Some(file);
        self
    }
}
