use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const TRACE_FILE_PREFIX: &[u8] = b"RADPACKZ:RADTRACE ";
const DIGEST_HEX_LEN: usize = 64;
static TRACE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Streaming RADTRACE writer. The compressed body is written once, its
/// uncompressed BLAKE3 digest is patched into the fixed-width envelope header,
/// and only a complete file replaces the requested destination.
pub(crate) struct FileTraceSink {
    output_path: PathBuf,
    temp_path: PathBuf,
    encoder: Option<zstd::stream::write::Encoder<'static, File>>,
    digest: blake3::Hasher,
    error: Option<String>,
}

impl FileTraceSink {
    pub(crate) fn create(output_path: &Path) -> Result<Self, String> {
        let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
        let leaf = output_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "trace output path has no UTF-8 file name".to_string())?;
        let sequence = TRACE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(
            ".{leaf}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|error| format!("cannot create trace staging file: {error}"))?;
        file.write_all(TRACE_FILE_PREFIX)
            .and_then(|_| file.write_all(&[b'0'; DIGEST_HEX_LEN]))
            .and_then(|_| file.write_all(b" "))
            .map_err(|error| format!("cannot write trace envelope header: {error}"))?;
        let encoder = zstd::stream::write::Encoder::new(file, crate::radpack::ZSTD_LEVEL)
            .map_err(|error| format!("cannot start trace compression: {error}"))?;
        Ok(Self {
            output_path: output_path.to_path_buf(),
            temp_path,
            encoder: Some(encoder),
            digest: blake3::Hasher::new(),
            error: None,
        })
    }

    pub(crate) fn write_line(&mut self, line: &str) {
        if self.error.is_some() {
            return;
        }
        let encoder = self.encoder.as_mut().expect("active trace encoder");
        if let Err(error) = encoder
            .write_all(line.as_bytes())
            .and_then(|_| encoder.write_all(b"\n"))
        {
            self.error = Some(format!("cannot stream trace record: {error}"));
            return;
        }
        self.digest.update(line.as_bytes());
        self.digest.update(b"\n");
    }

    pub(crate) fn finish(mut self) -> Result<PathBuf, String> {
        if let Some(error) = self.error.take() {
            let _ = std::fs::remove_file(&self.temp_path);
            return Err(error);
        }
        let encoder = self.encoder.take().expect("active trace encoder");
        let mut file = encoder
            .finish()
            .map_err(|error| format!("cannot finish trace compression: {error}"))?;
        let digest = self.digest.finalize().to_hex();
        file.seek(std::io::SeekFrom::Start(TRACE_FILE_PREFIX.len() as u64))
            .and_then(|_| file.write_all(digest.as_bytes()))
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("cannot seal trace envelope: {error}"))?;
        drop(file);
        if self.output_path.exists() {
            std::fs::remove_file(&self.output_path)
                .map_err(|error| format!("cannot replace existing trace: {error}"))?;
        }
        std::fs::rename(&self.temp_path, &self.output_path)
            .map_err(|error| format!("cannot publish completed trace: {error}"))?;
        Ok(self.output_path)
    }
}

/// A bounded reader over the uncompressed JSONL body. `verify_digest` is used
/// by the metadata pass before execution; ordinary replay then reopens the
/// same canonical envelope and consumes records one at a time.
pub(crate) struct TraceBodyReader {
    reader: Box<dyn std::io::BufRead + Send>,
    expected_digest: Option<String>,
    digest: blake3::Hasher,
}

impl TraceBodyReader {
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|error| format!("cannot open trace: {error}"))?;
        let mut prefix = vec![0u8; TRACE_FILE_PREFIX.len()];
        let read = file
            .read(&mut prefix)
            .map_err(|error| format!("cannot read trace header: {error}"))?;
        file.seek(std::io::SeekFrom::Start(0))
            .map_err(|error| format!("cannot seek trace: {error}"))?;
        if read == TRACE_FILE_PREFIX.len() && prefix == TRACE_FILE_PREFIX {
            file.seek(std::io::SeekFrom::Start(TRACE_FILE_PREFIX.len() as u64))
                .map_err(|error| format!("cannot seek trace digest: {error}"))?;
            let mut digest = [0u8; DIGEST_HEX_LEN];
            file.read_exact(&mut digest)
                .map_err(|error| format!("cannot read trace digest: {error}"))?;
            let mut separator = [0u8; 1];
            file.read_exact(&mut separator)
                .map_err(|error| format!("cannot read trace header separator: {error}"))?;
            if separator != *b" " || !digest.iter().all(u8::is_ascii_hexdigit) {
                return Err("trace has a malformed RADPACKZ digest header".to_string());
            }
            let expected_digest = String::from_utf8(digest.to_vec())
                .map_err(|_| "trace digest header is not UTF-8".to_string())?;
            let decoder = zstd::stream::read::Decoder::new(file)
                .map_err(|error| format!("cannot start trace decompression: {error}"))?;
            return Ok(Self {
                reader: Box::new(std::io::BufReader::new(decoder)),
                expected_digest: Some(expected_digest),
                digest: blake3::Hasher::new(),
            });
        }
        Ok(Self {
            reader: Box::new(std::io::BufReader::new(file)),
            expected_digest: None,
            digest: blake3::Hasher::new(),
        })
    }

    pub(crate) fn read_line(&mut self, output: &mut String) -> Result<usize, String> {
        output.clear();
        let read = self
            .reader
            .read_line(output)
            .map_err(|error| format!("cannot read trace record: {error}"))?;
        if read != 0 {
            self.digest.update(output.as_bytes());
        }
        Ok(read)
    }

    pub(crate) fn verify_digest(&self) -> Result<(), String> {
        let Some(expected) = &self.expected_digest else {
            return Ok(());
        };
        let actual = self.digest.clone().finalize().to_hex();
        if expected == actual.as_str() {
            Ok(())
        } else {
            Err(format!(
                "trace integrity digest mismatch: expected {}..., computed {}...",
                &expected[..12],
                &actual.as_str()[..12]
            ))
        }
    }
}

pub(crate) struct TraceFileIndex {
    pub(crate) metadata_jsonl: String,
    pub(crate) total_io: usize,
    pub(crate) total_frames: u64,
}

pub(crate) fn index_trace_file(path: &Path) -> Result<TraceFileIndex, String> {
    let mut body = TraceBodyReader::open(path)?;
    let mut line = String::new();
    let mut metadata_jsonl = String::new();
    let mut line_number = 0usize;
    let mut total_io = 0usize;
    let mut total_frames = 0u64;
    let mut saw_header = false;
    while body.read_line(&mut line)? != 0 {
        line_number += 1;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .map_err(|error| format!("trace line {line_number} is not valid JSON: {error}"))?;
        match value["t"].as_str() {
            Some("header") if !saw_header => {
                saw_header = true;
                metadata_jsonl.push_str(&line);
                if !line.ends_with('\n') {
                    metadata_jsonl.push('\n');
                }
            }
            Some("io") => {
                parse_io_record(&value)?;
                total_io += 1;
            }
            Some("frame") => total_frames += 1,
            Some("end") => {
                metadata_jsonl.push_str(&line);
                if !line.ends_with('\n') {
                    metadata_jsonl.push('\n');
                }
            }
            other => {
                return Err(format!(
                    "trace line {line_number}: unknown record type {other:?}"
                ));
            }
        }
    }
    body.verify_digest()?;
    Ok(TraceFileIndex {
        metadata_jsonl,
        total_io,
        total_frames,
    })
}

pub(crate) struct StrictTraceStream {
    body: TraceBodyReader,
    line_number: usize,
    total_io: usize,
    consumed_io: usize,
}

impl StrictTraceStream {
    pub(crate) fn open(path: &Path, total_io: usize) -> Result<Self, String> {
        let mut body = TraceBodyReader::open(path)?;
        let mut line = String::new();
        let mut line_number = 0usize;
        loop {
            if body.read_line(&mut line)? == 0 {
                return Err("trace is empty".to_string());
            }
            line_number += 1;
            if line.trim().is_empty() {
                continue;
            }
            let header: serde_json::Value = serde_json::from_str(&line)
                .map_err(|_| "trace header is not valid JSON".to_string())?;
            if header["t"] != "header" {
                return Err("trace does not start with a header record".to_string());
            }
            break;
        }
        Ok(Self {
            body,
            line_number,
            total_io,
            consumed_io: 0,
        })
    }

    pub(crate) fn next_io(
        &mut self,
        current_frame: u64,
        builtin: &str,
        args_digest: &str,
    ) -> Result<IoRecord, String> {
        let mut line = String::new();
        loop {
            if self.body.read_line(&mut line)? == 0 {
                return Err(format!(
                    "replay divergence at frame {current_frame}: the replayed run calls {builtin}() but the recorded run performed no further io"
                ));
            }
            self.line_number += 1;
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(&line).map_err(|error| {
                format!(
                    "trace line {} is not valid JSON: {error}",
                    self.line_number
                )
            })?;
            match value["t"].as_str() {
                Some("io") => {
                    let record = parse_io_record(&value)?;
                    if record.builtin != builtin
                        || record.args_digest != args_digest
                        || record.frame != current_frame
                    {
                        return Err(format!(
                            "replay divergence at frame {current_frame}, record #{}: recorded {}(args {}) in frame {}, replayed {}(args {})",
                            self.consumed_io,
                            record.builtin,
                            record.args_digest,
                            record.frame,
                            builtin,
                            args_digest
                        ));
                    }
                    self.consumed_io += 1;
                    return Ok(record);
                }
                Some("frame" | "end") => continue,
                other => {
                    return Err(format!(
                        "trace line {}: unknown record type {other:?}",
                        self.line_number
                    ));
                }
            }
        }
    }

    pub(crate) fn consumed(&self) -> usize {
        self.consumed_io
    }

    pub(crate) fn remaining(&self) -> usize {
        self.total_io.saturating_sub(self.consumed_io)
    }
}
