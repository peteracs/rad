//! Immutable value capture for Causal Laws transactions.
//!
//! This module is deliberately separate from the general-purpose `Value`
//! implementation: ordinary RAD execution permits functions, capture cells,
//! iterators, tasks, and forks, while proposals and candidate patches must be
//! detached data with no surviving interior-mutation channel.

use crate::value::{Allocator, Object, Value};

impl Value {
    /// Validate and detach a value graph crossing a causal capture boundary.
    ///
    /// Proposals and staged candidates must be durable data, never handles to
    /// mutable execution machinery. In particular, copying a closure would
    /// retain its capture-cell pointers and copying a map iterator would retain
    /// an interior cursor. Reject those categories before recursively cloning
    /// all accepted data into transaction-owned storage.
    pub(crate) fn freeze_causal(&self, target: &mut dyn Allocator) -> Result<Self, String> {
        self.validate_causal_data()?;
        Ok(self.deep_copy(target))
    }

    fn validate_causal_data(&self) -> Result<(), String> {
        let Some(object) = self.as_object() else {
            return Ok(());
        };
        match object {
            Object::List(values) => {
                for value in values.iter() {
                    value.validate_causal_data()?;
                }
            }
            Object::Tuple(values) => {
                for value in values.iter() {
                    value.validate_causal_data()?;
                }
            }
            Object::Map(values) => {
                for value in values.values() {
                    value.validate_causal_data()?;
                }
            }
            Object::Component(component) => {
                for value in &component.values {
                    value.validate_causal_data()?;
                }
            }
            Object::SumType(value) => {
                for field in value.fields.values() {
                    field.validate_causal_data()?;
                }
            }
            Object::Fn(_)
            | Object::Closure(_)
            | Object::Cell(_)
            | Object::BuiltinFn(_)
            | Object::NativeFn(_)
            | Object::Task(_)
            | Object::MapIter(_, _, _)
            | Object::WorldFork(_) => {
                return Err(format!(
                    "causal proposals and candidates cannot capture {} values",
                    self.type_name()
                ));
            }
            Object::Str(_)
            | Object::BigInt(_)
            | Object::State(_)
            | Object::EntityId(_)
            | Object::BitSet(_)
            | Object::Buffer(_)
            | Object::ByteBuf(_)
            | Object::SystemRef(_) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::gc::GcHeap;
    use crate::value::{MapStorage, Object, Value};

    #[test]
    fn causal_freeze_detaches_nested_mutable_values() {
        let mut source_gc = GcHeap::new();
        let mut source_buffer = Value::buffer(&mut source_gc, "before".to_string());
        let source = Value::component(
            &mut source_gc,
            "Payload".to_string(),
            std::sync::Arc::new(vec!["data".to_string()]),
            vec![source_buffer],
        );
        let mut target_gc = GcHeap::new();
        let frozen = source
            .freeze_causal(&mut target_gc)
            .expect("data-only graph should freeze");

        let Some(Object::Buffer(value)) = source_buffer.as_object_mut() else {
            panic!("test source should be a buffer");
        };
        value.push_str("-mutated");

        let frozen_buffer = frozen
            .as_component()
            .and_then(|component| component.values.first())
            .and_then(Value::as_buffer)
            .map(String::as_str);
        assert_eq!(frozen_buffer, Some("before"));
    }

    #[test]
    fn causal_freeze_rejects_interior_and_execution_handles() {
        let mut source_gc = GcHeap::new();
        let iterator = Value::map_iter(&mut source_gc, MapStorage::new(), Vec::new());
        let mut target_gc = GcHeap::new();
        let error = iterator
            .freeze_causal(&mut target_gc)
            .expect_err("iterator cursor is mutable execution state");
        assert!(error.contains("map_iter"), "{error}");
    }
}
