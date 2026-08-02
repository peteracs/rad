//! Record & replay (list item #2), Phase 1: the trace recorder.
//!
//! Strategy: record *inputs*, not state. The interpreter is deterministic
//! (enforced by `determinism.rs`), so a log of every value that crosses the
//! determinism boundary — io builtin results, clock reads, the initial RNG
//! seed — is sufficient to reproduce an entire execution bit-for-bit.
//!
//! The effect system already enumerated the boundary for us: every impure
//! builtin is classified in `builtin_effect` (`builtins.rs`), so the recorder
//! interposes at a single chokepoint (`VM::call_builtin`) instead of tracing
//! syscalls. `rand_int` is NOT recorded: it is pure xorshift off the seed.
//! `print`/`eprint`/`log` are NOT recorded: they are deterministic outputs,
//! not inputs.
//!
//! Trace format (JSONL, one object per line):
//!   header  {"t":"header","version":1,"source":...,"source_hash":...,"seed":...}
//!   io      {"t":"io","f":<frame>,"s":<seq>,"b":<builtin>,"a":<args digest>,
//!            "r":<tagged result>}            (or "e":<error> when it failed)
//!   frame   {"t":"frame","n":<frame just ended>,"fuel":<remaining, if metered>}
//!   end     {"t":"end","world":<content digest at exit (or crash) point>}
//!
//! Traces are self-contained: the header embeds the full merged source, so
//! `rad replay trace.radr` needs nothing else on disk. `source_hash` is an
//! integrity check on the embedded source — a tampered trace is refused.
//!
//! The `a` digest exists for divergence detection: if a replayed run computes
//! different arguments for an io call than the recorded run did, replay halts
//! loudly instead of returning a result from a timeline that never happened.

use crate::gc::GcHeap;
use crate::value::{Builtin, MapKey, MapStorage, Value};
use std::collections::HashMap;

pub const TRACE_VERSION: u64 = 1;

/// Builtins whose results come from outside the deterministic boundary and
/// must be recorded (and, in replay, served from the trace instead of
/// re-executed — a replay must never re-fire an HTTP POST or rewrite files).
pub fn is_replay_managed(b: Builtin) -> bool {
    use Builtin::*;
    matches!(
        b,
        Clock
            | NowUnixS
            | NowUnixMs
            | SysArgs
            | Input
            | Readline
            | ReadStdinAll
            | ReadFile
            | ReadFileBytes
            | WriteFile
            | WriteFileBytes
            | AppendFile
            | FileExists
            | RemoveFile
            | ListDir
            | CreateDir
            | RemoveDir
            | HttpGet
            | HttpPost
            | HttpPostJson
            | HttpRequest
            | TcpConnect
            | TcpListen
            | TcpAccept
            | TcpAcceptTimeout
            | TcpRead
            | TcpWrite
            | TcpClose
            | UdpBind
            | UdpRecvFrom
            | UdpRecvFromTimeout
            | UdpRecvFromBytes
            | UdpRecvFromBytesTimeout
            | UdpRecvByteBuf
            | UdpRecvByteBufTimeout
            | UdpSendTo
            | UdpSendToBytes
            | UdpSendByteBuf
            | UdpClose
    )
}

/// Digest of builtin arguments, used purely for divergence detection.
/// Relies on `Display` being deterministic (guaranteed by `determinism.rs`).
pub fn args_digest(args: &[Value]) -> String {
    let mut hasher = blake3::Hasher::new();
    for a in args {
        hasher.update(format!("{}", a).as_bytes());
        hasher.update(&[0x1f]);
    }
    hasher.finalize().to_hex()[..16].to_string()
}

pub fn source_hash(source: &str) -> String {
    blake3::hash(source.as_bytes()).to_hex().to_string()
}

// ---------------------------------------------------------------------------
// Tagged value codec.
//
// `value_to_json` is lossy (sum types flatten to objects, entity ids to
// ints), which is fine for data interchange but not for a trace that must
// reconstruct the exact `Value` an io builtin returned. This codec tags every
// node with its kind so the round trip is exact.
// ---------------------------------------------------------------------------

/// Map key -> `[tag, payload]` JSON pair; tuple keys nest recursively.
fn map_key_to_json(k: &MapKey) -> serde_json::Value {
    match k {
        MapKey::Str(s) => serde_json::json!(["s", s]),
        MapKey::Int(i) => serde_json::json!(["i", i]),
        MapKey::Bool(b) => serde_json::json!(["b", b]),
        MapKey::Entity(e) => serde_json::json!(["e", e]),
        MapKey::Tuple(items) => {
            serde_json::json!(["t", items.iter().map(map_key_to_json).collect::<Vec<_>>()])
        }
    }
}

/// Inverse of `map_key_to_json`.
fn json_to_map_key(karr: &[serde_json::Value]) -> Option<MapKey> {
    match karr.first()?.as_str()? {
        "s" => Some(MapKey::Str(karr.get(1)?.as_str()?.to_string())),
        "i" => Some(MapKey::Int(karr.get(1)?.as_i64()?)),
        "b" => Some(MapKey::Bool(karr.get(1)?.as_bool()?)),
        "e" => Some(MapKey::Entity(karr.get(1)?.as_u64()? as u32)),
        "t" => {
            let items = karr.get(1)?.as_array()?;
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let pair = item.as_array().filter(|a| a.len() == 2)?;
                out.push(json_to_map_key(pair)?);
            }
            Some(MapKey::Tuple(out))
        }
        _ => None,
    }
}

pub fn encode_value(v: &Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    if v.is_nil() {
        return Ok(json!({"t": "nil"}));
    }
    if let Some(b) = v.as_bool() {
        return Ok(json!({"t": "bool", "v": b}));
    }
    if let Some(n) = v.as_int() {
        return Ok(json!({"t": "int", "v": n}));
    }
    if let Some(x) = v.as_float() {
        if !x.is_finite() {
            return Err(format!("trace codec: cannot encode non-finite float {}", x));
        }
        return Ok(json!({"t": "float", "v": x}));
    }
    if let Some(e) = v.as_entity_id() {
        return Ok(json!({"t": "entity", "v": e}));
    }
    if let Some(s) = v.as_str() {
        return Ok(json!({"t": "str", "v": s}));
    }
    if let Some(items) = v.as_list() {
        let encoded: Result<Vec<_>, _> = items.iter().map(encode_value).collect();
        return Ok(json!({"t": "list", "v": encoded?}));
    }
    if let Some(items) = v.as_tuple() {
        let encoded: Result<Vec<_>, _> = items.iter().map(encode_value).collect();
        return Ok(json!({"t": "tuple", "v": encoded?}));
    }
    if let Some(m) = v.as_map() {
        let mut sorted_keys: Vec<&MapKey> = m.keys().collect();
        sorted_keys.sort();
        let mut pairs = Vec::with_capacity(m.len());
        for k in sorted_keys {
            let key = map_key_to_json(k);
            pairs.push(serde_json::json!([key, encode_value(&m[k])?]));
        }
        return Ok(json!({"t": "map", "v": pairs}));
    }
    if let Some(st) = v.as_sum_type() {
        let mut fields = serde_json::Map::with_capacity(st.fields.len());
        let mut keys: Vec<&String> = st.fields.keys().collect();
        keys.sort();
        for k in keys {
            fields.insert(k.clone(), encode_value(&st.fields[k])?);
        }
        return Ok(json!({
            "t": "sum", "ty": st.type_name, "var": st.variant, "fields": fields
        }));
    }
    if let Some(c) = v.as_component() {
        let values: Result<Vec<_>, _> = c.values.iter().map(encode_value).collect();
        return Ok(json!({
            "t": "comp", "ty": c.type_name, "layout": *c.layout, "values": values?
        }));
    }
    Err(format!(
        "trace codec: cannot encode {} (io builtins should only return data)",
        v.type_name()
    ))
}

pub fn decode_value(gc: &mut GcHeap, j: &serde_json::Value) -> Result<Value, String> {
    let tag = j
        .get("t")
        .and_then(|t| t.as_str())
        .ok_or_else(|| format!("trace codec: missing tag in {}", j))?;
    let bad = |what: &str| format!("trace codec: malformed {} node: {}", what, j);
    match tag {
        "nil" => Ok(Value::NIL),
        "bool" => Ok(Value::from_bool(
            j["v"].as_bool().ok_or_else(|| bad("bool"))?,
        )),
        "int" => Ok(Value::from_int(
            gc,
            j["v"].as_i64().ok_or_else(|| bad("int"))?,
        )),
        "float" => Ok(Value::from_float(
            j["v"].as_f64().ok_or_else(|| bad("float"))?,
        )),
        "entity" => Ok(Value::from_entity_id(
            gc,
            j["v"].as_u64().ok_or_else(|| bad("entity"))? as u32,
        )),
        "str" => Ok(Value::from_string(
            gc,
            j["v"].as_str().ok_or_else(|| bad("str"))?.to_string(),
        )),
        "list" | "tuple" => {
            let items = j["v"].as_array().ok_or_else(|| bad(tag))?;
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(decode_value(gc, item)?);
            }
            if tag == "list" {
                Ok(Value::list(gc, out))
            } else {
                Ok(Value::tuple(gc, out))
            }
        }
        "map" => {
            let pairs = j["v"].as_array().ok_or_else(|| bad("map"))?;
            let mut m = MapStorage::new();
            for pair in pairs {
                let kv = pair
                    .as_array()
                    .filter(|p| p.len() == 2)
                    .ok_or_else(|| bad("map"))?;
                let key_arr = kv[0]
                    .as_array()
                    .filter(|p| p.len() == 2)
                    .ok_or_else(|| bad("map key"))?;
                let key = json_to_map_key(key_arr).ok_or_else(|| bad("map key"))?;
                m.insert(key, decode_value(gc, &kv[1])?);
            }
            Ok(Value::map(gc, m))
        }
        "sum" => {
            let ty = j["ty"].as_str().ok_or_else(|| bad("sum"))?.to_string();
            let var = j["var"].as_str().ok_or_else(|| bad("sum"))?.to_string();
            let fields_obj = j["fields"].as_object().ok_or_else(|| bad("sum"))?;
            let mut fields = HashMap::with_capacity(fields_obj.len());
            for (k, fv) in fields_obj {
                fields.insert(k.clone(), decode_value(gc, fv)?);
            }
            Ok(Value::sum_type(gc, ty, var, fields))
        }
        "comp" => {
            let ty = j["ty"].as_str().ok_or_else(|| bad("comp"))?.to_string();
            let layout: Vec<String> = j["layout"]
                .as_array()
                .ok_or_else(|| bad("comp"))?
                .iter()
                .map(|s| s.as_str().map(|s| s.to_string()).ok_or_else(|| bad("comp")))
                .collect::<Result<_, _>>()?;
            let values_arr = j["values"].as_array().ok_or_else(|| bad("comp"))?;
            let mut values = Vec::with_capacity(values_arr.len());
            for item in values_arr {
                values.push(decode_value(gc, item)?);
            }
            Ok(Value::component(
                gc,
                ty,
                std::sync::Arc::new(layout),
                values,
            ))
        }
        other => Err(format!("trace codec: unknown tag '{}'", other)),
    }
}

// ---------------------------------------------------------------------------
// Recorder.
// ---------------------------------------------------------------------------

pub struct TraceRecorder {
    lines: Vec<String>,
    /// Frames completed so far; the current frame index for new io records.
    frame: u64,
    /// Sequence number within the current frame.
    seq: u64,
}

impl TraceRecorder {
    pub fn new(source: &str, seed: u64) -> Self {
        let header = serde_json::json!({
            "t": "header",
            "version": TRACE_VERSION,
            "source": source,
            "source_hash": source_hash(source),
            "seed": seed,
        });
        Self {
            lines: vec![header.to_string()],
            frame: 0,
            seq: 0,
        }
    }

    pub fn record_io(
        &mut self,
        builtin: &str,
        args_digest: String,
        result: &Result<serde_json::Value, String>,
    ) {
        let mut obj = serde_json::json!({
            "t": "io",
            "f": self.frame,
            "s": self.seq,
            "b": builtin,
            "a": args_digest,
        });
        match result {
            Ok(r) => obj["r"] = r.clone(),
            Err(e) => obj["e"] = serde_json::Value::String(e.clone()),
        }
        self.lines.push(obj.to_string());
        self.seq += 1;
    }

    /// Mark the end of a frame (called when `flush_events` flips the event
    /// buffers). `fuel_remaining` is included only when the VM is metered.
    pub fn record_frame(&mut self, fuel_remaining: u64) {
        let mut obj = serde_json::json!({"t": "frame", "n": self.frame});
        if fuel_remaining != u64::MAX {
            obj["fuel"] = serde_json::Value::from(fuel_remaining);
        }
        self.lines.push(obj.to_string());
        self.frame += 1;
        self.seq = 0;
    }

    /// Final record: the world content digest at exit (or crash) point.
    /// Replay verifies its own world against this for an end-to-end check.
    pub fn record_end(&mut self, world_digest: &str) {
        self.lines
            .push(serde_json::json!({"t": "end", "world": world_digest}).to_string());
    }

    pub fn to_jsonl(&self) -> String {
        let mut out = self.lines.join("\n");
        out.push('\n');
        out
    }
}

// ---------------------------------------------------------------------------
// Replayer.
// ---------------------------------------------------------------------------

/// Errors returned when a frame boundary hits `--to-frame` start with this
/// prefix; the CLI treats them as a successful stop, not a runtime error.
pub const REPLAY_STOP_PREFIX: &str = "replay: stopped at start of frame";

#[derive(Debug, Clone)]
pub struct IoRecord {
    pub frame: u64,
    pub seq: u64,
    pub builtin: String,
    pub args_digest: String,
    /// `Ok(tagged value)` for successful io, `Err(message)` for io that
    /// failed in the recorded run (replay reproduces the failure).
    pub result: Result<serde_json::Value, String>,
}

pub struct ReplayReport {
    pub frames_replayed: u64,
    pub io_replayed: usize,
    pub leftover_io: usize,
    /// `None` when the trace carried no end record.
    pub end_digest_match: Option<bool>,
    /// Retro mode only: reads served by repeating a key's last recorded
    /// value after its FIFO queue was exhausted.
    pub reused_reads: usize,
    /// Retro mode only: pure-output write calls the recording never
    /// performed, replayed as virtualized no-ops (see [`ReplayMode::Retro`]).
    pub virtual_writes: usize,
}

/// Pure-output builtins: they emit data and return `nil` on success, so a
/// replay needs no recorded answer to serve them. In retro mode a call whose
/// args don't match any recorded io is virtualized (side effect suppressed,
/// `nil` returned) instead of treated as an oracle hole — changing what a
/// program *writes* is the entire point of a retroactive edit.
fn is_virtualizable_write(builtin: &str) -> bool {
    matches!(builtin, "write_file" | "write_file_bytes" | "append_file")
}

/// How recorded io is served back to the running program.
enum ReplayMode {
    /// Faithful replay: records are consumed strictly in order, with
    /// divergence checks on builtin, args digest, and frame coordinate.
    Strict,
    /// Retroactive replay (list item #6): the trace is an io *oracle* keyed
    /// by `(builtin, args digest)` with FIFO consumption per key — edited
    /// code gets the same answers the recorded world gave, regardless of
    /// reordering. An exhausted key repeats its last value (repeatable-read
    /// semantics: the file didn't change mid-session, the clock freezes at
    /// its last reading). Io the recorded run never performed is a *hole*:
    /// fabricating io would mix timelines, so replay halts loudly.
    Retro {
        oracle: HashMap<(String, String), std::collections::VecDeque<IoRecord>>,
        last_served: HashMap<(String, String), IoRecord>,
        reused: usize,
        virtualized: usize,
    },
}

pub struct TraceReplayer {
    source: String,
    seed: u64,
    records: Vec<IoRecord>,
    /// First record index of each frame — the frame-indexed cursor that
    /// `goto_frame` seeking (Phase 3) repositions instead of re-firing io.
    frame_starts: std::collections::BTreeMap<u64, usize>,
    cursor: usize,
    current_frame: u64,
    stop_at_frame: Option<u64>,
    /// Number of frame-boundary records in the trace — the highest frame
    /// index a replay can stop at (`--to-frame` range check).
    total_frames: u64,
    end_world_digest: Option<String>,
    mode: ReplayMode,
    /// Time travel: when enabled, the VM pushes a CoW world snapshot at
    /// every main-timeline frame boundary. `timeline[k]` is the world at the
    /// start of frame `k` (`timeline[0]` is the empty pre-run world; the
    /// final entry is the world at program end). Snapshots are O(archetypes)
    /// `Arc` bumps, so *every* frame is a keyframe and `goto_frame` is pure
    /// index movement — no re-execution.
    capture_timeline: bool,
    timeline: Vec<std::sync::Arc<crate::world::WorldSnapshot>>,
}

impl std::fmt::Debug for TraceReplayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TraceReplayer")
            .field("records", &self.records.len())
            .field("cursor", &self.cursor)
            .field("current_frame", &self.current_frame)
            .field("timeline_len", &self.timeline.len())
            .finish_non_exhaustive()
    }
}

impl TraceReplayer {
    /// Parse a JSONL trace. Refuses tampered traces (embedded source vs
    /// `source_hash`) unless `force` is set.
    pub fn parse(jsonl: &str, force: bool) -> Result<Self, String> {
        let mut lines = jsonl.lines().filter(|l| !l.trim().is_empty());
        let header: serde_json::Value = lines
            .next()
            .ok_or("trace is empty")
            .and_then(|l| serde_json::from_str(l).map_err(|_| "trace header is not valid JSON"))
            .map_err(|e| e.to_string())?;
        if header["t"] != "header" {
            return Err("trace does not start with a header record".into());
        }
        let version = header["version"].as_u64().unwrap_or(0);
        if version != TRACE_VERSION {
            return Err(format!(
                "trace version {} is not supported (expected {})",
                version, TRACE_VERSION
            ));
        }
        let source = header["source"]
            .as_str()
            .ok_or("trace header has no embedded source")?
            .to_string();
        let recorded_hash = header["source_hash"].as_str().unwrap_or_default();
        if source_hash(&source) != recorded_hash && !force {
            return Err(
                "trace integrity check failed: embedded source does not match source_hash \
                 (the trace was modified). Use --force to replay it anyway."
                    .into(),
            );
        }
        let seed = header["seed"].as_u64().ok_or("trace header has no seed")?;

        let mut records = Vec::new();
        let mut frame_starts = std::collections::BTreeMap::new();
        let mut end_world_digest = None;
        let mut total_frames = 0u64;
        for (i, line) in lines.enumerate() {
            let j: serde_json::Value = serde_json::from_str(line)
                .map_err(|e| format!("trace line {} is not valid JSON: {}", i + 2, e))?;
            match j["t"].as_str() {
                Some("io") => {
                    let frame = j["f"].as_u64().ok_or("io record missing frame")?;
                    let rec = IoRecord {
                        frame,
                        seq: j["s"].as_u64().unwrap_or(0),
                        builtin: j["b"].as_str().unwrap_or_default().to_string(),
                        args_digest: j["a"].as_str().unwrap_or_default().to_string(),
                        result: match j.get("e") {
                            Some(e) => Err(e.as_str().unwrap_or_default().to_string()),
                            None => Ok(j["r"].clone()),
                        },
                    };
                    frame_starts.entry(frame).or_insert(records.len());
                    records.push(rec);
                }
                Some("frame") => total_frames += 1,
                Some("end") => {
                    end_world_digest = j["world"].as_str().map(|s| s.to_string());
                }
                other => {
                    return Err(format!(
                        "trace line {}: unknown record type {:?}",
                        i + 2,
                        other
                    ))
                }
            }
        }
        Ok(Self {
            source,
            seed,
            records,
            frame_starts,
            cursor: 0,
            current_frame: 0,
            stop_at_frame: None,
            total_frames,
            end_world_digest,
            mode: ReplayMode::Strict,
            capture_timeline: false,
            timeline: Vec::new(),
        })
    }

    /// Switch to retroactive mode (see [`ReplayMode::Retro`]): recorded io
    /// becomes an args-keyed oracle so *edited* source can be replayed
    /// against the original session's inputs.
    pub fn into_retro(mut self) -> Self {
        let mut oracle: HashMap<(String, String), std::collections::VecDeque<IoRecord>> =
            HashMap::new();
        for rec in &self.records {
            oracle
                .entry((rec.builtin.clone(), rec.args_digest.clone()))
                .or_default()
                .push_back(rec.clone());
        }
        self.mode = ReplayMode::Retro {
            oracle,
            last_served: HashMap::new(),
            reused: 0,
            virtualized: 0,
        };
        self
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn stop_at(&mut self, frame: u64) {
        self.stop_at_frame = Some(frame);
    }

    /// Frame boundaries recorded in this trace — the highest index
    /// `--to-frame` can honour.
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Range-check a `--to-frame` request. `0..=total_frames` are honest
    /// stop points (0 = stop before anything runs); anything beyond the last
    /// recorded frame boundary can never trigger the stop sentinel — the old
    /// behaviour silently replayed the whole trace and printed "Replay
    /// verified" for a request it did not honour (dogfood finding).
    pub fn validate_stop_frame(&self, frame: u64) -> Result<(), String> {
        if frame > self.total_frames {
            return Err(format!(
                "--to-frame {} is beyond the end of this trace: it records {} frame \
                 boundar{} (valid stop points: 0..={})",
                frame,
                self.total_frames,
                if self.total_frames == 1 { "y" } else { "ies" },
                self.total_frames
            ));
        }
        Ok(())
    }

    /// Enable per-frame keyframing for time-travel sessions.
    pub fn enable_timeline_capture(&mut self) {
        self.capture_timeline = true;
    }

    pub fn capturing_timeline(&self) -> bool {
        self.capture_timeline
    }

    pub fn push_timeline_snapshot(&mut self, snap: crate::world::WorldSnapshot) {
        self.timeline.push(std::sync::Arc::new(snap));
    }

    /// The captured timeline: `timeline()[k]` = world at start of frame `k`,
    /// last entry = world at program end.
    pub fn take_timeline(&mut self) -> Vec<std::sync::Arc<crate::world::WorldSnapshot>> {
        std::mem::take(&mut self.timeline)
    }

    pub fn io_record_count(&self) -> usize {
        self.records.len()
    }

    pub fn end_world_digest(&self) -> Option<&str> {
        self.end_world_digest.as_deref()
    }

    /// Serve the next io record. Strict mode halts loudly on any divergence
    /// between the recorded and replayed timelines; retro mode answers from
    /// the args-keyed oracle.
    pub fn next_io(&mut self, builtin: &str, args_digest: &str) -> Result<IoRecord, String> {
        match &mut self.mode {
            ReplayMode::Strict => {
                let rec = self.records.get(self.cursor).ok_or_else(|| {
                    format!(
                        "replay divergence at frame {}: the replayed run calls {}() but the \
                         recorded run performed no further io",
                        self.current_frame, builtin
                    )
                })?;
                if rec.builtin != builtin
                    || rec.args_digest != args_digest
                    || rec.frame != self.current_frame
                {
                    return Err(format!(
                        "replay divergence at frame {}, record #{}: recorded {}(args {}) in frame {}, \
                         replayed {}(args {})",
                        self.current_frame,
                        self.cursor,
                        rec.builtin,
                        rec.args_digest,
                        rec.frame,
                        builtin,
                        args_digest
                    ));
                }
                let rec = rec.clone();
                self.cursor += 1;
                Ok(rec)
            }
            ReplayMode::Retro {
                oracle,
                last_served,
                reused,
                virtualized,
            } => {
                let key = (builtin.to_string(), args_digest.to_string());
                if let Some(queue) = oracle.get_mut(&key) {
                    if let Some(rec) = queue.pop_front() {
                        last_served.insert(key, rec.clone());
                        self.cursor += 1;
                        return Ok(rec);
                    }
                }
                if let Some(rec) = last_served.get(&key) {
                    // Repeatable read: the key was recorded, just fewer
                    // times than the edited code asks for.
                    *reused += 1;
                    return Ok(rec.clone());
                }
                if is_virtualizable_write(builtin) {
                    // The edit changed what the program writes (payload or
                    // path). Writes consume nothing from the recorded world,
                    // so there is nothing to fabricate: suppress the side
                    // effect (replay never performs real io) and return the
                    // builtin's success value.
                    *virtualized += 1;
                    return Ok(IoRecord {
                        frame: self.current_frame,
                        seq: 0,
                        builtin: builtin.to_string(),
                        args_digest: args_digest.to_string(),
                        result: Ok(serde_json::json!({"t": "nil"})),
                    });
                }
                // A read the recorded world never answered is a genuine
                // hole. Say precisely which kind: "same builtin, different
                // arguments" sends the user to their edit; "never called"
                // sends them to the recording.
                let same_builtin = self.records.iter().filter(|r| r.builtin == builtin).count();
                if same_builtin > 0 {
                    Err(format!(
                        "retroactive replay hole at frame {}: the edited program calls \
                         {}(args {}) — the recorded session called {}() {} time(s) but never \
                         with these arguments, and replay cannot fabricate answers from a \
                         world it never saw",
                        self.current_frame, builtin, args_digest, builtin, same_builtin
                    ))
                } else {
                    Err(format!(
                        "retroactive replay hole at frame {}: the edited program calls {}() \
                         but the recorded session never called it — replay cannot fabricate \
                         answers from a world it never saw",
                        self.current_frame, builtin
                    ))
                }
            }
        }
    }

    /// Advance the frame counter at a main-timeline `flush_events` flip.
    /// Returns the stop sentinel when `--to-frame` is reached.
    pub fn advance_frame(&mut self) -> Option<String> {
        self.current_frame += 1;
        if self.stop_at_frame == Some(self.current_frame) {
            return Some(format!("{} {}", REPLAY_STOP_PREFIX, self.current_frame));
        }
        None
    }

    /// Reposition the io cursor to the first record of `frame` — used by
    /// keyframe seeking: restore a snapshot, seek the cursor, re-execute.
    pub fn seek_frame(&mut self, frame: u64) {
        self.current_frame = frame;
        self.cursor = self
            .frame_starts
            .range(frame..)
            .next()
            .map(|(_, &idx)| idx)
            .unwrap_or(self.records.len());
    }

    pub fn report(&self, world_digest: &str) -> ReplayReport {
        ReplayReport {
            frames_replayed: self.current_frame,
            io_replayed: self.cursor,
            leftover_io: self.records.len() - self.cursor,
            end_digest_match: self.end_world_digest.as_ref().map(|d| d == world_digest),
            reused_reads: match &self.mode {
                ReplayMode::Strict => 0,
                ReplayMode::Retro { reused, .. } => *reused,
            },
            virtual_writes: match &self.mode {
                ReplayMode::Strict => 0,
                ReplayMode::Retro { virtualized, .. } => *virtualized,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::VM;

    fn record_run_raw(src: &str) -> (Result<(), String>, Vec<serde_json::Value>) {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        let result = Compiler::new().compile(&program).expect("compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.set_random_seed(7);
        vm.enable_recording(src);
        vm.load_compile_result(result);
        let run_result = vm.run(0);
        let trace = vm.take_trace().expect("trace");
        let lines = trace
            .lines()
            .map(|l| serde_json::from_str(l).expect("valid json line"))
            .collect();
        (run_result, lines)
    }

    fn record_run(src: &str) -> Vec<serde_json::Value> {
        let (result, lines) = record_run_raw(src);
        result.expect("run");
        lines
    }

    #[test]
    fn codec_roundtrips_every_data_kind() {
        let mut vm = VM::new();
        let gc = &mut vm.gc;

        let s = Value::from_string(gc, "hello \"quoted\"\nline".to_string());
        let li = {
            let one = Value::from_int(gc, 1);
            let pi = Value::from_float(3.5);
            Value::list(gc, vec![one, pi, Value::NIL, Value::from_bool(true)])
        };
        let m = {
            let mut storage = MapStorage::new();
            let v1 = Value::from_int(gc, 10);
            let v2 = Value::from_int(gc, 20);
            storage.insert(MapKey::Str("a".into()), v1);
            storage.insert(MapKey::Int(5), v2);
            Value::map(gc, storage)
        };
        let st = {
            let mut fields = HashMap::new();
            let inner = Value::from_int(gc, 99);
            fields.insert("value".to_string(), inner);
            Value::sum_type(gc, "Result".into(), "Ok".into(), fields)
        };
        let comp = {
            let x = Value::from_int(gc, 4);
            Value::component(
                gc,
                "Pos".into(),
                std::sync::Arc::new(vec!["x".to_string()]),
                vec![x],
            )
        };
        let ent = Value::from_entity_id(gc, 3);

        for v in [s, li, m, st, comp, ent] {
            let encoded = encode_value(&v).expect("encode");
            let decoded = decode_value(&mut vm.gc, &encoded).expect("decode");
            assert_eq!(
                format!("{}", v),
                format!("{}", decoded),
                "roundtrip changed value: {}",
                encoded
            );
        }
    }

    #[test]
    fn recorder_captures_io_and_frames_but_not_pure_builtins() {
        let dir = std::env::temp_dir().join("rad_replay_p1_test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("data.txt").to_string_lossy().replace('\\', "/");
        std::fs::write(dir.join("data.txt"), "payload").unwrap();

        let src = format!(
            r#"
            event Tick {{ n }}
            on Tick(e) {{
                print(e.n)
            }}
            print(rand_int(1, 100))
            let c = clock()
            let body = read_file("{file}")
            emit Tick {{ n: 1 }}
            flush_events()
            let c2 = now_unix_ms()
            print(len(str(c) + str(c2)))
            "#
        );
        let lines = record_run(&src);

        assert_eq!(lines[0]["t"], "header");
        assert_eq!(lines[0]["version"], 1);
        assert_eq!(lines[0]["seed"], 7);
        assert_eq!(lines[0]["source_hash"], source_hash(&src));

        let ios: Vec<&serde_json::Value> = lines.iter().filter(|l| l["t"] == "io").collect();
        let names: Vec<&str> = ios.iter().map(|l| l["b"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec!["clock", "read_file", "now_unix_ms"],
            "exactly the boundary-crossing builtins, in program order"
        );
        // rand_int and print must not appear: pure xorshift / deterministic output.

        // read_file result is the tagged file payload.
        let rf = ios[1];
        assert_eq!(rf["r"]["t"], "str");
        assert_eq!(rf["r"]["v"], "payload");
        assert_eq!(rf["a"].as_str().unwrap().len(), 16);

        // Frame accounting: clock + read_file land in frame 0, the
        // now_unix_ms after flush_events lands in frame 1 with seq reset.
        assert_eq!(ios[0]["f"], 0);
        assert_eq!(ios[0]["s"], 0);
        assert_eq!(ios[1]["f"], 0);
        assert_eq!(ios[1]["s"], 1);
        assert_eq!(ios[2]["f"], 1);
        assert_eq!(ios[2]["s"], 0);

        let frames: Vec<&serde_json::Value> = lines.iter().filter(|l| l["t"] == "frame").collect();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["n"], 0);
    }

    #[test]
    fn recorder_captures_io_failures_as_errors() {
        // read_file on a missing path is a hard VM error. The trace must
        // still capture it so replay can reproduce the same failure without
        // touching the file system.
        let src = r#"
            let missing = read_file("h:/definitely/not/a/real/path/x.txt")
            print(missing)
        "#;
        let (result, lines) = record_run_raw(src);
        assert!(result.is_err(), "missing file must error");
        let ios: Vec<&serde_json::Value> = lines.iter().filter(|l| l["t"] == "io").collect();
        assert_eq!(ios.len(), 1);
        assert_eq!(ios[0]["b"], "read_file");
        assert!(
            ios[0].get("r").is_none(),
            "failed io must not record a result"
        );
        assert!(
            ios[0]["e"].as_str().unwrap().contains("read_file() failed"),
            "error text must be captured: {}",
            ios[0]
        );
    }

    fn compile_and_make_vm(src: &str) -> VM {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        let result = Compiler::new().compile(&program).expect("compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.load_compile_result(result);
        vm
    }

    /// Record a run, then replay its trace in a fresh VM. Returns
    /// (recorded prints, recorded digest, replayed prints, replayed digest, report).
    fn roundtrip(src: &str) -> (Vec<String>, String, Vec<String>, String, ReplayReport) {
        let mut rec_vm = compile_and_make_vm(src);
        rec_vm.enable_recording(src);
        rec_vm.run(0).expect("recorded run");
        let rec_prints = rec_vm.print_buffer.clone();
        let rec_digest = rec_vm.world.content_digest();
        let trace = rec_vm.take_trace().expect("trace");

        let replayer = TraceReplayer::parse(&trace, false).expect("parse trace");
        let mut rep_vm = compile_and_make_vm(replayer.source());
        rep_vm.enable_replay(replayer);
        rep_vm.run(0).expect("replayed run");
        let rep_prints = rep_vm.print_buffer.clone();
        let rep_digest = rep_vm.world.content_digest();
        let report = rep_vm.finish_replay().expect("report");
        (rec_prints, rec_digest, rep_prints, rep_digest, report)
    }

    const ROUNDTRIP_SRC_TEMPLATE: &str = r#"
        component Pos { x: 0 }
        resource Score { total: 0 }
        event Bump { who }
        on Bump(e) {
            let p = get(e.who, Pos) |> unwrap
            set(e.who, Pos { x: p.x + 100 })
        }
        let hero = spawn("hero", Pos { x: rand_int(1, 40) })
        let started = clock()
        let cfg = read_file("__FILE__")
        print(cfg)
        emit Bump { who: hero }
        flush_events()
        set_resource(Score, Score { total: rand_int(1, 1000000) })
        flush_events()
        print((get(hero, Pos) |> unwrap).x)
        print(clock() >= started)
    "#;

    /// Each caller passes a unique subdir: tests run in parallel and one of
    /// them deletes its file mid-test, so sharing a path is a race.
    fn roundtrip_src(subdir: &str) -> String {
        let dir = std::env::temp_dir().join(subdir);
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("cfg.txt"), "cfg-payload").unwrap();
        let file = dir.join("cfg.txt").to_string_lossy().replace('\\', "/");
        ROUNDTRIP_SRC_TEMPLATE.replace("__FILE__", &file)
    }

    #[test]
    fn replay_roundtrip_is_byte_identical() {
        let src = roundtrip_src("rad_replay_p2_roundtrip");
        let (rec_prints, rec_digest, rep_prints, rep_digest, report) = roundtrip(&src);
        assert_eq!(rec_prints, rep_prints, "print buffers must match exactly");
        assert_eq!(rec_digest, rep_digest, "world digests must match exactly");
        assert_eq!(report.end_digest_match, Some(true));
        assert_eq!(report.leftover_io, 0);
        assert_eq!(report.frames_replayed, 2);
        assert_eq!(report.io_replayed, 3, "clock, read_file, clock");
    }

    #[test]
    fn replay_never_refires_io() {
        // Record with the file present, then DELETE it. Replay must still
        // produce the recorded payload: io is served from the trace, never
        // re-executed.
        let src = roundtrip_src("rad_replay_p2_refire");
        let mut rec_vm = compile_and_make_vm(&src);
        rec_vm.enable_recording(&src);
        rec_vm.run(0).expect("recorded run");
        let trace = rec_vm.take_trace().expect("trace");

        std::fs::remove_file(
            std::env::temp_dir()
                .join("rad_replay_p2_refire")
                .join("cfg.txt"),
        )
        .expect("delete the file out from under the replay");

        let replayer = TraceReplayer::parse(&trace, false).expect("parse");
        let mut rep_vm = compile_and_make_vm(replayer.source());
        rep_vm.enable_replay(replayer);
        rep_vm
            .run(0)
            .expect("replay must not touch the file system");
        assert_eq!(rep_vm.print_buffer[0], "cfg-payload");
    }

    #[test]
    fn replay_halts_on_digest_divergence() {
        let src = roundtrip_src("rad_replay_p2_divergence");
        let mut rec_vm = compile_and_make_vm(&src);
        rec_vm.enable_recording(&src);
        rec_vm.run(0).expect("recorded run");
        let trace = rec_vm.take_trace().expect("trace");

        // Tamper with one recorded args digest: simulates the replayed run
        // computing different io arguments than the recorded one.
        let mut lines: Vec<serde_json::Value> = trace
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let idx = lines
            .iter()
            .position(|l| l["t"] == "io" && l["b"] == "read_file")
            .expect("read_file record");
        lines[idx]["a"] = serde_json::Value::String("deadbeefdeadbeef".into());
        let tampered = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(trace.trim(), tampered, "tamper must change the trace");

        let replayer = TraceReplayer::parse(&tampered, false).expect("parse");
        let mut rep_vm = compile_and_make_vm(replayer.source());
        rep_vm.enable_replay(replayer);
        let err = rep_vm.run(0).expect_err("divergence must halt the replay");
        assert!(err.contains("replay divergence"), "got: {}", err);
        assert!(err.contains("read_file"), "got: {}", err);
    }

    #[test]
    fn tampered_source_is_refused_without_force() {
        let src = roundtrip_src("rad_replay_p2_tamper");
        let mut rec_vm = compile_and_make_vm(&src);
        rec_vm.enable_recording(&src);
        rec_vm.run(0).expect("recorded run");
        let trace = rec_vm.take_trace().expect("trace");

        let tampered = trace.replace("rand_int(1, 40)", "rand_int(1, 41)");
        assert_ne!(trace, tampered);

        let err = TraceReplayer::parse(&tampered, false).expect_err("must refuse");
        assert!(err.contains("integrity"), "got: {}", err);
        assert!(err.contains("--force"), "got: {}", err);
        // --force overrides the refusal (and the divergence machinery is
        // still armed downstream).
        assert!(TraceReplayer::parse(&tampered, true).is_ok());
    }

    #[test]
    fn replay_reproduces_recorded_io_failures() {
        let src = r#"
            let t = clock()
            let missing = read_file("h:/definitely/not/a/real/path/x.txt")
            print(missing)
        "#;
        let mut rec_vm = compile_and_make_vm(src);
        rec_vm.enable_recording(src);
        let rec_err = rec_vm.run(0).expect_err("recorded run crashes");
        let trace = rec_vm.take_trace().expect("trace");

        let replayer = TraceReplayer::parse(&trace, false).expect("parse");
        let mut rep_vm = compile_and_make_vm(replayer.source());
        rep_vm.enable_replay(replayer);
        let rep_err = rep_vm.run(0).expect_err("replay reproduces the crash");
        assert_eq!(rec_err, rep_err, "the crash must be reproduced verbatim");
        let report = rep_vm.finish_replay().expect("report");
        assert_eq!(
            report.end_digest_match,
            Some(true),
            "world at crash point must match"
        );
    }

    #[test]
    fn stop_at_frame_halts_at_frame_start() {
        let src = r#"
            resource Counter { n: 0 }
            event Tick {}
            on Tick(_e) {
                let c = get_resource(Counter) |> unwrap
                set_resource(Counter, Counter { n: c.n + 1 })
            }
            for _i in range(0, 5) {
                emit Tick {}
                flush_events()
            }
        "#;
        let mut rec_vm = compile_and_make_vm(src);
        rec_vm.enable_recording(src);
        rec_vm.run(0).expect("recorded run");
        let trace = rec_vm.take_trace().expect("trace");

        let mut replayer = TraceReplayer::parse(&trace, false).expect("parse");
        replayer.stop_at(2);
        let mut rep_vm = compile_and_make_vm(src);
        rep_vm.enable_replay(replayer);
        let err = rep_vm.run(0).expect_err("stop sentinel");
        // The VM decorates propagated errors with call-site context, so the
        // sentinel is matched with contains(), not starts_with().
        assert!(err.contains(REPLAY_STOP_PREFIX), "got: {}", err);
        // Handlers dispatched by flush #k belong to frame k: frame 0 only
        // emitted, frame 1 dispatched the first Tick. Stopping at the start
        // of frame 2 therefore shows exactly ONE tick applied.
        let mut twin = compile_and_make_vm(
            "resource Counter { n: 0 }\nset_resource(Counter, Counter { n: 1 })",
        );
        twin.run(0).expect("twin");
        assert_eq!(
            rep_vm.world.content_digest(),
            twin.world.content_digest(),
            "state at start of frame 2 must show exactly 1 dispatched tick"
        );
        let report = rep_vm.finish_replay().expect("report");
        assert_eq!(report.frames_replayed, 2);
    }

    #[test]
    fn seek_frame_repositions_the_io_cursor() {
        // Build a trace with io in frames 0, 1, and 3 (none in 2).
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_io(
            "clock",
            "d0".into(),
            &Ok(serde_json::json!({"t":"int","v":0})),
        );
        rec.record_frame(u64::MAX); // -> frame 1
        rec.record_io(
            "clock",
            "d1".into(),
            &Ok(serde_json::json!({"t":"int","v":1})),
        );
        rec.record_io(
            "clock",
            "d2".into(),
            &Ok(serde_json::json!({"t":"int","v":2})),
        );
        rec.record_frame(u64::MAX); // -> frame 2
        rec.record_frame(u64::MAX); // -> frame 3
        rec.record_io(
            "clock",
            "d3".into(),
            &Ok(serde_json::json!({"t":"int","v":3})),
        );
        let jsonl = rec.to_jsonl();

        let mut rep = TraceReplayer::parse(&jsonl, false).expect("parse");
        // Seek into frame 1: next io must be d1.
        rep.seek_frame(1);
        let r = rep.next_io("clock", "d1").expect("d1");
        assert_eq!(r.seq, 0);
        // Seek to frame 2 (no io): cursor lands on frame 3's record.
        rep.seek_frame(2);
        rep.advance_frame(); // 2 -> 3
        let r = rep.next_io("clock", "d3").expect("d3");
        assert_eq!(r.frame, 3);
        // Seek past the end: any further io is a divergence.
        rep.seek_frame(99);
        assert!(rep.next_io("clock", "dX").is_err());
    }

    #[test]
    fn retro_oracle_fifo_repeat_last_and_holes() {
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_io(
            "clock",
            "dX".into(),
            &Ok(serde_json::json!({"t":"int","v":1})),
        );
        rec.record_io(
            "clock",
            "dX".into(),
            &Ok(serde_json::json!({"t":"int","v":2})),
        );
        let mut rep = TraceReplayer::parse(&rec.to_jsonl(), false)
            .expect("parse")
            .into_retro();

        // FIFO per key: same question, answers in recorded order.
        assert_eq!(rep.next_io("clock", "dX").unwrap().result.unwrap()["v"], 1);
        assert_eq!(rep.next_io("clock", "dX").unwrap().result.unwrap()["v"], 2);
        // Exhausted key: repeatable read of the last value.
        assert_eq!(rep.next_io("clock", "dX").unwrap().result.unwrap()["v"], 2);
        assert_eq!(rep.report("x").reused_reads, 1);
        // Never-recorded key: a hole, loud.
        let err = rep.next_io("read_file", "dY").expect_err("hole");
        assert!(err.contains("hole"), "got: {}", err);
        assert!(err.contains("read_file"), "got: {}", err);
    }

    /// A4 BUG 06: `--to-frame 0` and any N beyond the last recorded frame
    /// boundary used to be silently dropped — the whole trace ran and the
    /// tool printed "Replay verified" for a request it did not honour.
    #[test]
    fn to_frame_range_is_validated_against_the_trace() {
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_frame(u64::MAX);
        rec.record_frame(u64::MAX);
        rec.record_frame(u64::MAX);
        let rep = TraceReplayer::parse(&rec.to_jsonl(), false).expect("parse");
        assert_eq!(rep.total_frames(), 3);
        // 0 (run nothing) through 3 (the last boundary) are honest stops.
        for n in 0..=3 {
            assert!(rep.validate_stop_frame(n).is_ok(), "n={}", n);
        }
        for n in [4u64, 50, 100000] {
            let err = rep.validate_stop_frame(n).expect_err("out of range");
            assert!(err.contains(&format!("--to-frame {}", n)), "got: {}", err);
            assert!(err.contains("3 frame boundaries"), "got: {}", err);
            assert!(err.contains("0..=3"), "got: {}", err);
        }
        // A trace that never flushed has no boundary to stop at.
        let empty = TraceRecorder::new("src", 1);
        let rep = TraceReplayer::parse(&empty.to_jsonl(), false).expect("parse");
        assert_eq!(rep.total_frames(), 0);
        assert!(rep.validate_stop_frame(0).is_ok());
        assert!(rep.validate_stop_frame(1).is_err());
    }

    /// A4 BUG 05 (defect 1): an edited program whose write_file carries a
    /// different payload used to halt as an "oracle hole". Writes consume
    /// nothing from the recorded world — they replay as virtualized no-ops.
    #[test]
    fn retro_virtualizes_changed_writes_instead_of_halting() {
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_io(
            "write_file",
            "d-old".into(),
            &Ok(serde_json::json!({"t":"nil"})),
        );
        let mut rep = TraceReplayer::parse(&rec.to_jsonl(), false)
            .expect("parse")
            .into_retro();

        // Same args as recorded: served from the oracle, not virtualized.
        assert!(rep.next_io("write_file", "d-old").unwrap().result.is_ok());
        // Changed payload: virtualized success (nil), not a hole.
        let rec2 = rep
            .next_io("write_file", "d-new")
            .expect("virtualized write");
        assert_eq!(rec2.result.unwrap()["t"], "nil");
        // A write the recording never performed at all is also fine.
        assert!(rep.next_io("append_file", "d-x").is_ok());
        let report = rep.report("x");
        assert_eq!(report.virtual_writes, 2);
        assert_eq!(report.io_replayed, 1, "only the oracle hit consumes");
    }

    /// A4 BUG 05 (defect 2): the hole diagnostic claimed "the recorded
    /// session never performed that io" even when the same builtin WAS
    /// recorded with different arguments. The two cases now read differently.
    #[test]
    fn retro_hole_diagnostic_distinguishes_changed_args_from_never_called() {
        let mut rec = TraceRecorder::new("src", 1);
        rec.record_io(
            "read_file",
            "d-a".into(),
            &Ok(serde_json::json!({"t":"str","v":"x"})),
        );
        let mut rep = TraceReplayer::parse(&rec.to_jsonl(), false)
            .expect("parse")
            .into_retro();

        let err = rep.next_io("read_file", "d-b").expect_err("changed args");
        assert!(
            err.contains("called read_file() 1 time(s) but never with these arguments"),
            "got: {}",
            err
        );
        let err = rep.next_io("http_get", "d-c").expect_err("never called");
        assert!(err.contains("never called it"), "got: {}", err);
        assert!(!err.contains("never performed that io"), "got: {}", err);
    }

    #[test]
    fn retro_replay_serves_recorded_io_to_edited_code() {
        // Record with the original source, DELETE the file, then replay an
        // EDITED source (different spawn count) against the recorded inputs.
        let dir = std::env::temp_dir().join("rad_replay_p6_test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("cfg.txt"), "retro-payload").unwrap();
        let file = dir.join("cfg.txt").to_string_lossy().replace('\\', "/");

        let original = format!(
            r#"
            component Pos {{ x: 0 }}
            let a = spawn("a", Pos {{ x: 1 }})
            print(read_file("{file}"))
            flush_events()
            "#
        );
        let edited = format!(
            r#"
            component Pos {{ x: 0 }}
            let a = spawn("a", Pos {{ x: 1 }})
            let b = spawn("b", Pos {{ x: 2 }})
            print(read_file("{file}"))
            print(read_file("{file}"))
            flush_events()
            "#
        );

        let mut rec_vm = compile_and_make_vm(&original);
        rec_vm.enable_recording(&original);
        rec_vm.run(0).expect("recorded run");
        let world_a = rec_vm.world.snapshot();
        let trace = rec_vm.take_trace().expect("trace");

        std::fs::remove_file(dir.join("cfg.txt")).expect("delete file");

        let retro = TraceReplayer::parse(&trace, false)
            .expect("parse")
            .into_retro();
        let mut vm_b = compile_and_make_vm(&edited);
        vm_b.enable_replay(retro);
        vm_b.run(0).expect("retro run");
        // Both reads served from the oracle (second via repeatable read),
        // even though the file is gone.
        assert_eq!(vm_b.print_buffer, vec!["retro-payload", "retro-payload"]);
        let report = vm_b.finish_replay().expect("report");
        assert_eq!(report.reused_reads, 1);
        // The edit's blast radius is visible as a world diff: row "a" is
        // value-identical across both runs, only the new spawn counts.
        let world_b = vm_b.world.snapshot();
        let diff = crate::world::WorldSnapshot::diff_summary(&world_a, &world_b);
        assert_eq!(diff.get("Pos"), Some(&1usize), "diff: {:?}", diff);
    }

    #[test]
    fn retro_replay_shows_the_fix_blast_radius() {
        // The gold-drain bug from the Phase 3 dogfood: record the buggy run,
        // then retroactively replay the FIXED handler. The diff between the
        // two final worlds is exactly the bug's footprint.
        let buggy = r#"
            component Health { hp: 100 }
            component Gold { amount: 50 }
            event Hit { amount }
            let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 50 })
            on Hit(e) {
                let h = get(hero, Health) |> unwrap
                set(hero, Health { hp: h.hp - e.amount })
                if h.hp - e.amount < 80 {
                    set(hero, Gold { amount: 0 })
                }
            }
            for _i in range(0, 4) {
                emit Hit { amount: 10 }
                flush_events()
            }
        "#;
        let fixed = r#"
            component Health { hp: 100 }
            component Gold { amount: 50 }
            event Hit { amount }
            let hero = spawn("hero", Health { hp: 100 }, Gold { amount: 50 })
            on Hit(e) {
                let h = get(hero, Health) |> unwrap
                set(hero, Health { hp: h.hp - e.amount })
            }
            for _i in range(0, 4) {
                emit Hit { amount: 10 }
                flush_events()
            }
        "#;
        let mut rec_vm = compile_and_make_vm(buggy);
        rec_vm.enable_recording(buggy);
        rec_vm.run(0).expect("recorded run");
        let world_buggy = rec_vm.world.snapshot();
        let trace = rec_vm.take_trace().expect("trace");

        let retro = TraceReplayer::parse(&trace, false)
            .expect("parse")
            .into_retro();
        let mut vm_fixed = compile_and_make_vm(fixed);
        vm_fixed.enable_replay(retro);
        vm_fixed.run(0).expect("retro run");

        let world_fixed = vm_fixed.world.snapshot();
        let diff = crate::world::WorldSnapshot::diff_summary(&world_buggy, &world_fixed);
        // Health histories are identical; only the drained Gold differs.
        assert_eq!(diff.get("Gold"), Some(&1usize), "diff: {:?}", diff);
        assert!(!diff.contains_key("Health"), "diff: {:?}", diff);
    }

    #[test]
    fn trace_is_deterministic_across_twin_recorded_runs() {
        let src = r#"
            component Pos { x: 0 }
            let e = spawn("hero", Pos { x: rand_int(1, 50) })
            let t = clock()
            flush_events()
            print(get(e, Pos) |> unwrap)
        "#;
        let run = |_: u32| {
            let lines = record_run(src);
            // Strip the clock's actual reading: wall time legitimately
            // differs. Everything else must match exactly.
            lines
                .into_iter()
                .map(|mut l| {
                    if l["t"] == "io" && l["b"] == "clock" {
                        l["r"] = serde_json::Value::Null;
                    }
                    l.to_string()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(0), run(1));
    }
}
