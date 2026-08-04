

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

/// Effects that an observational failed-attempt replay must never execute.
/// Unlike ledger replay there is no recorded I/O tape for this one request,
/// so fail closed instead of touching the host. VM-local world/global/event
/// effects remain legal because the entire child is discarded.
pub(crate) fn is_observational_attempt_effect(b: Builtin) -> bool {
    use Builtin::*;
    is_replay_managed(b)
        || matches!(
            b,
            Print
                | Eprint
                | WriteStdout
                | WriteStderr
                | FlushStdout
                | DebugTrace
                | Log
                | Metric
                | SleepMs
                | SandboxRun
                | LoadExtension
        )
}

/// Digest of builtin arguments, used purely for divergence detection.
/// Relies on `Display` being deterministic (guaranteed by `determinism.rs`).
pub(crate) fn args_digest(args: &[Value]) -> String {
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

fn canonical_features(features: &[String]) -> Vec<String> {
    let mut features = features.to_vec();
    features.sort();
    features.dedup();
    features
}

fn feature_hash(features: &[String]) -> String {
    let mut digest = blake3::Hasher::new();
    digest.update(b"rad-trace-features/v1\0");
    for feature in canonical_features(features) {
        digest.update(feature.as_bytes());
        digest.update(&[0]);
    }
    digest.finalize().to_hex().to_string()
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
fn trace_u32(value: &serde_json::Value, field: &str) -> Result<u32, String> {
    let raw = value
        .as_u64()
        .ok_or_else(|| format!("trace codec: malformed {}", field))?;
    u32::try_from(raw).map_err(|_| format!("trace codec: {} exceeds u32", field))
}

fn json_to_map_key(karr: &[serde_json::Value]) -> Result<MapKey, String> {
    let tag = karr
        .first()
        .and_then(|value| value.as_str())
        .ok_or_else(|| "trace codec: malformed map key tag".to_string())?;
    let payload = karr
        .get(1)
        .ok_or_else(|| "trace codec: missing map key payload".to_string())?;
    match tag {
        "s" => Ok(MapKey::Str(
            payload
                .as_str()
                .ok_or_else(|| "trace codec: malformed string map key".to_string())?
                .to_string(),
        )),
        "i" => Ok(MapKey::Int(payload.as_i64().ok_or_else(|| {
            "trace codec: malformed integer map key".to_string()
        })?)),
        "b" => Ok(MapKey::Bool(payload.as_bool().ok_or_else(|| {
            "trace codec: malformed boolean map key".to_string()
        })?)),
        "e" => Ok(MapKey::Entity(trace_u32(payload, "entity map key")?)),
        "t" => {
            let items = payload
                .as_array()
                .ok_or_else(|| "trace codec: malformed tuple map key".to_string())?;
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let pair = item
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or_else(|| "trace codec: malformed tuple map key item".to_string())?;
                out.push(json_to_map_key(pair)?);
            }
            Ok(MapKey::Tuple(out))
        }
        _ => Err(format!("trace codec: unknown map key tag '{}'", tag)),
    }
}

pub(crate) fn encode_value(v: &Value) -> Result<serde_json::Value, String> {
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

pub(crate) fn decode_value(gc: &mut GcHeap, j: &serde_json::Value) -> Result<Value, String> {
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
        "entity" => Ok(Value::from_entity_id(gc, trace_u32(&j["v"], "entity id")?)),
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
                let key = json_to_map_key(key_arr)?;
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
        Self::new_with_features(source, seed, &[])
    }

    pub fn new_with_features(source: &str, seed: u64, features: &[String]) -> Self {
        Self::new_with_features_and_layout(source, seed, features, &SourceLayout::default())
    }

    pub fn new_with_features_and_layout(
        source: &str,
        seed: u64,
        features: &[String],
        source_layout: &SourceLayout,
    ) -> Self {
        let features = canonical_features(features);
        let source_layout_hash = source_layout
            .digest(source)
            .expect("recording source layout must describe its source");
        let header = serde_json::json!({
            "t": "header",
            "version": TRACE_VERSION,
            "source": source,
            "source_hash": source_hash(source),
            "features": features,
            "feature_hash": feature_hash(&features),
            "source_layout_version": SOURCE_LAYOUT_VERSION,
            "source_layout": source_layout,
            "source_layout_hash": source_layout_hash,
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

    /// Record both the final world and whether execution returned normally.
    /// A matching world is insufficient when replay crashes before performing
    /// any writes, so current traces authenticate the terminal outcome too.
    pub fn record_end_with_outcome(&mut self, world_digest: &str, error: Option<&str>) {
        let outcome = match error {
            Some(message) => serde_json::json!({"error": message}),
            None => serde_json::json!({"ok": true}),
        };
        self.lines.push(
            serde_json::json!({"t": "end", "world": world_digest, "outcome": outcome}).to_string(),
        );
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
    /// `None` for vintage traces that did not record terminal success/error.
    pub end_outcome_match: Option<bool>,
    /// Retro mode only: reads served by repeating a key's last recorded
    /// value after its FIFO queue was exhausted.
    pub reused_reads: usize,
    /// Retro mode only: pure-output write calls the recording never
    /// performed, replayed as virtualized no-ops (retro replay mode).
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
    source_layout: SourceLayout,
    features: Vec<String>,
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
    /// Outer `None` means a vintage trace; inner `None` means success.
    end_error: Option<Option<String>>,
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