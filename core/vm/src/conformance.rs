//! Stable embedding contracts for differential and conformance testing.

use crate::causality::{Cause, WriteKind};
use crate::host_value::{FrozenMapKey, FrozenValue};
use crate::vm::VM;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransitionEvent {
    pub tick: u64,
    pub name: String,
    pub payload: Json,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionWrite {
    pub frame: u64,
    pub entity: Option<u32>,
    pub entity_name: Option<String>,
    pub component: String,
    pub value: String,
    pub fields: BTreeMap<String, String>,
    pub kind: String,
    pub cause: Json,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransitionTrace {
    pub format: String,
    pub transition: String,
    pub before_digest: String,
    pub after_digest: String,
    pub changed_components: BTreeMap<String, usize>,
    pub events: Vec<TransitionEvent>,
    pub writes: Vec<TransitionWrite>,
    pub result: Json,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceComparison {
    pub equal: bool,
    pub differences: Vec<String>,
}

impl TransitionTrace {
    pub fn compare(&self, expected: &TransitionTrace) -> TraceComparison {
        let mut differences = Vec::new();
        compare_field(&mut differences, "format", &self.format, &expected.format);
        compare_field(
            &mut differences,
            "transition",
            &self.transition,
            &expected.transition,
        );
        compare_field(
            &mut differences,
            "before_digest",
            &self.before_digest,
            &expected.before_digest,
        );
        compare_field(
            &mut differences,
            "after_digest",
            &self.after_digest,
            &expected.after_digest,
        );
        compare_field(
            &mut differences,
            "changed_components",
            &self.changed_components,
            &expected.changed_components,
        );
        compare_field(&mut differences, "events", &self.events, &expected.events);
        compare_field(&mut differences, "writes", &self.writes, &expected.writes);
        compare_field(&mut differences, "result", &self.result, &expected.result);
        TraceComparison {
            equal: differences.is_empty(),
            differences,
        }
    }
}

fn compare_field<T: PartialEq + std::fmt::Debug>(
    differences: &mut Vec<String>,
    name: &str,
    actual: &T,
    expected: &T,
) {
    if actual != expected {
        differences.push(format!(
            "{name} mismatch: expected {expected:?}, actual {actual:?}"
        ));
    }
}

impl VM {
    /// Execute one public RAD transition and return an exact, portable trace
    /// for comparison with a Rust, TypeScript, native, or browser client.
    pub fn execute_transition(
        &mut self,
        name: &str,
        args: &[FrozenValue],
    ) -> Result<TransitionTrace, String> {
        let before = self.world_snapshot();
        let before_digest = self.world_digest();
        let event_start = self.event_log.len();
        let write_start = self.ledger.write_watermark();
        let result = self.call_global(name, args)?;
        let after = self.world_snapshot();
        let after_digest = self.world_digest();

        let changed_components = crate::world::WorldSnapshot::diff_summary(&before, &after)
            .into_iter()
            .collect();
        let events = self
            .event_log
            .iter()
            .skip(event_start)
            .map(|event| {
                Ok(TransitionEvent {
                    tick: event.tick,
                    name: event.event_name.clone(),
                    payload: crate::vm::value_to_json(&event.payload, 0)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let writes = self
            .ledger
            .writes_since(write_start)?
            .map(|write| TransitionWrite {
                frame: write.frame,
                entity: write.entity,
                entity_name: write.entity_name.as_ref().map(ToString::to_string),
                component: write.component.to_string(),
                value: write.value_string(),
                fields: write
                    .fields()
                    .iter()
                    .map(|(field, value)| (field.to_string(), value.to_string()))
                    .collect(),
                kind: write_kind_name(write.kind).to_string(),
                cause: cause_json(&write.by),
            })
            .collect();

        Ok(TransitionTrace {
            format: "rad_transition_trace_v1".to_string(),
            transition: name.to_string(),
            before_digest,
            after_digest,
            changed_components,
            events,
            writes,
            result: frozen_json(&result),
        })
    }
}

fn write_kind_name(kind: WriteKind) -> &'static str {
    match kind {
        WriteKind::Set => "set",
        WriteKind::Spawn => "spawn",
        WriteKind::Despawn => "despawn",
        WriteKind::Remove => "remove",
        WriteKind::Resource => "resource",
    }
}

fn cause_json(cause: &Cause) -> Json {
    match cause {
        Cause::Main => json!({ "kind": "main" }),
        Cause::System { name } => json!({ "kind": "system", "name": name.as_str() }),
        Cause::Handler { event, emit_id } => {
            json!({ "kind": "handler", "event": event.as_str(), "emit_id": emit_id })
        }
        Cause::Transaction { name, parent } => json!({
            "kind": "transaction",
            "name": name.as_str(),
            "parent": cause_json(parent),
        }),
        Cause::HostCall {
            extension,
            generation,
            plugin_digest,
            export,
            input_digest,
            output_digest,
            parent,
        } => json!({
            "kind": "host_call",
            "extension": extension.as_str(),
            "generation": generation.as_str(),
            "plugin_digest": plugin_digest.as_str(),
            "export": export.as_str(),
            "input_digest": input_digest.as_str(),
            "output_digest": output_digest.as_str(),
            "parent": cause_json(parent),
        }),
    }
}

fn frozen_key_json(key: &FrozenMapKey) -> Json {
    match key {
        FrozenMapKey::Int(value) => json!(["int", value]),
        FrozenMapKey::Native(value) => json!([
            "native",
            value.type_name,
            value.repr.to_string(),
            value.flavor.as_str(),
            value.bits
        ]),
        FrozenMapKey::String(value) => json!(["string", value]),
        FrozenMapKey::Bool(value) => json!(["bool", value]),
        FrozenMapKey::Entity(value) => json!(["entity", value]),
        FrozenMapKey::Tuple(values) => {
            json!([
                "tuple",
                values.iter().map(frozen_key_json).collect::<Vec<_>>()
            ])
        }
    }
}

fn frozen_key_from_json(value: &Json) -> Result<FrozenMapKey, String> {
    let values = value
        .as_array()
        .ok_or_else(|| "$map keys must be tagged arrays".to_string())?;
    let tag = values
        .first()
        .and_then(Json::as_str)
        .ok_or_else(|| "$map key tags must be strings".to_string())?;
    let payload = || {
        values
            .get(1)
            .ok_or_else(|| format!("$map {tag} key requires a value"))
    };
    Ok(match tag {
        "int" => FrozenMapKey::Int(
            payload()?
                .as_i64()
                .ok_or_else(|| "$map integer key must be i64".to_string())?,
        ),
        "native" => {
            if values.len() != 5 {
                return Err("$map native key must have five entries".to_string());
            }
            FrozenMapKey::Native(crate::native_types::NativeScalarValue {
                type_name: values[1]
                    .as_str()
                    .ok_or_else(|| "$map native key type must be a string".to_string())?
                    .to_string(),
                repr: values[2]
                    .as_str()
                    .and_then(crate::native_types::NativeScalarKind::parse)
                    .ok_or_else(|| "$map native key repr is invalid".to_string())?,
                flavor: values[3]
                    .as_str()
                    .and_then(crate::native_types::NativeTypeFlavor::parse)
                    .ok_or_else(|| "$map native key flavor is invalid".to_string())?,
                bits: values[4]
                    .as_u64()
                    .ok_or_else(|| "$map native key bits must be u64".to_string())?,
            })
        }
        "string" => FrozenMapKey::String(
            payload()?
                .as_str()
                .ok_or_else(|| "$map string key must be a string".to_string())?
                .to_string(),
        ),
        "bool" => FrozenMapKey::Bool(
            payload()?
                .as_bool()
                .ok_or_else(|| "$map boolean key must be a boolean".to_string())?,
        ),
        "entity" => FrozenMapKey::Entity(
            payload()?
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| "$map entity key must be a u32".to_string())?,
        ),
        "tuple" => FrozenMapKey::Tuple(
            payload()?
                .as_array()
                .ok_or_else(|| "$map tuple key must be an array".to_string())?
                .iter()
                .map(frozen_key_from_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        _ => return Err(format!("unknown $map key tag `{tag}`")),
    })
}

pub fn frozen_json(value: &FrozenValue) -> Json {
    match value {
        FrozenValue::Nil => Json::Null,
        FrozenValue::Bool(value) => json!(value),
        FrozenValue::Int(value) => json!(value),
        FrozenValue::Native(value) => json!({
            "$native": value.type_name,
            "repr": value.repr.to_string(),
            "flavor": value.flavor.as_str(),
            "bits": value.bits,
        }),
        FrozenValue::Float(value) => json!(value.get()),
        FrozenValue::String(value) => json!(value),
        FrozenValue::List(values) => Json::Array(values.iter().map(frozen_json).collect()),
        FrozenValue::Tuple(values) => {
            json!({ "$tuple": values.iter().map(frozen_json).collect::<Vec<_>>() })
        }
        FrozenValue::Map(values) => json!({
            "$map": values
                .iter()
                .map(|(key, value)| json!([frozen_key_json(key), frozen_json(value)]))
                .collect::<Vec<_>>()
        }),
        FrozenValue::Component { type_name, fields } => json!({
            "$component": type_name,
            "fields": fields
                .iter()
                .map(|(name, value)| (name.clone(), frozen_json(value)))
                .collect::<serde_json::Map<_, _>>()
        }),
        FrozenValue::State { machine, state } => {
            json!({ "$state": machine, "value": state })
        }
        FrozenValue::Sum {
            type_name,
            variant,
            fields,
        } => json!({
            "$sum": type_name,
            "variant": variant,
            "fields": fields
                .iter()
                .map(|(name, value)| (name.clone(), frozen_json(value)))
                .collect::<serde_json::Map<_, _>>()
        }),
        FrozenValue::Entity(value) => json!({ "$entity": value }),
        FrozenValue::BitSet(value) => json!({ "$bitset": value }),
        FrozenValue::Buffer(value) => json!({ "$buffer": value }),
        FrozenValue::Bytes(value) => json!({ "$bytes": value }),
        FrozenValue::System(value) => json!({ "$system": value }),
        FrozenValue::HostHandle(value) => json!({
            "$host_handle": value.type_name(),
            "owner": value.owner_digest(),
            "token": value.token_for_runtime(),
        }),
    }
}

pub fn frozen_from_json(value: &Json) -> Result<FrozenValue, String> {
    match value {
        Json::Null => Ok(FrozenValue::Nil),
        Json::Bool(value) => Ok(FrozenValue::Bool(*value)),
        Json::Number(value) => value
            .as_i64()
            .map(FrozenValue::Int)
            .or_else(|| value.as_f64().map(|value| FrozenValue::Float(value.into())))
            .ok_or_else(|| "JSON number is outside RAD's numeric domain".to_string()),
        Json::String(value) => Ok(FrozenValue::String(value.clone())),
        Json::Array(values) => values
            .iter()
            .map(frozen_from_json)
            .collect::<Result<Vec<_>, _>>()
            .map(FrozenValue::List),
        Json::Object(object) => {
            if let Some(entity) = object.get("$entity") {
                let entity = entity
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| "$entity must be a u32".to_string())?;
                return Ok(FrozenValue::Entity(entity));
            }
            if let Some(values) = object.get("$tuple") {
                return values
                    .as_array()
                    .ok_or_else(|| "$tuple must be an array".to_string())?
                    .iter()
                    .map(frozen_from_json)
                    .collect::<Result<Vec<_>, _>>()
                    .map(FrozenValue::Tuple);
            }
            if let Some(entries) = object.get("$map") {
                let entries = entries
                    .as_array()
                    .ok_or_else(|| "$map must be an array".to_string())?;
                let mut values = BTreeMap::new();
                for entry in entries {
                    let entry = entry
                        .as_array()
                        .filter(|entry| entry.len() == 2)
                        .ok_or_else(|| "$map entries must be [key, value] pairs".to_string())?;
                    let key = frozen_key_from_json(&entry[0])?;
                    let value = frozen_from_json(&entry[1])?;
                    if values.insert(key, value).is_some() {
                        return Err("$map contains a duplicate canonical key".to_string());
                    }
                }
                return Ok(FrozenValue::Map(values));
            }
            if let Some(type_name) = object.get("$component").and_then(Json::as_str) {
                return Ok(FrozenValue::Component {
                    type_name: type_name.to_string(),
                    fields: decode_fields(object.get("fields"), "$component")?,
                });
            }
            if let Some(machine) = object.get("$state").and_then(Json::as_str) {
                let state = object
                    .get("value")
                    .and_then(Json::as_str)
                    .ok_or_else(|| "$state requires a string value".to_string())?;
                return Ok(FrozenValue::State {
                    machine: machine.to_string(),
                    state: state.to_string(),
                });
            }
            if let Some(type_name) = object.get("$sum").and_then(Json::as_str) {
                let variant = object
                    .get("variant")
                    .and_then(Json::as_str)
                    .ok_or_else(|| "$sum requires a string variant".to_string())?;
                return Ok(FrozenValue::Sum {
                    type_name: type_name.to_string(),
                    variant: variant.to_string(),
                    fields: decode_fields(object.get("fields"), "$sum")?,
                });
            }
            if let Some(native) = object.get("$native").and_then(Json::as_str) {
                let repr = object
                    .get("repr")
                    .and_then(Json::as_str)
                    .and_then(crate::native_types::NativeScalarKind::parse)
                    .ok_or_else(|| "$native has an invalid repr".to_string())?;
                let flavor = object
                    .get("flavor")
                    .and_then(Json::as_str)
                    .and_then(crate::native_types::NativeTypeFlavor::parse)
                    .ok_or_else(|| "$native has an invalid flavor".to_string())?;
                let bits = object
                    .get("bits")
                    .and_then(Json::as_u64)
                    .ok_or_else(|| "$native requires u64 bits".to_string())?;
                return Ok(FrozenValue::Native(
                    crate::native_types::NativeScalarValue {
                        type_name: native.to_string(),
                        repr,
                        flavor,
                        bits,
                    },
                ));
            }
            if let Some(words) = object.get("$bitset") {
                let words = words
                    .as_array()
                    .ok_or_else(|| "$bitset must be an array".to_string())?
                    .iter()
                    .map(|word| {
                        word.as_u64()
                            .ok_or_else(|| "$bitset words must be u64".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(FrozenValue::BitSet(words));
            }
            if let Some(buffer) = object.get("$buffer").and_then(Json::as_str) {
                return Ok(FrozenValue::Buffer(buffer.to_string()));
            }
            if let Some(bytes) = object.get("$bytes") {
                let bytes = bytes
                    .as_array()
                    .ok_or_else(|| "$bytes must be an array".to_string())?
                    .iter()
                    .map(|byte| {
                        byte.as_u64()
                            .and_then(|value| u8::try_from(value).ok())
                            .ok_or_else(|| "$bytes entries must be u8".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(FrozenValue::Bytes(bytes));
            }
            if let Some(system) = object.get("$system").and_then(Json::as_str) {
                return Ok(FrozenValue::System(system.to_string()));
            }
            if object.contains_key("$host_handle") {
                return Err("host handles cannot be forged from JSON".to_string());
            }
            Err(
                "object values require a RAD type tag ($component, $sum, $state, or $map)"
                    .to_string(),
            )
        }
    }
}

fn decode_fields(
    value: Option<&Json>,
    container: &str,
) -> Result<BTreeMap<String, FrozenValue>, String> {
    value
        .and_then(Json::as_object)
        .ok_or_else(|| format!("{container} requires an object `fields` member"))?
        .iter()
        .map(|(name, value)| Ok((name.clone(), frozen_from_json(value)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_value::FrozenFloat;
    use crate::native_types::{NativeScalarKind, NativeScalarValue, NativeTypeFlavor};

    fn native_id(bits: u64) -> NativeScalarValue {
        NativeScalarValue {
            type_name: "ManagerGid".to_string(),
            repr: NativeScalarKind::U32,
            flavor: NativeTypeFlavor::Opaque,
            bits,
        }
    }

    #[test]
    fn json_round_trip_preserves_every_portable_frozen_value_shape() {
        let map = FrozenValue::try_map([
            (FrozenMapKey::Native(native_id(41)), FrozenValue::Entity(7)),
            (
                FrozenMapKey::Tuple(vec![
                    FrozenMapKey::String("key".to_string()),
                    FrozenMapKey::Bool(true),
                ]),
                FrozenValue::Bytes(vec![0, 127, 255]),
            ),
        ])
        .unwrap();
        let value = FrozenValue::List(vec![
            FrozenValue::Nil,
            FrozenValue::Bool(true),
            FrozenValue::Int(-9),
            FrozenValue::Native(native_id(42)),
            FrozenValue::Float(FrozenFloat::new(-0.0)),
            FrozenValue::String("rad".to_string()),
            FrozenValue::Tuple(vec![
                FrozenValue::Entity(3),
                FrozenValue::System("Tick".to_string()),
            ]),
            map,
            FrozenValue::try_component(
                "Identity",
                [("manager".to_string(), FrozenValue::Native(native_id(43)))],
            )
            .unwrap(),
            FrozenValue::State {
                machine: "Lifecycle".to_string(),
                state: "Live".to_string(),
            },
            FrozenValue::try_sum(
                "Option",
                "Some",
                [("value".to_string(), FrozenValue::Int(1))],
            )
            .unwrap(),
            FrozenValue::BitSet(vec![1, u64::MAX]),
            FrozenValue::Buffer("host-buffer".to_string()),
        ]);

        let encoded = frozen_json(&value);
        assert_eq!(frozen_from_json(&encoded).unwrap(), value);
    }

    #[test]
    fn host_handle_json_is_observable_but_cannot_forge_a_token() {
        let handle = FrozenValue::HostHandle(crate::ffi::HostHandleToken::from_recorded(
            "extension-digest".to_string(),
            "Socket".to_string(),
            99,
        ));
        let encoded = frozen_json(&handle);
        assert_eq!(encoded["$host_handle"], "Socket");
        let error = frozen_from_json(&encoded).unwrap_err();
        assert!(error.contains("cannot be forged"), "{error}");
    }

    #[test]
    fn transition_comparison_checks_the_complete_portable_contract() {
        let base = TransitionTrace {
            format: "rad_transition_trace_v1".to_string(),
            transition: "Retire".to_string(),
            before_digest: "before".to_string(),
            after_digest: "after".to_string(),
            changed_components: BTreeMap::from([("Live".to_string(), 1)]),
            events: Vec::new(),
            writes: Vec::new(),
            result: Json::Null,
        };
        assert!(base.compare(&base).equal);

        let mut mismatched = base.clone();
        mismatched.format = "rad_transition_trace_v2".to_string();
        mismatched.after_digest = "different".to_string();
        let comparison = base.compare(&mismatched);
        assert!(!comparison.equal);
        assert_eq!(comparison.differences.len(), 2, "{comparison:?}");
        assert!(comparison.differences[0].starts_with("format mismatch"));
    }
}
