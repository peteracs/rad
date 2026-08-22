// Bounded content fingerprints over a value graph: the metered digest, the
// graph walker, and the roots callers fingerprint against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FingerprintLimits {
    pub(crate) max_nodes: usize,
    pub(crate) max_worlds: usize,
    pub(crate) max_edges: usize,
    pub(crate) max_pending: usize,
    pub(crate) max_encoded_bytes: usize,
}

impl Default for FingerprintLimits {
    fn default() -> Self {
        Self {
            max_nodes: MAX_REPLAY_GRAPH_OBJECTS,
            max_worlds: MAX_REPLAY_FINGERPRINT_WORLDS,
            max_edges: MAX_REPLAY_FINGERPRINT_EDGES,
            max_pending: MAX_REPLAY_GRAPH_OBJECTS,
            max_encoded_bytes: MAX_REPLAY_GRAPH_BYTES,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FingerprintError {
    LimitExceeded {
        resource: &'static str,
        limit: usize,
    },
    InvalidObject,
}

impl std::fmt::Display for FingerprintError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitExceeded { resource, limit } => write!(
                formatter,
                "replay fingerprint exceeds the {limit}-{resource} limit"
            ),
            Self::InvalidObject => {
                formatter.write_str("replay fingerprint contains an invalid object")
            }
        }
    }
}

impl std::error::Error for FingerprintError {}

struct MeteredDigest {
    digest: Sha256,
    limits: FingerprintLimits,
    encoded_bytes: usize,
    edges: usize,
    failure: Option<FingerprintError>,
}

impl MeteredDigest {
    fn new(limits: FingerprintLimits) -> Self {
        Self {
            digest: Sha256::new(),
            limits,
            encoded_bytes: 0,
            edges: 0,
            failure: None,
        }
    }

    fn update(&mut self, bytes: impl AsRef<[u8]>) {
        if self.failure.is_some() {
            return;
        }
        let bytes = bytes.as_ref();
        let Some(total) = self.encoded_bytes.checked_add(bytes.len()) else {
            self.failure = Some(FingerprintError::LimitExceeded {
                resource: "encoded-byte",
                limit: self.limits.max_encoded_bytes,
            });
            return;
        };
        if total > self.limits.max_encoded_bytes {
            self.failure = Some(FingerprintError::LimitExceeded {
                resource: "encoded-byte",
                limit: self.limits.max_encoded_bytes,
            });
            return;
        }
        self.encoded_bytes = total;
        self.digest.update(bytes);
    }

    fn edge(&mut self) {
        if self.failure.is_some() {
            return;
        }
        self.edges = self.edges.saturating_add(1);
        if self.edges > self.limits.max_edges {
            self.failure = Some(FingerprintError::LimitExceeded {
                resource: "edge",
                limit: self.limits.max_edges,
            });
        }
    }

    fn fail(&mut self, error: FingerprintError) {
        if self.failure.is_none() {
            self.failure = Some(error);
        }
    }

    fn finish(self) -> Result<String, FingerprintError> {
        if let Some(error) = self.failure {
            Err(error)
        } else {
            Ok(hex::encode(self.digest.finalize()))
        }
    }
}

#[derive(Clone)]
enum FingerprintNode {
    Object(Value),
    Capture(*mut CaptureCell),
    World(Arc<WorldSnapshot>),
}

struct GraphFingerprinter {
    digest: MeteredDigest,
    objects: HashMap<usize, u64>,
    captures: HashMap<usize, u64>,
    worlds: HashMap<usize, u64>,
    pending: Vec<FingerprintNode>,
}

impl GraphFingerprinter {
    fn new(limits: FingerprintLimits) -> Self {
        Self {
            digest: MeteredDigest::new(limits),
            objects: HashMap::new(),
            captures: HashMap::new(),
            worlds: HashMap::new(),
            pending: Vec::new(),
        }
    }

    fn reserve_node(&mut self, worlds: usize) -> bool {
        let limits = self.digest.limits;
        let nodes = self
            .objects
            .len()
            .saturating_add(self.captures.len())
            .saturating_add(self.worlds.len())
            .saturating_add(1);
        if nodes > limits.max_nodes {
            self.digest.fail(FingerprintError::LimitExceeded {
                resource: "node",
                limit: limits.max_nodes,
            });
            return false;
        }
        if worlds > limits.max_worlds {
            self.digest.fail(FingerprintError::LimitExceeded {
                resource: "world-snapshot",
                limit: limits.max_worlds,
            });
            return false;
        }
        if self.pending.len().saturating_add(1) > limits.max_pending {
            self.digest.fail(FingerprintError::LimitExceeded {
                resource: "pending-node",
                limit: limits.max_pending,
            });
            return false;
        }
        true
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.digest.update((bytes.len() as u64).to_le_bytes());
        self.digest.update(bytes);
    }

    fn map_key(&mut self, key: &MapKey) {
        match key {
            MapKey::Bool(value) => self.digest.update([0, *value as u8]),
            MapKey::Int(value) => {
                self.digest.update([1]);
                self.digest.update(value.to_le_bytes());
            }
            MapKey::Native(value) => {
                self.digest.update([5]);
                self.bytes(value.type_name.as_bytes());
                self.bytes(value.repr.to_string().as_bytes());
                self.bytes(value.flavor.as_str().as_bytes());
                self.digest.update(value.bits.to_le_bytes());
            }
            MapKey::Entity(value) => {
                self.digest.update([2]);
                self.digest.update(value.to_le_bytes());
            }
            MapKey::Str(value) => {
                self.digest.update([3]);
                self.bytes(value.as_bytes());
            }
            MapKey::Tuple(values) => {
                self.digest.update([4]);
                self.digest.update((values.len() as u64).to_le_bytes());
                for value in values {
                    self.map_key(value);
                }
            }
        }
    }

    fn capture(&mut self, capture: *mut CaptureCell) {
        self.digest.edge();
        let identity = capture as usize;
        let id = if let Some(id) = self.captures.get(&identity) {
            *id
        } else {
            let id = self.captures.len() as u64;
            if !self.reserve_node(self.worlds.len()) {
                return;
            }
            self.captures.insert(identity, id);
            self.pending.push(FingerprintNode::Capture(capture));
            id
        };
        self.digest.update(b"c");
        self.digest.update(id.to_le_bytes());
    }

    fn value(&mut self, value: Value) {
        self.digest.edge();
        if value.is_nil() {
            self.digest.update(b"n");
        } else if let Some(value) = value.as_bool() {
            self.digest.update([b'b', value as u8]);
        } else if let Some(value) = value.as_int() {
            self.digest.update(b"i");
            self.digest.update(value.to_le_bytes());
        } else if let Some(value) = value.as_float() {
            self.digest.update(b"f");
            self.digest.update(value.to_bits().to_le_bytes());
        } else if let Some(value) = value.as_entity_id() {
            self.digest.update(b"e");
            self.digest.update(value.to_le_bytes());
        } else if let Some(value) = value.as_str() {
            // Strings are immutable, so their identity is not semantically
            // observable and only their canonical content is fingerprinted.
            self.digest.update(b"s");
            self.bytes(value.as_bytes());
        } else if let Some(identity) = value.object_identity() {
            let id = if let Some(id) = self.objects.get(&identity) {
                *id
            } else {
                let id = self.objects.len() as u64;
                if !self.reserve_node(self.worlds.len()) {
                    return;
                }
                self.objects.insert(identity, id);
                self.pending.push(FingerprintNode::Object(value));
                id
            };
            self.digest.update(b"o");
            self.digest.update(id.to_le_bytes());
        } else {
            self.digest.update(b"?");
            self.bytes(value.type_name().as_bytes());
        }
    }

    fn object(&mut self, value: Value) {
        let Some(object) = value.as_object() else {
            self.digest.fail(FingerprintError::InvalidObject);
            return;
        };
        match object {
            Object::BigInt(value) => {
                self.digest.update(b"I");
                self.digest.update(value.to_le_bytes());
            }
            Object::NativeScalar(value) => {
                self.digest.update(b"n");
                self.bytes(value.type_name.as_bytes());
                self.bytes(value.repr.to_string().as_bytes());
                self.digest.update([value.flavor as u8]);
                self.digest.update(value.bits.to_le_bytes());
            }
            Object::NativeType(value) => {
                self.digest.update(b"x");
                self.bytes(value.name.as_bytes());
                self.bytes(value.repr.to_string().as_bytes());
                self.digest.update([value.flavor as u8]);
                for (name, bits) in &value.members {
                    self.bytes(name.as_bytes());
                    self.digest.update(bits.to_le_bytes());
                }
            }
            Object::Str(value) => {
                self.digest.update(b"S");
                self.bytes(value.as_bytes());
            }
            Object::List(values) => {
                self.digest.update(b"L");
                self.digest.update((values.len() as u64).to_le_bytes());
                for value in values.iter() {
                    self.value(*value);
                }
            }
            Object::Tuple(values) => {
                self.digest.update(b"T");
                self.digest.update((values.len() as u64).to_le_bytes());
                for value in values {
                    self.value(*value);
                }
            }
            Object::Map(values) | Object::MapIter(values, _, _) => {
                self.digest
                    .update(if matches!(value.as_object(), Some(Object::Map(_))) {
                        b"M"
                    } else {
                        b"J"
                    });
                let mut entries = values.iter().collect::<Vec<_>>();
                entries.sort_by_key(|(key, _)| (*key).clone());
                self.digest.update((entries.len() as u64).to_le_bytes());
                for (key, value) in entries {
                    self.map_key(key);
                    self.value(*value);
                }
                if let Object::MapIter(_, index, keys) = value.as_object().unwrap() {
                    self.digest.update((index.get() as u64).to_le_bytes());
                    self.digest.update((keys.len() as u64).to_le_bytes());
                    for key in keys {
                        self.map_key(key);
                    }
                }
            }
            Object::Component(component) => {
                self.digest.update(b"C");
                self.bytes(component.type_name.as_bytes());
                self.digest
                    .update((component.layout.len() as u64).to_le_bytes());
                for name in component.layout.iter() {
                    self.bytes(name.as_bytes());
                }
                for value in &component.values {
                    self.value(*value);
                }
            }
            Object::State(state) => {
                self.digest.update(b"Q");
                self.bytes(state.machine.as_bytes());
                self.bytes(state.state.as_bytes());
            }
            Object::SumType(sum) => {
                self.digest.update(b"U");
                self.bytes(sum.type_name.as_bytes());
                self.bytes(sum.variant.as_bytes());
                let mut fields = sum.fields.iter().collect::<Vec<_>>();
                fields.sort_by_key(|(name, _)| (*name).clone());
                self.digest.update((fields.len() as u64).to_le_bytes());
                for (name, value) in fields {
                    self.bytes(name.as_bytes());
                    self.value(*value);
                }
            }
            Object::Fn(function) => {
                self.digest.update(b"F");
                self.bytes(function.name.as_bytes());
                self.digest.update([function.arity]);
                self.digest.update((function.chunk_id as u64).to_le_bytes());
            }
            Object::Closure(closure) => {
                self.digest.update(b"K");
                self.bytes(closure.name.as_bytes());
                self.digest.update([closure.arity]);
                self.digest.update((closure.chunk_id as u64).to_le_bytes());
                self.digest
                    .update((closure.captures.len() as u64).to_le_bytes());
                for capture in &closure.captures {
                    self.capture(*capture);
                }
            }
            Object::Cell(cell) => {
                self.digest.update(b"E");
                self.capture(*cell);
            }
            Object::BuiltinFn(builtin) => {
                self.digest.update(b"B");
                self.bytes(builtin.name().as_bytes());
            }
            Object::NativeFn(native) => {
                self.digest.update(b"N");
                self.bytes(native.extension.digest().as_bytes());
                self.bytes(native.name.as_bytes());
                self.digest.update(native.arity.to_le_bytes());
            }
            Object::HostHandle(handle) => {
                self.text("host_handle[");
                self.text(handle.owner_digest());
                self.byte(b',');
                self.text(handle.type_name());
                self.byte(b',');
                self.u64(handle.token_for_runtime());
                self.byte(b']');
            }
            Object::Task(task) => {
                self.digest.update(b"t");
                self.digest.update(task.to_le_bytes());
            }
            Object::BitSet(words) => {
                self.digest.update(b"D");
                self.digest.update((words.len() as u64).to_le_bytes());
                for word in words {
                    self.digest.update(word.to_le_bytes());
                }
            }
            Object::Buffer(value) => {
                self.digest.update(b"R");
                self.bytes(value.as_bytes());
            }
            Object::ByteBuf(value) => {
                self.digest.update(b"Y");
                self.bytes(value);
            }
            Object::SystemRef(name) => {
                self.digest.update(b"r");
                self.bytes(name.as_bytes());
            }
            Object::WorldFork(snapshot) => {
                self.world(snapshot);
            }
        }
    }

    fn world(&mut self, snapshot: &Arc<WorldSnapshot>) {
        self.digest.edge();
        let identity = Arc::as_ptr(snapshot) as usize;
        let id = if let Some(id) = self.worlds.get(&identity) {
            *id
        } else {
            let id = self.worlds.len() as u64;
            if !self.reserve_node(self.worlds.len().saturating_add(1)) {
                return;
            }
            self.worlds.insert(identity, id);
            self.pending
                .push(FingerprintNode::World(Arc::clone(snapshot)));
            id
        };
        self.digest.update(b"W");
        self.digest.update(id.to_le_bytes());
    }

    fn finish_pending(mut self) -> Result<String, FingerprintError> {
        let mut index = 0;
        while index < self.pending.len() {
            if self.digest.failure.is_some() {
                break;
            }
            match self.pending[index].clone() {
                FingerprintNode::Object(value) => self.object(value),
                FingerprintNode::Capture(capture) => {
                    self.digest.update(b"V");
                    self.value(unsafe { (*capture).get() });
                }
                FingerprintNode::World(snapshot) => {
                    self.digest.update(b"X");
                    snapshot.encode_operational_checkpoint(&mut self);
                }
            }
            index += 1;
        }
        self.digest.finish()
    }

    fn finish(mut self, roots: &[Value]) -> Result<String, FingerprintError> {
        self.digest.update(b"rad-replay-graph/v3\0");
        self.digest.update((roots.len() as u64).to_le_bytes());
        for root in roots {
            self.value(*root);
        }
        self.finish_pending()
    }

    fn finish_world(mut self, snapshot: &WorldSnapshot) -> Result<String, FingerprintError> {
        self.digest
            .update(b"rad-operational-world-fingerprint/v2\0");
        snapshot.encode_operational_checkpoint(&mut self);
        self.finish_pending()
    }

    fn finish_attempt_state(
        mut self,
        roots: &[Value],
        world: &WorldSnapshot,
        timeline: &[WorldSnapshot],
    ) -> Result<String, FingerprintError> {
        self.digest.update(b"rad-attempt-state-graph/v1\0");
        self.digest.update(b"R");
        self.digest.update((roots.len() as u64).to_le_bytes());
        for root in roots {
            self.value(*root);
        }
        self.digest.update(b"A");
        world.encode_operational_checkpoint(&mut self);
        self.digest.update(b"T");
        self.digest.update((timeline.len() as u64).to_le_bytes());
        for snapshot in timeline {
            snapshot.encode_operational_checkpoint(&mut self);
        }
        self.finish_pending()
    }
}

impl OperationalWorldEncoder for GraphFingerprinter {
    fn byte(&mut self, value: u8) {
        self.digest.update([value]);
    }

    fn u32(&mut self, value: u32) {
        self.digest.update(value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.digest.update(value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.digest.update(value.to_le_bytes());
    }

    fn usize(&mut self, value: usize) {
        self.digest.update((value as u64).to_le_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.digest.update([value as u8]);
    }

    fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn value(&mut self, value: Value) {
        GraphFingerprinter::value(self, value);
    }
}

/// Canonical content-and-topology identity for replay-visible VM roots.
/// Mutable aliases and closure captures are numbered by deterministic graph
/// discovery, never by allocator address.
pub(crate) fn fingerprint_roots(roots: &[Value]) -> Result<String, FingerprintError> {
    fingerprint_roots_with_limits(roots, FingerprintLimits::default())
}

pub(crate) fn fingerprint_roots_with_limits(
    roots: &[Value],
    limits: FingerprintLimits,
) -> Result<String, FingerprintError> {
    GraphFingerprinter::new(limits).finish(roots)
}

/// Canonical identity of the complete execution-relevant world snapshot.
/// This deliberately differs from the renderer/content digest.
pub(crate) fn fingerprint_world_snapshot(
    snapshot: &WorldSnapshot,
) -> Result<String, FingerprintError> {
    GraphFingerprinter::new(FingerprintLimits::default()).finish_world(snapshot)
}

/// One topology-preserving identity for every value graph reachable from an
/// attempt checkpoint. A snapshot shared between a global, an event payload,
/// the authoritative world, and a timeline entry receives one discovery ID.
pub(crate) fn fingerprint_attempt_state(
    roots: &[Value],
    world: &WorldSnapshot,
    timeline: &[WorldSnapshot],
) -> Result<String, FingerprintError> {
    GraphFingerprinter::new(FingerprintLimits::default())
        .finish_attempt_state(roots, world, timeline)
}
