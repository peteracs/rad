//! Sound Rust embedding boundary for RAD values.
//!
//! The VM's NaN-boxed `Value` is a compact internal handle containing GC
//! pointers. It must never outlive or be dereferenced independently of its
//! owning VM. Hosts exchange fully-owned [`FrozenValue`] trees, or borrow a
//! [`ValueHandle`] whose lifetime is tied to one live [`VM`].
//!
//! Raw VM values are intentionally not part of the public API:
//!
//! ```compile_fail
//! use rad_vm::value::Value;
//! let _raw = Value::NIL;
//! ```
//!
//! Handles cannot escape their owner:
//!
//! ```compile_fail
//! use rad_vm::host_value::{FrozenValue, ValueHandle};
//! use rad_vm::vm::VM;
//!
//! fn dangling() -> ValueHandle<'static> {
//!     let mut vm = VM::new();
//!     vm.import_value(&FrozenValue::String("owned by vm".into())).unwrap()
//! }
//! ```

use crate::causal_value::{CausalValueError, CausalValueLimits};
use crate::gc::GcHeap;
use crate::value::{ComponentData, MapKey, MapStorage, Object, Value};
use crate::vm::VM;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FrozenMapKey {
    Int(i64),
    String(String),
    Bool(bool),
    Entity(u32),
    Tuple(Vec<FrozenMapKey>),
}

/// Canonical IEEE-754 payload used at the owned host boundary.
///
/// Finite values retain their exact bits (including signed zero). Every NaN
/// collapses to one quiet positive NaN so equality, hashing, fingerprints,
/// and canonical encoding cannot depend on an attacker-controlled payload.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrozenFloat(u64);

impl FrozenFloat {
    const CANONICAL_NAN_BITS: u64 = 0x7FF8_0000_0000_0000;

    pub fn new(value: f64) -> Self {
        Self(if value.is_nan() {
            Self::CANONICAL_NAN_BITS
        } else {
            value.to_bits()
        })
    }

    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }

    pub fn bits(self) -> u64 {
        self.0
    }
}

impl From<f64> for FrozenFloat {
    fn from(value: f64) -> Self {
        Self::new(value)
    }
}

impl fmt::Debug for FrozenFloat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(formatter)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FrozenValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(FrozenFloat),
    String(String),
    List(Vec<FrozenValue>),
    Tuple(Vec<FrozenValue>),
    Map(BTreeMap<FrozenMapKey, FrozenValue>),
    Component {
        type_name: String,
        fields: BTreeMap<String, FrozenValue>,
    },
    State {
        machine: String,
        state: String,
    },
    Sum {
        type_name: String,
        variant: String,
        fields: BTreeMap<String, FrozenValue>,
    },
    Entity(u32),
    BitSet(Vec<u64>),
    Buffer(String),
    Bytes(Vec<u8>),
    System(String),
}

impl FrozenValue {
    pub fn try_map(
        entries: impl IntoIterator<Item = (FrozenMapKey, FrozenValue)>,
    ) -> Result<Self, CausalValueError> {
        let mut canonical = BTreeMap::new();
        for (key, value) in entries {
            if canonical.insert(key.clone(), value).is_some() {
                return Err(CausalValueError::DuplicateMapKey {
                    key: format!("{key:?}"),
                });
            }
        }
        Ok(Self::Map(canonical))
    }

    pub fn try_component(
        type_name: impl Into<String>,
        fields: impl IntoIterator<Item = (String, FrozenValue)>,
    ) -> Result<Self, CausalValueError> {
        let type_name = type_name.into();
        let fields = collect_named_fields("component", fields)?;
        Ok(Self::Component { type_name, fields })
    }

    pub fn try_sum(
        type_name: impl Into<String>,
        variant: impl Into<String>,
        fields: impl IntoIterator<Item = (String, FrozenValue)>,
    ) -> Result<Self, CausalValueError> {
        let type_name = type_name.into();
        let variant = variant.into();
        let fields = collect_named_fields("sum", fields)?;
        Ok(Self::Sum {
            type_name,
            variant,
            fields,
        })
    }

    pub fn canonical_bytes(&self, limits: &CausalValueLimits) -> Result<Vec<u8>, CausalValueError> {
        limits.validate_profile()?;
        self.validate_structure(limits)?;
        crate::canonical_value::frozen_bytes(self, limits)
    }

    fn import_into(&self, gc: &mut GcHeap) -> Value {
        match self {
            Self::Nil => Value::NIL,
            Self::Bool(value) => Value::from_bool(*value),
            Self::Int(value) => Value::from_int(gc, *value),
            Self::Float(value) => Value::from_float(value.get()),
            Self::String(value) => Value::from_string(gc, value.clone()),
            Self::List(values) => {
                let values = values.iter().map(|value| value.import_into(gc)).collect();
                Value::list(gc, values)
            }
            Self::Tuple(values) => {
                let values = values.iter().map(|value| value.import_into(gc)).collect();
                Value::tuple(gc, values)
            }
            Self::Map(entries) => {
                let entries = entries
                    .iter()
                    .map(|(key, value)| (key.import(), value.import_into(gc)))
                    .collect::<MapStorage>();
                Value::map(gc, entries)
            }
            Self::Component { type_name, fields } => {
                let layout = Arc::new(fields.keys().cloned().collect());
                let values = fields.values().map(|value| value.import_into(gc)).collect();
                Value::component(gc, type_name.clone(), layout, values)
            }
            Self::State { machine, state } => Value::from_state(gc, machine.clone(), state.clone()),
            Self::Sum {
                type_name,
                variant,
                fields,
            } => {
                let fields = fields
                    .iter()
                    .map(|(name, value)| (name.clone(), value.import_into(gc)))
                    .collect::<HashMap<_, _>>();
                Value::sum_type(gc, type_name.clone(), variant.clone(), fields)
            }
            Self::Entity(entity) => Value::from_entity_id(gc, *entity),
            Self::BitSet(words) => Value::bitset(gc, words.clone()),
            Self::Buffer(value) => Value::buffer(gc, value.clone()),
            Self::Bytes(bytes) => Value::bytebuf(gc, bytes.clone()),
            Self::System(name) => Value::system_ref(gc, name.clone()),
        }
    }

    fn validate_structure(&self, limits: &CausalValueLimits) -> Result<(), CausalValueError> {
        struct Budget<'a> {
            limits: &'a CausalValueLimits,
            nodes: usize,
            items: usize,
        }

        impl Budget<'_> {
            fn charge(
                &mut self,
                nodes: usize,
                items: usize,
                _bytes: usize,
            ) -> Result<(), CausalValueError> {
                self.nodes = self.nodes.saturating_add(nodes);
                self.items = self.items.saturating_add(items);
                if self.nodes > self.limits.max_nodes() {
                    return Err(CausalValueError::NodeLimit {
                        limit: self.limits.max_nodes(),
                    });
                }
                if self.items > self.limits.max_collection_items() {
                    return Err(CausalValueError::CollectionItemLimit {
                        limit: self.limits.max_collection_items(),
                    });
                }
                Ok(())
            }

            fn key(&mut self, key: &FrozenMapKey, depth: usize) -> Result<(), CausalValueError> {
                if depth > self.limits.max_depth() {
                    return Err(CausalValueError::DepthLimit {
                        limit: self.limits.max_depth(),
                    });
                }
                self.charge(1, 0, 1)?;
                match key {
                    FrozenMapKey::Int(_) => self.charge(0, 0, 8),
                    FrozenMapKey::String(value) => self.charge(0, 0, value.len()),
                    FrozenMapKey::Bool(_) => self.charge(0, 0, 1),
                    FrozenMapKey::Entity(_) => self.charge(0, 0, 4),
                    FrozenMapKey::Tuple(values) => {
                        self.charge(0, values.len(), 0)?;
                        for value in values {
                            self.key(value, depth + 1)?;
                        }
                        Ok(())
                    }
                }
            }

            fn value(&mut self, value: &FrozenValue, depth: usize) -> Result<(), CausalValueError> {
                if depth > self.limits.max_depth() {
                    return Err(CausalValueError::DepthLimit {
                        limit: self.limits.max_depth(),
                    });
                }
                self.charge(1, 0, 1)?;
                match value {
                    FrozenValue::Nil => Ok(()),
                    FrozenValue::Bool(_) => self.charge(0, 0, 1),
                    FrozenValue::Int(_) | FrozenValue::Float(_) => self.charge(0, 0, 8),
                    FrozenValue::String(value)
                    | FrozenValue::Buffer(value)
                    | FrozenValue::System(value) => self.charge(0, 0, value.len()),
                    FrozenValue::List(values) | FrozenValue::Tuple(values) => {
                        self.charge(0, values.len(), 0)?;
                        for value in values {
                            self.value(value, depth + 1)?;
                        }
                        Ok(())
                    }
                    FrozenValue::Map(entries) => {
                        self.charge(0, entries.len(), 0)?;
                        for (key, value) in entries {
                            self.key(key, depth + 1)?;
                            self.value(value, depth + 1)?;
                        }
                        Ok(())
                    }
                    FrozenValue::Component { type_name, fields } => {
                        self.charge(0, fields.len(), type_name.len())?;
                        for (name, value) in fields {
                            self.charge(0, 0, name.len())?;
                            self.value(value, depth + 1)?;
                        }
                        Ok(())
                    }
                    FrozenValue::State { machine, state } => {
                        self.charge(0, 0, machine.len() + state.len())
                    }
                    FrozenValue::Sum {
                        type_name,
                        variant,
                        fields,
                    } => {
                        self.charge(0, fields.len(), type_name.len() + variant.len())?;
                        for (name, value) in fields {
                            self.charge(0, 0, name.len())?;
                            self.value(value, depth + 1)?;
                        }
                        Ok(())
                    }
                    FrozenValue::Entity(_) => self.charge(0, 0, 4),
                    FrozenValue::BitSet(words) => {
                        self.charge(0, words.len(), words.len().saturating_mul(8))
                    }
                    FrozenValue::Bytes(bytes) => self.charge(0, bytes.len(), bytes.len()),
                }
            }
        }

        Budget {
            limits,
            nodes: 0,
            items: 0,
        }
        .value(self, 0)
    }

    fn validate(&self, limits: &CausalValueLimits) -> Result<(), CausalValueError> {
        self.canonical_bytes(limits).map(|_| ())
    }
}

fn collect_named_fields(
    container: &str,
    fields: impl IntoIterator<Item = (String, FrozenValue)>,
) -> Result<BTreeMap<String, FrozenValue>, CausalValueError> {
    let mut canonical = BTreeMap::new();
    for (name, value) in fields {
        if canonical.insert(name.clone(), value).is_some() {
            return Err(CausalValueError::DuplicateField {
                container: container.to_string(),
                field: name,
            });
        }
    }
    Ok(canonical)
}

impl FrozenMapKey {
    fn import(&self) -> MapKey {
        match self {
            Self::Int(value) => MapKey::Int(*value),
            Self::String(value) => MapKey::Str(value.clone()),
            Self::Bool(value) => MapKey::Bool(*value),
            Self::Entity(value) => MapKey::Entity(*value),
            Self::Tuple(values) => MapKey::Tuple(values.iter().map(FrozenMapKey::import).collect()),
        }
    }

    fn export(value: &MapKey) -> Self {
        match value {
            MapKey::Int(value) => Self::Int(*value),
            MapKey::Str(value) => Self::String(value.clone()),
            MapKey::Bool(value) => Self::Bool(*value),
            MapKey::Entity(value) => Self::Entity(*value),
            MapKey::Tuple(values) => Self::Tuple(values.iter().map(Self::export).collect()),
        }
    }
}

pub(crate) fn export_value(value: &Value) -> Result<FrozenValue, CausalValueError> {
    if value.is_nil() {
        return Ok(FrozenValue::Nil);
    }
    if let Some(value) = value.as_bool() {
        return Ok(FrozenValue::Bool(value));
    }
    if let Some(value) = value.as_int() {
        return Ok(FrozenValue::Int(value));
    }
    if let Some(value) = value.as_float() {
        return Ok(FrozenValue::Float(value.into()));
    }
    match value.as_object() {
        Some(Object::Str(value)) => Ok(FrozenValue::String(value.to_string())),
        Some(Object::List(values)) => values
            .iter()
            .map(export_value)
            .collect::<Result<Vec<_>, _>>()
            .map(FrozenValue::List),
        Some(Object::Tuple(values)) => values
            .iter()
            .map(export_value)
            .collect::<Result<Vec<_>, _>>()
            .map(FrozenValue::Tuple),
        Some(Object::Map(values)) => values
            .iter()
            .map(|(key, value)| Ok((FrozenMapKey::export(key), export_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(FrozenValue::Map),
        Some(Object::Component(component)) => component
            .layout
            .iter()
            .zip(&component.values)
            .map(|(name, value)| Ok((name.clone(), export_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(|fields| FrozenValue::Component {
                type_name: component.type_name.clone(),
                fields,
            }),
        Some(Object::State(value)) => Ok(FrozenValue::State {
            machine: value.machine.clone(),
            state: value.state.clone(),
        }),
        Some(Object::SumType(sum)) => {
            let fields = sum
                .fields
                .iter()
                .map(|(name, value)| Ok((name.clone(), export_value(value)?)))
                .collect::<Result<BTreeMap<_, _>, CausalValueError>>()?;
            Ok(FrozenValue::Sum {
                type_name: sum.type_name.clone(),
                variant: sum.variant.clone(),
                fields,
            })
        }
        Some(Object::EntityId(value)) => Ok(FrozenValue::Entity(*value)),
        Some(Object::BitSet(value)) => Ok(FrozenValue::BitSet(value.clone())),
        Some(Object::Buffer(value)) => Ok(FrozenValue::Buffer(value.clone())),
        Some(Object::ByteBuf(value)) => Ok(FrozenValue::Bytes(value.clone())),
        Some(Object::SystemRef(value)) => Ok(FrozenValue::System(value.clone())),
        Some(other) => Err(CausalValueError::Unsupported {
            type_name: match other {
                Object::Fn(_) => "function",
                Object::Closure(_) => "closure",
                Object::Cell(_) => "capture",
                Object::BuiltinFn(_) => "builtin",
                Object::NativeFn(_) => "native_fn",
                Object::Task(_) => "task",
                Object::MapIter(_, _, _) => "map_iter",
                Object::WorldFork(_) => "world_fork",
                _ => "runtime",
            }
            .to_string(),
        }),
        None => Err(CausalValueError::Unsupported {
            type_name: "invalid".to_string(),
        }),
    }
}

pub(crate) fn export_component_data(
    component: ComponentData,
    limits: &CausalValueLimits,
) -> Result<FrozenValue, CausalValueError> {
    // Validate each raw graph before following any pointer while exporting.
    // Validate the completed owned value as well so the limit is cumulative
    // across every field rather than resetting for each field.
    for value in &component.values {
        value.validate_causal_value(limits)?;
    }
    let fields = component
        .layout
        .iter()
        .zip(&component.values)
        .map(|(name, value)| Ok((name.clone(), export_value(value)?)))
        .collect::<Result<BTreeMap<_, _>, CausalValueError>>()?;
    let frozen = FrozenValue::Component {
        type_name: component.type_name,
        fields,
    };
    frozen.validate(limits)?;
    Ok(frozen)
}

/// Borrowed VM value. The raw GC pointer cannot escape through this API and
/// the borrow prevents collecting or dropping its owner while inspected.
pub struct ValueHandle<'vm> {
    value: Value,
    limits: CausalValueLimits,
    _owner: PhantomData<&'vm VM>,
}

impl<'vm> ValueHandle<'vm> {
    pub fn to_owned(&self) -> Result<FrozenValue, CausalValueError> {
        self.to_owned_with_limits(&self.limits)
    }

    pub fn to_owned_with_limits(
        &self,
        limits: &CausalValueLimits,
    ) -> Result<FrozenValue, CausalValueError> {
        self.value.validate_causal_value(limits)?;
        export_value(&self.value)
    }
}

impl VM {
    pub fn import_value<'vm>(
        &'vm mut self,
        value: &FrozenValue,
    ) -> Result<ValueHandle<'vm>, CausalValueError> {
        let limits = self.causal_value_limits;
        self.import_value_with_limits(value, &limits)
    }

    pub fn import_value_with_limits<'vm>(
        &'vm mut self,
        value: &FrozenValue,
        limits: &CausalValueLimits,
    ) -> Result<ValueHandle<'vm>, CausalValueError> {
        // FrozenValue is pointer-free and acyclic by construction. Validate
        // its deterministic resource contract before allocating anything in
        // the VM heap.
        value.validate(limits)?;
        let raw = value.import_into(&mut self.gc);
        Ok(ValueHandle {
            value: raw,
            limits: *limits,
            _owner: PhantomData,
        })
    }

    pub fn global_value(&self, name: &str) -> Option<ValueHandle<'_>> {
        let slot = self
            .global_names
            .iter()
            .position(|candidate| candidate == name)?;
        self.globals.get(slot).copied().map(|value| ValueHandle {
            value,
            limits: self.causal_value_limits,
            _owner: PhantomData,
        })
    }

    pub fn export_global(&self, name: &str) -> Result<FrozenValue, String> {
        self.global_value(name)
            .ok_or_else(|| format!("unknown global `{name}`"))?
            .to_owned()
            .map_err(|error| error.to_string())
    }

    pub fn call_global(&mut self, name: &str, args: &[FrozenValue]) -> Result<FrozenValue, String> {
        self.call_global_detailed(name, args)
            .map_err(|failure| failure.render_compat())
    }

    /// Call a RAD function while preserving typed settlement rejection,
    /// runtime, and host failures for embedders.
    pub fn call_global_detailed(
        &mut self,
        name: &str,
        args: &[FrozenValue],
    ) -> Result<FrozenValue, crate::constraint_types::VmFailure> {
        let slot = self
            .global_names
            .iter()
            .position(|candidate| candidate == name)
            .ok_or_else(|| crate::constraint_types::RuntimeError {
                code: "runtime.unknown_global".into(),
                message: format!("unknown global `{name}`"),
            })
            .map_err(crate::constraint_types::VmFailure::Runtime)?;
        let callee = self.globals[slot];
        let limits = self.causal_value_limits;
        let mut imported = Vec::with_capacity(args.len());
        for value in args {
            value.validate(&limits).map_err(|error| {
                crate::constraint_types::VmFailure::Runtime(crate::constraint_types::RuntimeError {
                    code: "host.invalid_argument".into(),
                    message: error.to_string(),
                })
            })?;
            imported.push(value.import_into(&mut self.gc));
        }
        let result = self.call_value_detailed(&callee, imported)?;
        result.validate_causal_value(&limits).map_err(|error| {
            crate::constraint_types::VmFailure::Runtime(crate::constraint_types::RuntimeError {
                code: "host.invalid_result".into(),
                message: error.to_string(),
            })
        })?;
        export_value(&result).map_err(|error| {
            crate::constraint_types::VmFailure::Runtime(crate::constraint_types::RuntimeError {
                code: "host.invalid_result".into(),
                message: error.to_string(),
            })
        })
    }

    /// Execute and retain a pointer-free recipe when the settlement rejects.
    /// The record is ephemeral debugger/test data and is never appended to
    /// the authoritative causality ledger.
    pub fn call_global_attempt(
        &mut self,
        name: &str,
        args: &[FrozenValue],
    ) -> Result<crate::constraint_types::SettlementAttemptOutcome, crate::constraint_types::VmFailure>
    {
        use crate::constraint_types::{
            FailedSettlementAttempt, SettlementAttemptOutcome, SETTLEMENT_ATTEMPT_RECORD_VERSION,
        };

        let base_world_digest = self.world.content_digest();
        let program_digest = self.program_digest();
        let runtime_feature_fingerprint = self.runtime_feature_fingerprint();
        let constraint_registry_digest = self.constraint_registry_digest();
        match self.call_global_detailed(name, args) {
            Ok(value) => Ok(SettlementAttemptOutcome::Committed(value)),
            Err(crate::constraint_types::VmFailure::SettlementRejected(rejection)) => Ok(
                SettlementAttemptOutcome::Rejected(Arc::new(FailedSettlementAttempt {
                    version: SETTLEMENT_ATTEMPT_RECORD_VERSION,
                    function: name.to_string(),
                    arguments: args.to_vec(),
                    base_world_digest,
                    program_digest,
                    runtime_feature_fingerprint,
                    constraint_registry_digest,
                    limit_profile_fingerprint: rejection.limit_profile_fingerprint.clone(),
                    capabilities: rejection.capabilities.clone(),
                    rejection,
                })),
            ),
            Err(other) => Err(other),
        }
    }

    /// Re-execute an ephemeral failed attempt against the same base world,
    /// limits, request, and capability context, then compare canonical
    /// semantic rejection bytes.
    pub fn replay_failed_attempt(
        &mut self,
        attempt: &crate::constraint_types::FailedSettlementAttempt,
    ) -> Result<Arc<crate::constraint_types::SettlementRejection>, crate::constraint_types::VmFailure>
    {
        use crate::constraint_types::{HostFault, VmFailure, SETTLEMENT_ATTEMPT_RECORD_VERSION};

        let fail = |code: &str, message: String| {
            VmFailure::Host(HostFault {
                code: code.to_string(),
                message,
            })
        };
        if attempt.version != SETTLEMENT_ATTEMPT_RECORD_VERSION {
            return Err(fail(
                "attempt.unsupported_version",
                format!("unsupported attempt record version {}", attempt.version),
            ));
        }
        let actual_base = self.world.content_digest();
        if actual_base != attempt.base_world_digest {
            return Err(fail(
                "attempt.base_mismatch",
                format!(
                    "attempt expects base {}, current world is {}",
                    attempt.base_world_digest, actual_base
                ),
            ));
        }
        let actual_profile = self.constraint_limit_profile.fingerprint();
        if actual_profile != attempt.limit_profile_fingerprint {
            return Err(fail(
                "attempt.limit_profile_mismatch",
                "constraint limit profile does not match the recorded attempt".into(),
            ));
        }
        if self.program_digest() != attempt.program_digest {
            return Err(fail(
                "attempt.program_mismatch",
                "compiled program does not match the recorded attempt".into(),
            ));
        }
        if self.runtime_feature_fingerprint() != attempt.runtime_feature_fingerprint {
            return Err(fail(
                "attempt.runtime_features_mismatch",
                "runtime feature set does not match the recorded attempt".into(),
            ));
        }
        if self.constraint_registry_digest() != attempt.constraint_registry_digest {
            return Err(fail(
                "attempt.constraint_registry_mismatch",
                "constraint registry does not match the recorded attempt".into(),
            ));
        }
        let actual_capabilities = self.constraint_capabilities();
        if actual_capabilities != attempt.capabilities {
            return Err(fail(
                "attempt.capability_mismatch",
                "capability profile does not match the recorded attempt".into(),
            ));
        }
        // Attempt replay is observational debugging, never an authoritative
        // transition. Execute against a detached child timeline and discard
        // it even if the program unexpectedly commits or faults.
        let mut replay_vm = self
            .detached_attempt_replay_vm()
            .map_err(|message| fail("attempt.replay_fork_failed", message))?;
        let replayed = match replay_vm.call_global_detailed(&attempt.function, &attempt.arguments) {
            Err(VmFailure::SettlementRejected(rejection)) => rejection,
            Ok(_) => {
                return Err(fail(
                    "attempt.unexpected_commit",
                    "replayed attempt committed instead of rejecting".into(),
                ))
            }
            Err(other) => return Err(other),
        };
        let replayed_bytes = replayed
            .canonical_bytes(&replay_vm.constraint_limit_profile)
            .map_err(|error| fail("attempt.rejection_encoding_failed", error.to_string()))?;
        let expected_bytes = attempt
            .rejection
            .canonical_bytes(&self.constraint_limit_profile)
            .map_err(|error| fail("attempt.rejection_encoding_failed", error.to_string()))?;
        if replayed.capabilities != attempt.capabilities || replayed_bytes != expected_bytes {
            return Err(fail(
                "attempt.rejection_mismatch",
                "replayed attempt produced a different canonical rejection".into(),
            ));
        }
        Ok(replayed)
    }

    pub(crate) fn detached_attempt_replay_vm(&self) -> Result<VM, String> {
        let mut replay = VM::from_shared_state(self.shared_state());

        // Clone every mutable heap root as one graph. This preserves sharing
        // and cycles inside the child while replacing every closure capture
        // cell. Constant pools are included because a malformed or legacy
        // pre-settlement path must not mutate a parent-owned constant alias.
        let mut roots = self.globals.clone();
        let chunk_constant_counts = self
            .chunks
            .iter()
            .map(|chunk| {
                roots.extend_from_slice(chunk.constants());
                chunk.constants().len()
            })
            .collect::<Vec<_>>();
        for (_, payload, _) in &self.events_current {
            roots.push(*payload);
        }
        for (_, payload, _) in &self.events_next {
            roots.push(*payload);
        }
        for (_, payload, _) in &self.events_processing {
            roots.push(*payload);
        }
        for (_, _, payload, _) in &self.delayed_events {
            roots.push(*payload);
        }
        for entry in &self.event_log {
            roots.push(entry.payload);
        }
        let mut completed_tasks = self
            .tasks
            .iter()
            .filter_map(|(id, task)| match task.status {
                crate::vm::TaskStatus::Completed(value) => Some((*id, value)),
                crate::vm::TaskStatus::Ready | crate::vm::TaskStatus::Failed(_) => None,
            })
            .collect::<Vec<_>>();
        completed_tasks.sort_by_key(|(id, _)| *id);
        roots.extend(completed_tasks.iter().map(|(_, value)| *value));

        let cloned =
            crate::vm::replay_clone::VmForkCloneContext::new(&mut replay.gc).clone_roots(&roots);
        let mut cloned = cloned.into_iter();
        replay.globals = cloned.by_ref().take(self.globals.len()).collect();
        let mut child_chunks = Vec::with_capacity(self.chunks.len());
        for (chunk, constant_count) in self.chunks.iter().zip(chunk_constant_counts) {
            let constants = cloned.by_ref().take(constant_count).collect::<Vec<_>>();
            child_chunks.push(
                chunk.reseal_with_constants(constants).map_err(|error| {
                    format!("replay constant graph failed verification: {error}")
                })?,
            );
        }
        replay.chunks = Arc::new(child_chunks);

        let mut next_value = || {
            cloned
                .next()
                .ok_or_else(|| "replay graph clone produced too few values".to_string())
        };
        replay.events_current = self
            .events_current
            .iter()
            .map(|(name, _, id)| Ok((name.clone(), next_value()?, *id)))
            .collect::<Result<Vec<_>, String>>()?;
        replay.events_next = self
            .events_next
            .iter()
            .map(|(name, _, id)| Ok((name.clone(), next_value()?, *id)))
            .collect::<Result<Vec<_>, String>>()?;
        replay.events_processing = self
            .events_processing
            .iter()
            .map(|(name, _, id)| Ok((name.clone(), next_value()?, *id)))
            .collect::<Result<Vec<_>, String>>()?;
        replay.delayed_events = self
            .delayed_events
            .iter()
            .map(|(delay, name, _, id)| Ok((*delay, name.clone(), next_value()?, *id)))
            .collect::<Result<Vec<_>, String>>()?;
        replay.event_log = self
            .event_log
            .iter()
            .map(|entry| {
                Ok(crate::vm::EventLogEntry {
                    tick: entry.tick,
                    event_name: entry.event_name.clone(),
                    payload: next_value()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        replay.tasks = self.tasks.clone();
        for (id, _) in completed_tasks {
            let task = replay
                .tasks
                .get_mut(&id)
                .ok_or_else(|| format!("replay task {id} disappeared while cloning"))?;
            task.status = crate::vm::TaskStatus::Completed(next_value()?);
        }
        if cloned.next().is_some() {
            return Err("replay graph clone produced unexpected extra values".into());
        }

        replay.world.restore(self.world.snapshot());
        replay.indexed_decl = self.indexed_decl.clone();
        replay.migrations = self.migrations.clone();
        replay.sandbox_caps = self.sandbox_caps.clone();
        replay.sys_args = self.sys_args.clone();
        replay.fuel = self.fuel;
        replay.mem_limit = self.mem_limit;
        replay.rng_state = self.rng_state;
        replay.current_cause = self.current_cause.clone();
        replay.causality_frame = self.causality_frame;
        replay.ledger = self.ledger.clone();
        replay.emit_ids_current = self.emit_ids_current.clone();
        replay.emit_ids_next = self.emit_ids_next.clone();
        replay.next_task_id = self.next_task_id;
        replay.current_trace_id = self.current_trace_id;
        replay.next_trace_id = self.next_trace_id;
        replay.sandbox_input_json = self.sandbox_input_json.clone();
        replay.sandbox_output_json = self.sandbox_output_json.clone();
        replay.last_sandbox_output_json = self.last_sandbox_output_json.clone();
        replay.last_sandbox_fuel_spent = self.last_sandbox_fuel_spent;
        replay.once_guard_passed = self.once_guard_passed;
        replay.suppress_output = true;
        replay.observational_attempt_replay = true;
        Ok(replay)
    }

    pub fn enqueue_frozen_event(&mut self, payload: &FrozenValue) -> Result<(), String> {
        payload
            .validate(&self.causal_value_limits)
            .map_err(|error| error.to_string())?;
        let payload = payload.import_into(&mut self.gc);
        self.enqueue_event(payload)
    }

    pub fn component_value(
        &self,
        entity: u32,
        component: &str,
    ) -> Result<Option<FrozenValue>, CausalValueError> {
        self.world
            .get_component(entity, component)
            .map(|component| export_component_data(component, &self.causal_value_limits))
            .transpose()
    }

    pub fn resource_value(&self, resource: &str) -> Result<Option<FrozenValue>, CausalValueError> {
        self.world
            .get_resource(resource)
            .map(|component| export_component_data(component, &self.causal_value_limits))
            .transpose()
    }

    pub fn causal_value_limits(&self) -> CausalValueLimits {
        self.causal_value_limits
    }

    pub fn set_causal_value_limits(&mut self, limits: CausalValueLimits) {
        self.causal_value_limits = limits;
        self.constraint_limit_profile
            .synchronize_value_limits(limits);
    }

    pub fn constraint_limit_profile(&self) -> &crate::constraint_types::ConstraintLimitProfile {
        &self.constraint_limit_profile
    }

    pub fn set_constraint_limit_profile(
        &mut self,
        profile: crate::constraint_types::ConstraintLimitProfile,
    ) {
        self.causal_value_limits = profile.value_limits();
        self.constraint_limit_profile = profile;
    }

    fn program_digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"rad-compiled-program/v2\0");
        digest.update(b"bytecode-semantics/1\0compiler-semantics/1\0");
        for chunk in self.chunks.iter() {
            digest.update((chunk.name().len() as u64).to_le_bytes());
            digest.update(chunk.name().as_bytes());
            digest.update((chunk.code().len() as u64).to_le_bytes());
            digest.update(chunk.code());
            digest.update((chunk.constants().len() as u64).to_le_bytes());
            for constant in chunk.constants() {
                if constant.is_nil() {
                    digest.update(b"nil\0");
                } else if let Some(value) = constant.as_bool() {
                    digest.update([b'b', value as u8]);
                } else if let Some(value) = constant.as_int() {
                    digest.update(b"i");
                    digest.update(value.to_le_bytes());
                } else if let Some(value) = constant.as_float() {
                    digest.update(b"f");
                    digest.update(value.to_bits().to_le_bytes());
                } else if let Some(value) = constant.as_str() {
                    digest.update(b"s");
                    digest.update((value.len() as u64).to_le_bytes());
                    digest.update(value.as_bytes());
                } else if let Some(function) = constant.as_fn() {
                    digest.update(b"F");
                    digest.update(function.name.as_bytes());
                    digest.update([function.arity]);
                    digest.update((function.chunk_id as u64).to_le_bytes());
                } else if let Ok(frozen) = export_value(constant) {
                    digest.update(b"v");
                    match frozen.canonical_bytes(&self.causal_value_limits) {
                        Ok(bytes) => {
                            digest.update((bytes.len() as u64).to_le_bytes());
                            digest.update(bytes);
                        }
                        Err(error) => {
                            // Sealed programs with constants outside the
                            // host-value profile remain distinguishable and
                            // fail closed for replay identity.
                            let rendered = error.to_string();
                            digest.update(b"invalid\0");
                            digest.update((rendered.len() as u64).to_le_bytes());
                            digest.update(rendered.as_bytes());
                        }
                    }
                } else {
                    digest.update(b"o");
                    digest.update(constant.type_name().as_bytes());
                    let rendered = constant.print_display();
                    digest.update((rendered.len() as u64).to_le_bytes());
                    digest.update(rendered.as_bytes());
                }
            }
        }
        hex::encode(digest.finalize())
    }

    fn constraint_registry_digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"rad-constraint-registry/v1\0");
        for constraint in self.constraint_registry.iter() {
            digest.update(constraint.name.as_bytes());
            digest.update([0]);
            digest.update(constraint.attached_component.as_bytes());
            digest.update([0]);
            for watch in constraint.watches.iter() {
                digest.update(watch.as_bytes());
                digest.update([0]);
            }
            digest.update(constraint.global_slot.to_le_bytes());
        }
        hex::encode(digest.finalize())
    }

    fn runtime_feature_fingerprint(&self) -> String {
        let descriptor = format!(
            "rad-runtime/v2;crate={};compiler_semantics=1;bytecode_semantics=1;causal_laws=1;candidate_constraints=1;constraint_profile={};target={}",
            env!("CARGO_PKG_VERSION"),
            self.constraint_limit_profile.version(),
            std::env::consts::ARCH,
        );
        hex::encode(Sha256::digest(descriptor.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::{FrozenFloat, FrozenMapKey, FrozenValue};
    use crate::value::{ComponentData, Value};
    use crate::vm::VM;
    use crate::{CausalValueError, CausalValueLimits};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn frozen_values_round_trip_without_raw_heap_handles() {
        let source = FrozenValue::Component {
            type_name: "Payload".into(),
            fields: BTreeMap::from([
                ("name".into(), FrozenValue::String("hero".into())),
                (
                    "data".into(),
                    FrozenValue::Map(BTreeMap::from([(
                        FrozenMapKey::String("hp".into()),
                        FrozenValue::Int(100),
                    )])),
                ),
            ]),
        };
        let mut vm = VM::new();
        let handle = vm.import_value(&source).expect("owned import");
        assert_eq!(handle.to_owned().expect("owned export"), source);
    }

    #[test]
    fn importing_every_nan_pattern_stays_float() {
        for bits in [
            0x7FF0_0000_0000_0001,
            0x7FFC_0000_0000_0000,
            0xFFFC_0000_0000_0000,
            0xFFFF_FFFF_FFFF_FFFF,
        ] {
            let source = FrozenValue::Float(f64::from_bits(bits).into());
            let mut vm = VM::new();
            let exported = vm
                .import_value(&source)
                .expect("NaN import")
                .to_owned()
                .expect("NaN export");
            assert!(matches!(exported, FrozenValue::Float(value) if value.get().is_nan()));
        }
    }

    #[test]
    fn frozen_constructors_reject_duplicates_and_canonicalize_order_and_nan() {
        let duplicate_key = FrozenMapKey::String("same".into());
        assert!(matches!(
            FrozenValue::try_map([
                (duplicate_key.clone(), FrozenValue::Int(1)),
                (duplicate_key, FrozenValue::Int(2)),
            ]),
            Err(CausalValueError::DuplicateMapKey { .. })
        ));
        assert!(matches!(
            FrozenValue::try_component(
                "Pair",
                [
                    ("same".into(), FrozenValue::Int(1)),
                    ("same".into(), FrozenValue::Int(2)),
                ],
            ),
            Err(CausalValueError::DuplicateField { .. })
        ));
        assert!(matches!(
            FrozenValue::try_sum(
                "Choice",
                "One",
                [
                    ("same".into(), FrozenValue::Int(1)),
                    ("same".into(), FrozenValue::Int(2)),
                ],
            ),
            Err(CausalValueError::DuplicateField { .. })
        ));

        let first = FrozenValue::try_map([
            (FrozenMapKey::String("z".into()), FrozenValue::Int(2)),
            (FrozenMapKey::String("a".into()), FrozenValue::Int(1)),
        ])
        .expect("unique map");
        let second = FrozenValue::try_map([
            (FrozenMapKey::String("a".into()), FrozenValue::Int(1)),
            (FrozenMapKey::String("z".into()), FrozenValue::Int(2)),
        ])
        .expect("unique map");
        assert_eq!(first, second);
        assert_eq!(
            FrozenFloat::new(f64::from_bits(0xFFFF_FFFF_FFFF_FFFF)),
            FrozenFloat::new(f64::from_bits(0x7FF0_0000_0000_0001))
        );
    }

    #[test]
    fn encoded_byte_limit_is_exact_canonical_output_length() {
        let value = FrozenValue::try_component(
            "Escaped",
            [(
                "text".into(),
                FrozenValue::String("quote=\" newline=\n snowman=â˜ƒ".into()),
            )],
        )
        .expect("canonical component");
        let unlimited = CausalValueLimits::default();
        let bytes = value.canonical_bytes(&unlimited).expect("canonical bytes");
        let text = String::from_utf8(bytes.clone()).expect("canonical UTF-8");
        assert_eq!(
            text,
            "{\"c\":[\"Escaped\",{\"text\":\"quote=\\\" newline=\\n snowman=â˜ƒ\"}]}"
        );

        let exact = CausalValueLimits::default()
            .with_max_encoded_bytes(bytes.len())
            .expect("exact profile");
        assert_eq!(value.canonical_bytes(&exact).unwrap().len(), bytes.len());
        let short = CausalValueLimits::default()
            .with_max_encoded_bytes(bytes.len() - 1)
            .expect("short profile");
        assert_eq!(
            value.canonical_bytes(&short),
            Err(CausalValueError::EncodedByteLimit {
                limit: bytes.len() - 1,
                actual: bytes.len(),
            })
        );
    }

    #[test]
    fn fuzz_frozen_import_export_never_panics_or_changes_values() {
        fn next(seed: &mut u64) -> u64 {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            *seed
        }

        fn generate(seed: &mut u64, depth: usize) -> FrozenValue {
            if depth == 0 {
                return match next(seed) % 6 {
                    0 => FrozenValue::Nil,
                    1 => FrozenValue::Bool(next(seed) & 1 == 1),
                    2 => FrozenValue::Int(next(seed) as i64),
                    3 => FrozenValue::Float(((next(seed) as i32) as f64 / 7.0).into()),
                    4 => FrozenValue::String(format!("s{:016x}", next(seed))),
                    _ => FrozenValue::Bytes(next(seed).to_le_bytes().to_vec()),
                };
            }
            match next(seed) % 7 {
                0..=2 => generate(seed, 0),
                3 => FrozenValue::List(
                    (0..(next(seed) as usize % 5))
                        .map(|_| generate(seed, depth - 1))
                        .collect(),
                ),
                4 => FrozenValue::Tuple(
                    (0..(next(seed) as usize % 4))
                        .map(|_| generate(seed, depth - 1))
                        .collect(),
                ),
                5 => FrozenValue::Component {
                    type_name: "Generated".into(),
                    fields: (0..(next(seed) as usize % 4))
                        .map(|index| (format!("f{index}"), generate(seed, depth - 1)))
                        .collect(),
                },
                _ => FrozenValue::Map(
                    (0..(next(seed) as usize % 4))
                        .map(|index| {
                            (
                                FrozenMapKey::String(format!("k{index}")),
                                generate(seed, depth - 1),
                            )
                        })
                        .collect(),
                ),
            }
        }

        let iterations = std::env::var("RAD_FUZZ_ITERS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2_000);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut seed = 0xA11C_E5ED_5AFE_CAFE;
            let mut vm = VM::new();
            for _ in 0..iterations {
                let value = generate(&mut seed, 5);
                let round_trip = vm
                    .import_value(&value)
                    .expect("generated value is within default limits")
                    .to_owned()
                    .expect("generated value exports");
                assert_eq!(round_trip, value);
            }
        }));
        assert!(result.is_ok(), "FrozenValue import/export must never panic");
    }

    #[test]
    fn frozen_import_rejects_deep_and_wide_values_before_allocation() {
        let mut deep = FrozenValue::Nil;
        for _ in 0..256 {
            deep = FrozenValue::List(vec![deep]);
        }
        let mut vm = VM::new();
        let limits = CausalValueLimits::default()
            .with_max_depth(32)
            .expect("test profile");
        assert!(matches!(
            vm.import_value_with_limits(&deep, &limits),
            Err(CausalValueError::DepthLimit { limit: 32 })
        ));

        let wide = FrozenValue::List(vec![FrozenValue::Nil; 100_001]);
        assert!(matches!(
            vm.import_value(&wide),
            Err(CausalValueError::CollectionItemLimit { .. })
        ));
    }

    #[test]
    fn component_and_resource_exports_apply_one_cumulative_budget() {
        let mut vm = VM::new();
        let entity = vm.world.spawn_entity(Some("hero"));
        let component = ComponentData {
            type_name: "Pair".into(),
            layout: Arc::new(vec!["left".into(), "right".into()]),
            values: vec![
                Value::from_string(&mut vm.gc, "12345".into()),
                Value::from_string(&mut vm.gc, "67890".into()),
            ],
        };
        assert!(vm.world.set_component(entity, component.clone()));
        vm.world.init_resource("Pair", component);
        vm.set_causal_value_limits(
            CausalValueLimits::default()
                .with_max_encoded_bytes(12)
                .expect("test profile"),
        );

        assert!(matches!(
            vm.component_value(entity, "Pair"),
            Err(CausalValueError::EncodedByteLimit { limit: 12, .. })
        ));
        assert!(matches!(
            vm.resource_value("Pair"),
            Err(CausalValueError::EncodedByteLimit { limit: 12, .. })
        ));
    }
}
