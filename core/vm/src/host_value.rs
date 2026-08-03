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
use std::collections::HashMap;
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

#[derive(Clone, Debug, PartialEq)]
pub enum FrozenValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<FrozenValue>),
    Tuple(Vec<FrozenValue>),
    Map(Vec<(FrozenMapKey, FrozenValue)>),
    Component {
        type_name: String,
        fields: Vec<(String, FrozenValue)>,
    },
    State {
        machine: String,
        state: String,
    },
    Sum {
        type_name: String,
        variant: String,
        fields: Vec<(String, FrozenValue)>,
    },
    Entity(u32),
    BitSet(Vec<u64>),
    Buffer(String),
    Bytes(Vec<u8>),
    System(String),
}

impl FrozenValue {
    fn import_into(&self, gc: &mut GcHeap) -> Value {
        match self {
            Self::Nil => Value::NIL,
            Self::Bool(value) => Value::from_bool(*value),
            Self::Int(value) => Value::from_int(gc, *value),
            Self::Float(value) => Value::from_float(*value),
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
                let layout = Arc::new(fields.iter().map(|(name, _)| name.clone()).collect());
                let values = fields
                    .iter()
                    .map(|(_, value)| value.import_into(gc))
                    .collect();
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

    fn validate(&self, limits: &CausalValueLimits) -> Result<(), CausalValueError> {
        struct Budget<'a> {
            limits: &'a CausalValueLimits,
            nodes: usize,
            items: usize,
            bytes: usize,
        }

        impl Budget<'_> {
            fn charge(
                &mut self,
                nodes: usize,
                items: usize,
                bytes: usize,
            ) -> Result<(), CausalValueError> {
                self.nodes = self.nodes.saturating_add(nodes);
                self.items = self.items.saturating_add(items);
                self.bytes = self.bytes.saturating_add(bytes);
                if self.nodes > self.limits.max_nodes {
                    return Err(CausalValueError::NodeLimit {
                        limit: self.limits.max_nodes,
                    });
                }
                if self.items > self.limits.max_collection_items {
                    return Err(CausalValueError::CollectionItemLimit {
                        limit: self.limits.max_collection_items,
                    });
                }
                if self.bytes > self.limits.max_encoded_bytes {
                    return Err(CausalValueError::EncodedByteLimit {
                        limit: self.limits.max_encoded_bytes,
                        actual: self.bytes,
                    });
                }
                Ok(())
            }

            fn key(&mut self, key: &FrozenMapKey, depth: usize) -> Result<(), CausalValueError> {
                if depth > self.limits.max_depth {
                    return Err(CausalValueError::DepthLimit {
                        limit: self.limits.max_depth,
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
                if depth > self.limits.max_depth {
                    return Err(CausalValueError::DepthLimit {
                        limit: self.limits.max_depth,
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
            bytes: 0,
        }
        .value(self, 0)
    }
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

fn export_value(value: &Value) -> Result<FrozenValue, CausalValueError> {
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
        return Ok(FrozenValue::Float(value));
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
        Some(Object::Map(values)) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(key, _)| (*key).clone());
            entries
                .into_iter()
                .map(|(key, value)| Ok((FrozenMapKey::export(key), export_value(value)?)))
                .collect::<Result<Vec<_>, _>>()
                .map(FrozenValue::Map)
        }
        Some(Object::Component(component)) => component
            .layout
            .iter()
            .zip(&component.values)
            .map(|(name, value)| Ok((name.clone(), export_value(value)?)))
            .collect::<Result<Vec<_>, _>>()
            .map(|fields| FrozenValue::Component {
                type_name: component.type_name.clone(),
                fields,
            }),
        Some(Object::State(value)) => Ok(FrozenValue::State {
            machine: value.machine.clone(),
            state: value.state.clone(),
        }),
        Some(Object::SumType(sum)) => {
            let mut fields = sum.fields.iter().collect::<Vec<_>>();
            fields.sort_by_key(|(name, _)| (*name).clone());
            let fields = fields
                .into_iter()
                .map(|(name, value)| Ok((name.clone(), export_value(value)?)))
                .collect::<Result<Vec<_>, CausalValueError>>()?;
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

fn export_component_data(
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
        .collect::<Result<Vec<_>, CausalValueError>>()?;
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
        let slot = self
            .global_names
            .iter()
            .position(|candidate| candidate == name)
            .ok_or_else(|| format!("unknown global `{name}`"))?;
        let callee = self.globals[slot];
        let limits = self.causal_value_limits;
        let mut imported = Vec::with_capacity(args.len());
        for value in args {
            value.validate(&limits).map_err(|error| error.to_string())?;
            imported.push(value.import_into(&mut self.gc));
        }
        let result = self.call_value(&callee, imported)?;
        result
            .validate_causal_value(&limits)
            .map_err(|error| error.to_string())?;
        export_value(&result).map_err(|error| error.to_string())
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
    }
}

#[cfg(test)]
mod tests {
    use super::{FrozenMapKey, FrozenValue};
    use crate::value::{ComponentData, Value};
    use crate::vm::VM;
    use crate::{CausalValueError, CausalValueLimits};
    use std::sync::Arc;

    #[test]
    fn frozen_values_round_trip_without_raw_heap_handles() {
        let source = FrozenValue::Component {
            type_name: "Payload".into(),
            fields: vec![
                ("name".into(), FrozenValue::String("hero".into())),
                (
                    "data".into(),
                    FrozenValue::Map(vec![(
                        FrozenMapKey::String("hp".into()),
                        FrozenValue::Int(100),
                    )]),
                ),
            ],
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
            let source = FrozenValue::Float(f64::from_bits(bits));
            let mut vm = VM::new();
            let exported = vm
                .import_value(&source)
                .expect("NaN import")
                .to_owned()
                .expect("NaN export");
            assert!(matches!(exported, FrozenValue::Float(value) if value.is_nan()));
        }
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
                    3 => FrozenValue::Float((next(seed) as i32) as f64 / 7.0),
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
        let limits = CausalValueLimits {
            max_depth: 32,
            ..CausalValueLimits::default()
        };
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
        vm.set_causal_value_limits(CausalValueLimits {
            max_encoded_bytes: 12,
            ..CausalValueLimits::default()
        });

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
