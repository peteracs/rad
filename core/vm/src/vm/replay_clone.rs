//! Graph-preserving heap clone used by observational failed-attempt replay.
//!
//! `Value::deep_copy` is intentionally a tree copier for ordinary detached
//! payloads. A VM fork needs a stronger contract: object aliases and cycles
//! must survive inside the child, while closure capture cells must never keep
//! pointers into the authoritative VM. This module performs an iterative
//! discover/allocate pass followed by a pointer-rewrite pass.

use crate::gc::{CaptureCell, GcHeap};
use crate::value::{ClosureValue, MapKey, Object, RadList, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_REPLAY_GRAPH_OBJECTS: usize = 1_000_000;
const MAX_REPLAY_GRAPH_BYTES: usize = 256 * 1024 * 1024;

enum FingerprintNode {
    Object(Value),
    Capture(*mut CaptureCell),
}

struct GraphFingerprinter {
    digest: Sha256,
    objects: HashMap<usize, u64>,
    captures: HashMap<usize, u64>,
    pending: Vec<FingerprintNode>,
}

impl GraphFingerprinter {
    fn new() -> Self {
        Self {
            digest: Sha256::new(),
            objects: HashMap::new(),
            captures: HashMap::new(),
            pending: Vec::new(),
        }
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
        let identity = capture as usize;
        let id = if let Some(id) = self.captures.get(&identity) {
            *id
        } else {
            let id = self.captures.len() as u64;
            self.captures.insert(identity, id);
            self.pending.push(FingerprintNode::Capture(capture));
            id
        };
        self.digest.update(b"c");
        self.digest.update(id.to_le_bytes());
    }

    fn value(&mut self, value: Value) {
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
        match value
            .as_object()
            .expect("fingerprinted object remains live")
        {
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
                self.digest.update(b"W");
                self.bytes(snapshot.snapshot_json_like().as_bytes());
                self.digest
                    .update((snapshot.emit_ids.len() as u64).to_le_bytes());
                for emit_id in snapshot.emit_ids.iter() {
                    self.digest.update(emit_id.to_le_bytes());
                }
                self.digest
                    .update(snapshot.rollout_seed.unwrap_or(0).to_le_bytes());
            }
        }
    }

    fn finish(mut self, roots: &[Value]) -> String {
        self.digest.update(b"rad-replay-graph/v1\0");
        self.digest.update((roots.len() as u64).to_le_bytes());
        for root in roots {
            self.value(*root);
        }
        let mut index = 0;
        while index < self.pending.len() {
            match self.pending[index] {
                FingerprintNode::Object(value) => self.object(value),
                FingerprintNode::Capture(capture) => {
                    self.digest.update(b"V");
                    self.value(unsafe { (*capture).get() });
                }
            }
            index += 1;
        }
        hex::encode(self.digest.finalize())
    }
}

/// Canonical content-and-topology identity for replay-visible VM roots.
/// Mutable aliases and closure captures are numbered by deterministic graph
/// discovery, never by allocator address.
pub(crate) fn fingerprint_roots(roots: &[Value]) -> String {
    GraphFingerprinter::new().finish(roots)
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
