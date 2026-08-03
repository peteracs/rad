//! Exact, bounded canonical encoding for host-owned and internal values.
//!
//! This module is deliberately separate from host import/export. One encoder
//! defines proposal ordering, causal byte limits, replay fingerprints, and the
//! future structured constraint wire boundary.

use crate::causal_value::{CausalValueError, CausalValueLimits};
use crate::host_value::{FrozenFloat, FrozenMapKey, FrozenValue};
use crate::value::{MapKey, Object, Value};
use std::collections::BTreeMap;

struct Encoder {
    bytes: Vec<u8>,
    count: usize,
    limit: usize,
}

impl Encoder {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            count: 0,
            limit,
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        let start = self.count;
        self.count = self.count.saturating_add(bytes.len());
        if start < self.limit {
            let remaining = self.limit - start;
            self.bytes
                .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        }
    }

    fn text(&mut self, text: &str) {
        self.write(text.as_bytes());
    }

    fn byte(&mut self, byte: u8) {
        self.write(&[byte]);
    }

    fn escaped(&mut self, value: &str) {
        self.byte(b'"');
        for character in value.chars() {
            match character {
                '"' => self.text("\\\""),
                '\\' => self.text("\\\\"),
                '\n' => self.text("\\n"),
                '\r' => self.text("\\r"),
                '\t' => self.text("\\t"),
                character if (character as u32) < 0x20 => {
                    self.text(&format!("\\u{:04x}", character as u32));
                }
                character => {
                    let mut encoded = [0_u8; 4];
                    self.write(character.encode_utf8(&mut encoded).as_bytes());
                }
            }
        }
        self.byte(b'"');
    }

    fn float(&mut self, value: FrozenFloat) {
        let value = value.get();
        if value.is_nan() {
            self.text("{\"f\":\"nan\"}");
        } else if value == f64::INFINITY {
            self.text("{\"f\":\"+inf\"}");
        } else if value == f64::NEG_INFINITY {
            self.text("{\"f\":\"-inf\"}");
        } else if value.abs() >= 1e17 {
            self.text(&format!("{value:e}"));
        } else if value == value.trunc() {
            self.text(&format!("{value:.1}"));
        } else {
            self.text(&value.to_string());
        }
    }

    fn frozen_key(&mut self, key: &FrozenMapKey) {
        match key {
            FrozenMapKey::String(value) => {
                self.text("\"s\",");
                self.escaped(value);
            }
            FrozenMapKey::Int(value) => self.text(&format!("\"i\",{value}")),
            FrozenMapKey::Bool(value) => {
                self.text(if *value { "\"b\",true" } else { "\"b\",false" });
            }
            FrozenMapKey::Entity(value) => self.text(&format!("\"e\",{value}")),
            FrozenMapKey::Tuple(values) => {
                self.text("\"t\",[");
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        self.byte(b',');
                    }
                    self.byte(b'[');
                    self.frozen_key(value);
                    self.byte(b']');
                }
                self.byte(b']');
            }
        }
    }

    fn raw_key(&mut self, key: &MapKey) {
        match key {
            MapKey::Str(value) => {
                self.text("\"s\",");
                self.escaped(value);
            }
            MapKey::Int(value) => self.text(&format!("\"i\",{value}")),
            MapKey::Bool(value) => {
                self.text(if *value { "\"b\",true" } else { "\"b\",false" });
            }
            MapKey::Entity(value) => self.text(&format!("\"e\",{value}")),
            MapKey::Tuple(values) => {
                self.text("\"t\",[");
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        self.byte(b',');
                    }
                    self.byte(b'[');
                    self.raw_key(value);
                    self.byte(b']');
                }
                self.byte(b']');
            }
        }
    }

    fn frozen_fields(&mut self, fields: &BTreeMap<String, FrozenValue>) {
        self.byte(b'{');
        for (index, (name, value)) in fields.iter().enumerate() {
            if index > 0 {
                self.byte(b',');
            }
            self.escaped(name);
            self.byte(b':');
            self.frozen(value);
        }
        self.byte(b'}');
    }

    fn frozen(&mut self, value: &FrozenValue) {
        match value {
            FrozenValue::Nil => self.text("null"),
            FrozenValue::Bool(value) => self.text(if *value { "true" } else { "false" }),
            FrozenValue::Int(value) => self.text(&value.to_string()),
            FrozenValue::Float(value) => self.float(*value),
            FrozenValue::String(value) => self.escaped(value),
            FrozenValue::List(values) => {
                self.byte(b'[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        self.byte(b',');
                    }
                    self.frozen(value);
                }
                self.byte(b']');
            }
            FrozenValue::Tuple(values) => {
                self.text("{\"t\":[");
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        self.byte(b',');
                    }
                    self.frozen(value);
                }
                self.text("]}");
            }
            FrozenValue::Map(entries) => {
                self.text("{\"m\":[");
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        self.byte(b',');
                    }
                    self.text("[[");
                    self.frozen_key(key);
                    self.text("],");
                    self.frozen(value);
                    self.byte(b']');
                }
                self.text("]}");
            }
            FrozenValue::Component { type_name, fields } => {
                self.text("{\"c\":[");
                self.escaped(type_name);
                self.byte(b',');
                self.frozen_fields(fields);
                self.text("]}");
            }
            FrozenValue::State { machine, state } => {
                self.text("{\"state\":[");
                self.escaped(machine);
                self.byte(b',');
                self.escaped(state);
                self.text("]}");
            }
            FrozenValue::Sum {
                type_name,
                variant,
                fields,
            } => {
                self.text("{\"s\":[");
                self.escaped(type_name);
                self.byte(b',');
                self.escaped(variant);
                self.byte(b',');
                self.frozen_fields(fields);
                self.text("]}");
            }
            FrozenValue::Entity(value) => self.text(&format!("{{\"e\":{value}}}")),
            FrozenValue::BitSet(words) => {
                self.text("{\"bits\":[");
                for (index, word) in words.iter().enumerate() {
                    if index > 0 {
                        self.byte(b',');
                    }
                    self.text(&word.to_string());
                }
                self.text("]}");
            }
            FrozenValue::Buffer(value) => {
                self.text("{\"buffer\":");
                self.escaped(value);
                self.byte(b'}');
            }
            FrozenValue::Bytes(bytes) => {
                self.text("{\"bytes\":\"");
                for byte in bytes {
                    self.text(&format!("{byte:02x}"));
                }
                self.text("\"}");
            }
            FrozenValue::System(value) => {
                self.text("{\"system\":");
                self.escaped(value);
                self.byte(b'}');
            }
        }
    }

    fn raw_fields<'a>(
        &mut self,
        fields: impl IntoIterator<Item = (&'a String, &'a Value)>,
    ) -> Result<(), CausalValueError> {
        let mut fields = fields.into_iter().collect::<Vec<_>>();
        fields.sort_by_key(|(left, _)| *left);
        self.byte(b'{');
        for (index, (name, value)) in fields.into_iter().enumerate() {
            if index > 0 {
                self.byte(b',');
            }
            self.escaped(name);
            self.byte(b':');
            self.raw(value)?;
        }
        self.byte(b'}');
        Ok(())
    }

    fn raw_component(
        &mut self,
        component: &crate::value::ComponentData,
    ) -> Result<(), CausalValueError> {
        self.text("{\"c\":[");
        self.escaped(&component.type_name);
        self.byte(b',');
        self.raw_fields(component.layout.iter().zip(component.values.iter()))?;
        self.text("]}");
        Ok(())
    }

    fn raw(&mut self, value: &Value) -> Result<(), CausalValueError> {
        if value.is_nil() {
            self.text("null");
        } else if let Some(value) = value.as_bool() {
            self.text(if value { "true" } else { "false" });
        } else if let Some(value) = value.as_int() {
            self.text(&value.to_string());
        } else if let Some(value) = value.as_float() {
            self.float(value.into());
        } else {
            match value.as_object() {
                Some(Object::Str(value)) => self.escaped(value),
                Some(Object::List(values)) => {
                    self.byte(b'[');
                    for (index, value) in values.iter().enumerate() {
                        if index > 0 {
                            self.byte(b',');
                        }
                        self.raw(value)?;
                    }
                    self.byte(b']');
                }
                Some(Object::Tuple(values)) => {
                    self.text("{\"t\":[");
                    for (index, value) in values.iter().enumerate() {
                        if index > 0 {
                            self.byte(b',');
                        }
                        self.raw(value)?;
                    }
                    self.text("]}");
                }
                Some(Object::Map(values)) => {
                    self.text("{\"m\":[");
                    let mut entries = values.iter().collect::<Vec<_>>();
                    entries.sort_by_key(|(left, _)| *left);
                    for (index, (key, value)) in entries.into_iter().enumerate() {
                        if index > 0 {
                            self.byte(b',');
                        }
                        self.text("[[");
                        self.raw_key(key);
                        self.text("],");
                        self.raw(value)?;
                        self.byte(b']');
                    }
                    self.text("]}");
                }
                Some(Object::Component(component)) => {
                    self.raw_component(component)?;
                }
                Some(Object::State(value)) => {
                    self.text("{\"state\":[");
                    self.escaped(&value.machine);
                    self.byte(b',');
                    self.escaped(&value.state);
                    self.text("]}");
                }
                Some(Object::SumType(sum)) => {
                    self.text("{\"s\":[");
                    self.escaped(&sum.type_name);
                    self.byte(b',');
                    self.escaped(&sum.variant);
                    self.byte(b',');
                    self.raw_fields(sum.fields.iter())?;
                    self.text("]}");
                }
                Some(Object::EntityId(value)) => self.text(&format!("{{\"e\":{value}}}")),
                Some(Object::BitSet(words)) => {
                    self.text("{\"bits\":[");
                    for (index, word) in words.iter().enumerate() {
                        if index > 0 {
                            self.byte(b',');
                        }
                        self.text(&word.to_string());
                    }
                    self.text("]}");
                }
                Some(Object::Buffer(value)) => {
                    self.text("{\"buffer\":");
                    self.escaped(value);
                    self.byte(b'}');
                }
                Some(Object::ByteBuf(bytes)) => {
                    self.text("{\"bytes\":\"");
                    for byte in bytes {
                        self.text(&format!("{byte:02x}"));
                    }
                    self.text("\"}");
                }
                Some(Object::SystemRef(value)) => {
                    self.text("{\"system\":");
                    self.escaped(value);
                    self.byte(b'}');
                }
                Some(other) => return Err(unsupported_object(other)),
                None => {
                    return Err(CausalValueError::Unsupported {
                        type_name: "invalid".into(),
                    });
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<Vec<u8>, CausalValueError> {
        if self.count > self.limit {
            Err(CausalValueError::EncodedByteLimit {
                limit: self.limit,
                actual: self.count,
            })
        } else {
            Ok(self.bytes)
        }
    }
}

fn unsupported_object(object: &Object) -> CausalValueError {
    CausalValueError::Unsupported {
        type_name: match object {
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
    }
}

pub(crate) fn frozen_bytes(
    value: &FrozenValue,
    limits: &CausalValueLimits,
) -> Result<Vec<u8>, CausalValueError> {
    let mut encoder = Encoder::new(limits.max_encoded_bytes());
    encoder.frozen(value);
    encoder.finish()
}

pub(crate) fn internal_bytes(
    value: &Value,
    limits: &CausalValueLimits,
) -> Result<Vec<u8>, CausalValueError> {
    let mut encoder = Encoder::new(limits.max_encoded_bytes());
    encoder.raw(value)?;
    encoder.finish()
}

pub(crate) fn component_bytes(
    component: &crate::value::ComponentData,
    limits: &CausalValueLimits,
) -> Result<Vec<u8>, CausalValueError> {
    let mut encoder = Encoder::new(limits.max_encoded_bytes());
    encoder.raw_component(component)?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_encoding_counts_escaped_utf8_bytes() {
        let value = FrozenValue::String("line\nĻ".into());
        let limits = CausalValueLimits::default();
        let bytes = frozen_bytes(&value, &limits).expect("encode");
        assert_eq!(bytes, b"\"line\\n\xC4\xBB\"");
        let too_small = limits
            .with_max_encoded_bytes(bytes.len() - 1)
            .expect("valid limit");
        assert!(matches!(
            frozen_bytes(&value, &too_small),
            Err(CausalValueError::EncodedByteLimit { actual, .. }) if actual == bytes.len()
        ));
    }
}
