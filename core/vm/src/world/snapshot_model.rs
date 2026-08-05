

/// `{"field":value,...}` for one component/resource row (Display-encoded
/// values, escaped keys) — shared by full dumps and render deltas.
fn resource_fields_json(data: &ComponentData) -> String {
    use std::fmt::Write;
    let mut out = String::from("{");
    for (i, (k, v)) in data.layout.iter().zip(data.values.iter()).enumerate() {
        if i > 0 {
            out.push(',');
        }
        let mut key = String::with_capacity(k.len());
        for ch in k.chars() {
            match ch {
                '"' => key.push_str("\\\""),
                '\\' => key.push_str("\\\\"),
                c => key.push(c),
            }
        }
        let _ = write!(&mut out, "\"{}\":{}", key, v);
    }
    out.push('}');
    out
}

/// Shared renderer-facing dump for live worlds and timeline snapshots:
/// `{"entities":[{id,name,components:[{type,fields}]}],"resources":{..}}`.
fn dump_world_json(
    ids: &[u32],
    name_of: impl Fn(u32) -> Option<String>,
    comps_of: impl Fn(u32) -> Vec<ComponentData>,
    resource_names: &[String],
    resource_get: impl Fn(&str) -> Option<ComponentData>,
) -> String {
    use std::fmt::Write;

    fn json_escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => {
                    let _ = write!(&mut out, "\\u{:04x}", c as u32);
                }
                c => out.push(c),
            }
        }
        out
    }

    fn fields_json(layout: &[String], values: &[Value]) -> String {
        let mut out = String::from("{");
        for (i, (k, v)) in layout.iter().zip(values.iter()).enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(&mut out, "\"{}\":{}", json_escape(k), v);
        }
        out.push('}');
        out
    }

    let mut s = String::from("{\"entities\":[");
    for (i, &eid) in ids.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push('{');
        let _ = write!(&mut s, "\"id\":{}", eid);
        if let Some(name) = name_of(eid) {
            let _ = write!(&mut s, ",\"name\":\"{}\"", json_escape(&name));
        } else {
            s.push_str(",\"name\":null");
        }
        s.push_str(",\"components\":[");
        let comps = comps_of(eid);
        for (j, c) in comps.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            s.push('{');
            let _ = write!(
                &mut s,
                "\"type\":\"{}\",\"fields\":{}",
                json_escape(&c.type_name),
                fields_json(&c.layout, &c.values)
            );
            s.push('}');
        }
        s.push_str("]}");
    }
    // Resources are program state too — a GUI renderer reading the world
    // needs e.g. its UiConfig. Same field encoding as components.
    s.push_str("],\"resources\":{");
    for (i, name) in resource_names.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        if let Some(data) = resource_get(name) {
            let _ = write!(
                &mut s,
                "\"{}\":{}",
                json_escape(name),
                fields_json(&data.layout, &data.values)
            );
        }
    }
    s.push_str("}}");
    s
}

/// A sealed operational snapshot. Safe host code can retain and inspect the
/// supported views, but cannot fabricate or replace allocator internals.
///
/// ```compile_fail
/// fn forge(snapshot: rad_vm::world::WorldSnapshot) {
///     let _ = snapshot.next_id;
/// }
/// ```
#[derive(Clone)]
pub struct WorldSnapshot {
    pub(crate) next_id: u32,
    pub(crate) fresh_ids_exhausted: bool,
    pub(crate) free_ids: Arc<BTreeSet<u32>>,
    pub(crate) generations: Arc<HashMap<u32, u32>>,
    pub(crate) name_to_id: Arc<HashMap<String, u32>>,
    pub(crate) id_to_name: Arc<HashMap<u32, String>>,
    pub(crate) type_registry: Arc<HashMap<String, TypeId>>,
    pub(crate) next_type_id: TypeId,
    pub(crate) archetypes: Vec<Archetype>,
    pub(crate) archetype_map: Arc<HashMap<Vec<TypeId>, ArchetypeId>>,
    pub(crate) entity_archetype: Arc<HashMap<u32, ArchetypeId>>,
    indexed_fields: Arc<HashMap<String, HashSet<String>>>,
    indices: Arc<HashMap<IndexKey, Vec<u32>>>,
    resources: Arc<ResourceMap>,
    authoritative_relations: crate::relation_runtime::AuthoritativeRelationState,
    derived_relations: crate::relation_derivation::DerivedRelationState,
    /// In-flight events at capture time: `(event, payload, trace_id)`.
    /// Events are program state — a snapshot that drops them is not a
    /// snapshot. Payloads are persisted on capture. `fork()` fills this,
    /// `commit()` restores it, `simulate()`/sandbox guests seed from it,
    /// and `merge_forks` three-way merges it.
    pub(crate) events: Arc<Vec<(String, Value, u64)>>,
    /// Causality emit-record ids, parallel to `events` (provenance survives
    /// the fork/commit roundtrip).
    pub(crate) emit_ids: Arc<Vec<u64>>,
    /// Delayed (`emit … after N`) timers at capture time: `(ticks_left,
    /// event, payload, emit_id)`. Same principle as `events` — timers are
    /// program state; a snapshot that drops them loses every scheduled
    /// respawn, and the emit id keeps timer causality intact.
    pub(crate) delayed: Arc<Vec<(i64, String, Value, u64)>>,
    /// Foreign provenance riding the snapshot: set by `fork_from_bytes`
    /// (the sender's ledger closure), carried through `merge_forks`, and
    /// ingested into the local ledger by `commit()`. `None` for local forks
    /// — their provenance already lives in the VM's ledger.
    pub(crate) provenance: Option<Arc<crate::causality::WireProvenance>>,
    /// The effective RNG seed the rollout that produced this snapshot ran
    /// under, when it came out of the simulate family (`simulate_par`,
    /// `simulate_many`, `simulate_seeded`). `fork_seed()` reads it, making a
    /// single outlier rollout reproducible in isolation (dogfood feature seq
    /// 150). Local-only debug metadata: never serialized to the world wire or
    /// included in the content digest, but included in operational replay
    /// identity because `fork_seed()` makes it observable. Cleared by
    /// `with_resource` (an overridden copy is a new candidate, not the
    /// rollout's output).
    pub(crate) rollout_seed: Option<u64>,
}

/// Versioned sink for the complete execution-relevant state of a
/// [`WorldSnapshot`]. The snapshot owns the inventory; replay hashing and
/// `WorldFork` graph identity only supply the sink. This prevents the restore
/// and identity paths from growing independent, renderer-shaped field lists.
pub(crate) trait OperationalWorldEncoder {
    fn byte(&mut self, value: u8);
    fn u32(&mut self, value: u32);
    fn u64(&mut self, value: u64);
    fn i64(&mut self, value: i64);
    fn usize(&mut self, value: usize);
    fn bool(&mut self, value: bool);
    fn text(&mut self, value: &str);
    fn value(&mut self, value: Value);

    fn optional_u64(&mut self, value: Option<u64>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.u64(value);
        }
    }

    fn optional_u32(&mut self, value: Option<u32>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.u32(value);
        }
    }

    fn optional_text(&mut self, value: Option<&str>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.text(value);
        }
    }
}
