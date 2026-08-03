//! Graph-preserving heap clone used by observational failed-attempt replay.
//!
//! `Value::deep_copy` is intentionally a tree copier for ordinary detached
//! payloads. A VM fork needs a stronger contract: object aliases and cycles
//! must survive inside the child, while closure capture cells must never keep
//! pointers into the authoritative VM. This module performs an iterative
//! discover/allocate pass followed by a pointer-rewrite pass.

use crate::gc::{CaptureCell, GcHeap};
use crate::value::{ClosureValue, Object, RadList, Value};
use std::collections::HashMap;
use std::sync::Arc;

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
    pub(crate) fn clone_roots(mut self, roots: &[Value]) -> Vec<Value> {
        self.discover(roots);
        self.populate_objects();
        self.populate_captures();
        roots.iter().map(|value| self.rewrite(*value)).collect()
    }

    fn discover(&mut self, roots: &[Value]) {
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

    fn populate_objects(&mut self) {
        for (source, mut target) in self.object_sources.clone() {
            let object = match source.as_object().expect("discovered object remains live") {
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
            *target
                .as_object_mut()
                .expect("fork placeholder belongs to child heap") = object;
        }
    }

    fn populate_captures(&self) {
        for (source, target) in &self.capture_sources {
            let value = unsafe { (**source).get() };
            unsafe { (**target).set(self.rewrite(value)) };
        }
    }
}
