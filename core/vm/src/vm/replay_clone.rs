//! Graph-preserving heap clone used by observational failed-attempt replay.
//!
//! `Value::deep_copy` is intentionally a tree copier for ordinary detached
//! payloads. A VM fork needs a stronger contract: object aliases and cycles
//! must survive inside the child, while closure capture cells must never keep
//! pointers into the authoritative VM. This module performs an iterative
//! discover/allocate pass followed by a pointer-rewrite pass.

use crate::gc::{CaptureCell, GcHeap};
use crate::value::{ClosureValue, MapKey, Object, RadList, Value};
use crate::world::{OperationalWorldEncoder, WorldSnapshot};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_REPLAY_GRAPH_OBJECTS: usize = 1_000_000;
const MAX_REPLAY_GRAPH_BYTES: usize = 256 * 1024 * 1024;
const MAX_REPLAY_FINGERPRINT_WORLDS: usize = 100_000;
const MAX_REPLAY_FINGERPRINT_EDGES: usize = 4_000_000;

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
            Object::EntityId(entity) => {
                self.digest.update(b"e");
                self.digest.update(entity.to_le_bytes());
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

    #[cfg(test)]
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
#[cfg(test)]
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

pub(crate) struct VmForkCloneContext<'a> {
    target: &'a mut GcHeap,
    objects: HashMap<usize, Value>,
    captures: HashMap<usize, *mut CaptureCell>,
    object_sources: Vec<(Value, Value)>,
    capture_sources: Vec<(*mut CaptureCell, *mut CaptureCell)>,
}

impl<'a> VmForkCloneContext<'a> {
    pub(crate) fn new(target: &'a mut GcHeap) -> Self {
        Self {
            target,
            objects: HashMap::new(),
            captures: HashMap::new(),
            object_sources: Vec::new(),
            capture_sources: Vec::new(),
        }
    }

    /// Clone all roots as one graph. Sharing between different globals,
    /// constant pools, queued events, and task results is preserved.
    pub(crate) fn clone_roots(mut self, roots: &[Value]) -> Result<Vec<Value>, String> {
        self.discover(roots)?;
        self.populate_objects()?;
        self.populate_captures();
        Ok(roots.iter().map(|value| self.rewrite(*value)).collect())
    }

    fn ensure_budget(&self) -> Result<(), String> {
        let nodes = self.objects.len().saturating_add(self.captures.len());
        if nodes > MAX_REPLAY_GRAPH_OBJECTS {
            return Err(format!(
                "attempt replay graph exceeds the {MAX_REPLAY_GRAPH_OBJECTS}-node limit"
            ));
        }
        if self.target.bytes_allocated() > MAX_REPLAY_GRAPH_BYTES {
            return Err(format!(
                "attempt replay graph exceeds the {MAX_REPLAY_GRAPH_BYTES}-byte heap limit"
            ));
        }
        Ok(())
    }

    fn discover(&mut self, roots: &[Value]) -> Result<(), String> {
        enum Pending {
            Value(Value),
            Capture(*mut CaptureCell),
        }

        let mut pending = roots
            .iter()
            .copied()
            .map(Pending::Value)
            .collect::<Vec<_>>();
        while let Some(item) = pending.pop() {
            match item {
                Pending::Capture(source) => {
                    let identity = source as usize;
                    if self.captures.contains_key(&identity) {
                        continue;
                    }
                    let target = self.target.alloc(CaptureCell::new(Value::NIL));
                    self.captures.insert(identity, target);
                    self.capture_sources.push((source, target));
                    self.ensure_budget()?;
                    pending.push(Pending::Value(unsafe { (*source).get() }));
                }
                Pending::Value(source) => {
                    let Some(identity) = source.object_identity() else {
                        continue;
                    };
                    if self.objects.contains_key(&identity) {
                        continue;
                    }
                    // The object tag does not encode the variant, so a tiny
                    // placeholder can safely reserve identity before any
                    // outgoing edge is traversed. That is what breaks cycles.
                    let target = Value::from_object(self.target, Object::BigInt(0));
                    self.objects.insert(identity, target);
                    self.object_sources.push((source, target));
                    self.ensure_budget()?;

                    let Some(object) = source.as_object() else {
                        continue;
                    };
                    match object {
                        Object::List(values) => {
                            pending.extend(values.iter().copied().map(Pending::Value))
                        }
                        Object::Tuple(values) => {
                            pending.extend(values.iter().copied().map(Pending::Value))
                        }
                        Object::Map(values) | Object::MapIter(values, _, _) => {
                            pending.extend(values.values().copied().map(Pending::Value))
                        }
                        Object::Component(component) => {
                            pending.extend(component.values.iter().copied().map(Pending::Value))
                        }
                        Object::SumType(sum) => {
                            pending.extend(sum.fields.values().copied().map(Pending::Value))
                        }
                        Object::Closure(closure) => {
                            pending.extend(closure.captures.iter().copied().map(Pending::Capture))
                        }
                        Object::Cell(cell) => pending.push(Pending::Capture(*cell)),
                        Object::BigInt(_)
                        | Object::Str(_)
                        | Object::State(_)
                        | Object::Fn(_)
                        | Object::BuiltinFn(_)
                        | Object::NativeFn(_)
                        | Object::EntityId(_)
                        | Object::Task(_)
                        | Object::BitSet(_)
                        | Object::Buffer(_)
                        | Object::ByteBuf(_)
                        | Object::SystemRef(_)
                        | Object::WorldFork(_) => {}
                    }
                }
            }
        }
        Ok(())
    }

    fn rewrite(&self, source: Value) -> Value {
        source
            .object_identity()
            .and_then(|identity| self.objects.get(&identity).copied())
            .unwrap_or(source)
    }

    fn rewritten_capture(&self, source: *mut CaptureCell) -> *mut CaptureCell {
        self.captures[&(source as usize)]
    }

    fn populate_objects(&mut self) -> Result<(), String> {
        for (source, mut target) in self.object_sources.clone() {
            let source_object = source.as_object().expect("discovered object remains live");
            let target_ptr = target
                .as_object_mut()
                .expect("fork placeholder belongs to child heap")
                as *mut Object;
            let projected = self.target.projected_object_replacement_bytes(
                target_ptr,
                source_object.accounted_heap_bytes(),
            )?;
            if projected > MAX_REPLAY_GRAPH_BYTES {
                return Err(format!(
                    "attempt replay graph exceeds the {MAX_REPLAY_GRAPH_BYTES}-byte heap limit"
                ));
            }
            let object = match source_object {
                Object::BigInt(value) => Object::BigInt(*value),
                Object::Str(value) => Object::Str(Arc::clone(value)),
                Object::List(values) => Object::List(RadList::new(
                    values.iter().map(|value| self.rewrite(*value)).collect(),
                )),
                Object::Tuple(values) => {
                    Object::Tuple(values.iter().map(|value| self.rewrite(*value)).collect())
                }
                Object::Map(values) => Object::Map(
                    values
                        .iter()
                        .map(|(key, value)| (key.clone(), self.rewrite(*value)))
                        .collect(),
                ),
                Object::MapIter(values, index, keys) => Object::MapIter(
                    values
                        .iter()
                        .map(|(key, value)| (key.clone(), self.rewrite(*value)))
                        .collect(),
                    std::cell::Cell::new(index.get()),
                    keys.clone(),
                ),
                Object::Component(component) => Object::Component(crate::value::ComponentData {
                    type_name: component.type_name.clone(),
                    layout: Arc::clone(&component.layout),
                    values: component
                        .values
                        .iter()
                        .map(|value| self.rewrite(*value))
                        .collect(),
                }),
                Object::State(state) => Object::State(state.clone()),
                Object::SumType(sum) => Object::SumType(crate::value::SumTypeInst {
                    type_name: sum.type_name.clone(),
                    variant: sum.variant.clone(),
                    fields: sum
                        .fields
                        .iter()
                        .map(|(name, value)| (name.clone(), self.rewrite(*value)))
                        .collect(),
                }),
                Object::Fn(function) => Object::Fn(function.clone()),
                Object::Closure(closure) => Object::Closure(ClosureValue {
                    name: closure.name.clone(),
                    arity: closure.arity,
                    chunk_id: closure.chunk_id,
                    captures: closure
                        .captures
                        .iter()
                        .map(|capture| self.rewritten_capture(*capture))
                        .collect(),
                }),
                Object::Cell(cell) => Object::Cell(self.rewritten_capture(*cell)),
                Object::BuiltinFn(builtin) => Object::BuiltinFn(*builtin),
                Object::NativeFn(native) => Object::NativeFn(native.clone()),
                Object::EntityId(entity) => Object::EntityId(*entity),
                Object::Task(task) => Object::Task(*task),
                Object::BitSet(words) => Object::BitSet(words.clone()),
                Object::Buffer(value) => Object::Buffer(value.clone()),
                Object::ByteBuf(value) => Object::ByteBuf(value.clone()),
                Object::SystemRef(name) => Object::SystemRef(name.clone()),
                // World snapshots are immutable COW values. The child may
                // restore or branch one, but cannot mutate the Arc payload.
                Object::WorldFork(snapshot) => Object::WorldFork(Arc::clone(snapshot)),
            };
            self.target.replace_accounted_object(target_ptr, object)?;
            self.ensure_budget()?;
        }
        Ok(())
    }

    fn populate_captures(&self) {
        for (source, target) in &self.capture_sources {
            let value = unsafe { (**source).get() };
            unsafe { (**target).set(self.rewrite(value)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fingerprint_roots, fingerprint_roots_with_limits, fingerprint_world_snapshot,
        FingerprintError, FingerprintLimits,
    };
    use crate::causality::WireProvenance;
    use crate::gc::GcHeap;
    use crate::value::Value;
    use crate::world::World;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn operational_world_identity_includes_hidden_allocator_and_type_state() {
        let baseline = World::new().snapshot();
        let visible = baseline.snapshot_json_like();
        let digest = fingerprint_world_snapshot(&baseline);

        let mut next_id = baseline.clone();
        next_id.next_id = 41;
        assert_eq!(visible, next_id.snapshot_json_like());
        assert_ne!(digest, fingerprint_world_snapshot(&next_id));

        let mut free_ids = baseline.clone();
        free_ids.free_ids = vec![7, 3];
        assert_eq!(visible, free_ids.snapshot_json_like());
        let free_digest = fingerprint_world_snapshot(&free_ids);
        assert_ne!(digest, free_digest);
        free_ids.free_ids.reverse();
        assert_ne!(free_digest, fingerprint_world_snapshot(&free_ids));

        let mut types = baseline.clone();
        types.type_registry = Arc::new(HashMap::from([("HiddenType".to_string(), 9)]));
        types.next_type_id = 10;
        assert_eq!(visible, types.snapshot_json_like());
        assert_ne!(digest, fingerprint_world_snapshot(&types));
    }

    #[test]
    fn world_fork_identity_includes_events_timers_provenance_and_seed() {
        fn root_fingerprint(snapshot: crate::world::WorldSnapshot) -> String {
            let mut gc = GcHeap::new();
            let root = Value::world_fork(&mut gc, Arc::new(snapshot));
            fingerprint_roots(&[root]).expect("test fingerprint should fit")
        }

        let baseline = World::new().snapshot();
        let visible = baseline.snapshot_json_like();
        let digest = root_fingerprint(baseline.clone());

        let mut event = baseline.clone();
        event.events = Arc::new(vec![("Ping".to_string(), Value::int(1), 11)]);
        assert_eq!(visible, event.snapshot_json_like());
        let event_digest = root_fingerprint(event.clone());
        assert_ne!(digest, event_digest);
        event.events = Arc::new(vec![("Ping".to_string(), Value::int(2), 11)]);
        assert_ne!(event_digest, root_fingerprint(event));

        let mut delayed = baseline.clone();
        delayed.delayed = Arc::new(vec![(3, "Later".to_string(), Value::int(2), 12)]);
        assert_eq!(visible, delayed.snapshot_json_like());
        let delayed_digest = root_fingerprint(delayed.clone());
        assert_ne!(digest, delayed_digest);
        delayed.delayed = Arc::new(vec![(4, "Later".to_string(), Value::int(2), 12)]);
        assert_ne!(delayed_digest, root_fingerprint(delayed));

        let mut provenance = baseline.clone();
        provenance.provenance = Some(Arc::new(WireProvenance {
            origin: "remote-a".to_string(),
            ..WireProvenance::default()
        }));
        assert_eq!(visible, provenance.snapshot_json_like());
        assert_ne!(digest, root_fingerprint(provenance));

        let mut seeded = baseline;
        seeded.rollout_seed = Some(99);
        assert_eq!(visible, seeded.snapshot_json_like());
        assert_ne!(digest, root_fingerprint(seeded));
    }

    #[test]
    fn world_fork_fingerprint_preserves_snapshot_sharing_topology() {
        let snapshot = Arc::new(World::new().snapshot());

        let mut shared_gc = GcHeap::new();
        let shared_left = Value::world_fork(&mut shared_gc, Arc::clone(&snapshot));
        let shared_right = Value::world_fork(&mut shared_gc, Arc::clone(&snapshot));
        assert_eq!(shared_left, shared_right);
        let shared = fingerprint_roots(&[shared_left, shared_right]).expect("shared graph fits");

        let mut distinct_gc = GcHeap::new();
        let distinct_left = Value::world_fork(&mut distinct_gc, Arc::new((*snapshot).clone()));
        let distinct_right = Value::world_fork(&mut distinct_gc, Arc::new((*snapshot).clone()));
        assert_ne!(distinct_left, distinct_right);
        let distinct =
            fingerprint_roots(&[distinct_left, distinct_right]).expect("distinct graph fits");

        assert_ne!(shared, distinct);
    }

    #[test]
    fn deep_copy_keeps_world_snapshot_topology() {
        let mut source_gc = GcHeap::new();
        let snapshot = Arc::new(World::new().snapshot());
        let original = Value::world_fork(&mut source_gc, Arc::clone(&snapshot));
        let copy = original.deep_copy(&mut source_gc);
        assert_eq!(original, copy);

        let same_snapshot = fingerprint_roots(&[original, copy]).expect("shared graph fits");
        let distinct = Value::world_fork(&mut source_gc, Arc::new((*snapshot).clone()));
        let distinct_snapshot =
            fingerprint_roots(&[original, distinct]).expect("distinct graph fits");
        assert_ne!(same_snapshot, distinct_snapshot);
    }

    #[test]
    fn shared_world_snapshot_dag_is_fingerprinted_by_distinct_nodes() {
        let mut gc = GcHeap::new();
        let mut snapshot = Arc::new(World::new().snapshot());
        for depth in 0..18 {
            let left = Value::world_fork(&mut gc, Arc::clone(&snapshot));
            let right = Value::world_fork(&mut gc, Arc::clone(&snapshot));
            let mut next = World::new().snapshot();
            next.events = Arc::new(vec![
                (format!("left-{depth}"), left, depth * 2),
                (format!("right-{depth}"), right, depth * 2 + 1),
            ]);
            snapshot = Arc::new(next);
        }
        let root = Value::world_fork(&mut gc, snapshot);
        let limits = FingerprintLimits {
            max_nodes: 128,
            max_worlds: 32,
            max_edges: 256,
            max_pending: 128,
            max_encoded_bytes: 1024 * 1024,
        };
        fingerprint_roots_with_limits(&[root], limits)
            .expect("shared DAG work should be linear in distinct snapshots");
    }

    #[test]
    fn deep_world_snapshot_chain_returns_a_typed_limit_error() {
        let mut gc = GcHeap::new();
        let mut snapshot = Arc::new(World::new().snapshot());
        for depth in 0..10_000 {
            let parent = Value::world_fork(&mut gc, snapshot);
            let mut next = World::new().snapshot();
            next.events = Arc::new(vec![(format!("depth-{depth}"), parent, depth)]);
            snapshot = Arc::new(next);
        }
        let root = Value::world_fork(&mut gc, snapshot);
        let limits = FingerprintLimits {
            max_nodes: 1_000,
            max_worlds: 32,
            max_edges: 256,
            max_pending: 1_000,
            max_encoded_bytes: 1024 * 1024,
        };
        assert_eq!(
            fingerprint_roots_with_limits(&[root], limits),
            Err(FingerprintError::LimitExceeded {
                resource: "world-snapshot",
                limit: 32,
            })
        );
    }
}
