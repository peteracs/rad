//! Immutable, bounded value capture for Causal Laws transactions.
//!
//! Ordinary RAD values form a heap graph and may contain cycles created by
//! non-causal in-place operations.  A proposal or candidate must instead be
//! detached data with deterministic resource use.  This module validates the
//! entire graph before copying it into transaction-owned storage.

use crate::value::{Allocator, MapKey, Object, Value};
use std::collections::HashSet;
use std::fmt;

/// Deterministic structural budget for values crossing a causal boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CausalValueLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_encoded_bytes: usize,
    pub max_collection_items: usize,
}

impl Default for CausalValueLimits {
    fn default() -> Self {
        Self {
            max_depth: 128,
            max_nodes: 100_000,
            max_encoded_bytes: 8 * 1024 * 1024,
            max_collection_items: 100_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CausalValueError {
    Cycle,
    DepthLimit { limit: usize },
    NodeLimit { limit: usize },
    CollectionItemLimit { limit: usize },
    EncodedByteLimit { limit: usize, actual: usize },
    Unsupported { type_name: String },
}

impl fmt::Display for CausalValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cycle => write!(formatter, "causal value graph contains a cycle"),
            Self::DepthLimit { limit } => write!(
                formatter,
                "causal value graph exceeds maximum depth of {limit}"
            ),
            Self::NodeLimit { limit } => write!(
                formatter,
                "causal value graph exceeds maximum node count of {limit}"
            ),
            Self::CollectionItemLimit { limit } => write!(
                formatter,
                "causal value graph exceeds maximum collection item count of {limit}"
            ),
            Self::EncodedByteLimit { limit, actual } => write!(
                formatter,
                "causal value encoding is {actual} bytes, exceeding the {limit}-byte limit"
            ),
            Self::Unsupported { type_name } => write!(
                formatter,
                "causal proposals and candidates cannot capture {type_name} values"
            ),
        }
    }
}

impl std::error::Error for CausalValueError {}

struct GraphValidator<'a> {
    limits: &'a CausalValueLimits,
    active_path: HashSet<usize>,
    nodes: usize,
    collection_items: usize,
    encoded_bytes: usize,
}

impl<'a> GraphValidator<'a> {
    fn new(limits: &'a CausalValueLimits) -> Self {
        Self {
            limits,
            active_path: HashSet::new(),
            nodes: 0,
            collection_items: 0,
            encoded_bytes: 0,
        }
    }

    fn charge_node(&mut self) -> Result<(), CausalValueError> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > self.limits.max_nodes {
            return Err(CausalValueError::NodeLimit {
                limit: self.limits.max_nodes,
            });
        }
        Ok(())
    }

    fn charge_items(&mut self, count: usize) -> Result<(), CausalValueError> {
        self.collection_items = self.collection_items.saturating_add(count);
        if self.collection_items > self.limits.max_collection_items {
            return Err(CausalValueError::CollectionItemLimit {
                limit: self.limits.max_collection_items,
            });
        }
        Ok(())
    }

    fn charge_bytes(&mut self, count: usize) -> Result<(), CausalValueError> {
        self.encoded_bytes = self.encoded_bytes.saturating_add(count);
        if self.encoded_bytes > self.limits.max_encoded_bytes {
            return Err(CausalValueError::EncodedByteLimit {
                limit: self.limits.max_encoded_bytes,
                actual: self.encoded_bytes,
            });
        }
        Ok(())
    }

    fn visit_map_key(&mut self, key: &MapKey, depth: usize) -> Result<(), CausalValueError> {
        if depth > self.limits.max_depth {
            return Err(CausalValueError::DepthLimit {
                limit: self.limits.max_depth,
            });
        }
        self.charge_node()?;
        self.charge_bytes(1)?;
        match key {
            MapKey::Int(_) => self.charge_bytes(8)?,
            MapKey::Str(value) => self.charge_bytes(value.len())?,
            MapKey::Bool(_) => self.charge_bytes(1)?,
            MapKey::Entity(_) => self.charge_bytes(4)?,
            MapKey::Tuple(items) => {
                self.charge_items(items.len())?;
                for item in items {
                    self.visit_map_key(item, depth + 1)?;
                }
            }
        }
        Ok(())
    }

    fn visit(&mut self, value: &Value, depth: usize) -> Result<(), CausalValueError> {
        if depth > self.limits.max_depth {
            return Err(CausalValueError::DepthLimit {
                limit: self.limits.max_depth,
            });
        }
        self.charge_node()?;
        // One stable type marker per value. Aggregate payloads add their data
        // below; this is a deterministic structural encoding budget rather
        // than an allocator-size estimate.
        self.charge_bytes(1)?;

        let Some(object) = value.as_object() else {
            self.charge_bytes(8)?;
            return Ok(());
        };
        let identity = value
            .object_identity()
            .expect("an object-tagged value must have an identity");
        if !self.active_path.insert(identity) {
            return Err(CausalValueError::Cycle);
        }

        let result = match object {
            Object::List(values) => {
                self.charge_items(values.len())?;
                for item in values.iter() {
                    self.visit(item, depth + 1)?;
                }
                Ok(())
            }
            Object::Tuple(values) => {
                self.charge_items(values.len())?;
                for item in values {
                    self.visit(item, depth + 1)?;
                }
                Ok(())
            }
            Object::Map(values) => {
                self.charge_items(values.len())?;
                let mut entries = values.iter().collect::<Vec<_>>();
                entries.sort_by_key(|(key, _)| (*key).clone());
                for (key, item) in entries {
                    self.visit_map_key(key, depth + 1)?;
                    self.visit(item, depth + 1)?;
                }
                Ok(())
            }
            Object::Component(component) => {
                self.charge_bytes(component.type_name.len())?;
                for field in component.layout.iter() {
                    self.charge_bytes(field.len())?;
                }
                self.charge_items(component.values.len())?;
                for item in &component.values {
                    self.visit(item, depth + 1)?;
                }
                Ok(())
            }
            Object::SumType(sum) => {
                self.charge_bytes(sum.type_name.len() + sum.variant.len())?;
                self.charge_items(sum.fields.len())?;
                let mut fields = sum.fields.iter().collect::<Vec<_>>();
                fields.sort_by_key(|(name, _)| (*name).clone());
                for (name, item) in fields {
                    self.charge_bytes(name.len())?;
                    self.visit(item, depth + 1)?;
                }
                Ok(())
            }
            Object::Fn(_)
            | Object::Closure(_)
            | Object::Cell(_)
            | Object::BuiltinFn(_)
            | Object::NativeFn(_)
            | Object::Task(_)
            | Object::MapIter(_, _, _)
            | Object::WorldFork(_) => Err(CausalValueError::Unsupported {
                type_name: value.type_name(),
            }),
            Object::Str(value) => self.charge_bytes(value.len()),
            Object::BigInt(_) => self.charge_bytes(8),
            Object::State(value) => self.charge_bytes(value.machine.len() + value.state.len()),
            Object::EntityId(_) => self.charge_bytes(4),
            Object::BitSet(words) => self.charge_bytes(words.len().saturating_mul(8)),
            Object::Buffer(value) => self.charge_bytes(value.len()),
            Object::ByteBuf(bytes) => self.charge_bytes(bytes.len()),
            Object::SystemRef(value) => self.charge_bytes(value.len()),
        };

        self.active_path.remove(&identity);
        result
    }
}

impl Value {
    pub(crate) fn validate_causal_value(
        &self,
        limits: &CausalValueLimits,
    ) -> Result<(), CausalValueError> {
        GraphValidator::new(limits).visit(self, 0)
    }

    /// Validate and detach a value graph crossing a causal capture boundary.
    ///
    /// Repeated DAG edges are deliberately expanded and charged as a tree.
    /// This makes the budget independent of allocator addresses and matches
    /// the tree-shaped canonical wire representation. Cycles are rejected.
    pub(crate) fn freeze_causal(
        &self,
        target: &mut dyn Allocator,
        limits: &CausalValueLimits,
    ) -> Result<Self, CausalValueError> {
        self.validate_causal_value(limits)?;

        Ok(self.deep_copy(target))
    }
}

#[cfg(test)]
mod tests {
    use super::{CausalValueError, CausalValueLimits};
    use crate::gc::GcHeap;
    use crate::value::{MapKey, MapStorage, Object, Value};
    use std::collections::HashMap;

    fn freeze(value: Value, source_gc: &mut GcHeap) -> Result<(), CausalValueError> {
        let mut target_gc = GcHeap::new();
        let result = value
            .freeze_causal(&mut target_gc, &CausalValueLimits::default())
            .map(|_| ());
        // Keep the source heap borrowed through validation; its object graph
        // must remain alive for the whole operation.
        let _ = source_gc.object_count();
        result
    }

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
            .freeze_causal(&mut target_gc, &CausalValueLimits::default())
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
        let error = freeze(iterator, &mut source_gc)
            .expect_err("iterator cursor is mutable execution state");
        assert!(matches!(error, CausalValueError::Unsupported { .. }));
        assert!(error.to_string().contains("map_iter"), "{error}");
    }

    #[test]
    fn causal_freeze_rejects_self_and_two_object_cycles() {
        let mut gc = GcHeap::new();
        let mut self_cycle = Value::list(&mut gc, Vec::new());
        let alias = self_cycle;
        let Some(Object::List(list)) = self_cycle.as_object_mut() else {
            panic!("list expected");
        };
        list.push(alias);
        assert_eq!(freeze(self_cycle, &mut gc), Err(CausalValueError::Cycle));

        let mut left = Value::list(&mut gc, Vec::new());
        let mut right = Value::list(&mut gc, Vec::new());
        let left_alias = left;
        let right_alias = right;
        let Some(Object::List(items)) = left.as_object_mut() else {
            panic!("list expected");
        };
        items.push(right_alias);
        let Some(Object::List(items)) = right.as_object_mut() else {
            panic!("list expected");
        };
        items.push(left_alias);
        assert_eq!(freeze(left, &mut gc), Err(CausalValueError::Cycle));
    }

    #[test]
    fn causal_freeze_finds_cycles_through_map_component_and_sum() {
        let mut gc = GcHeap::new();

        let mut map = Value::map(&mut gc, MapStorage::new());
        let map_alias = map;
        let Some(Object::Map(entries)) = map.as_object_mut() else {
            panic!("map expected");
        };
        entries.insert(MapKey::Str("self".into()), map_alias);
        assert_eq!(freeze(map, &mut gc), Err(CausalValueError::Cycle));

        let mut component = Value::component(
            &mut gc,
            "Node".into(),
            std::sync::Arc::new(vec!["next".into()]),
            vec![Value::NIL],
        );
        let component_alias = component;
        let Some(Object::Component(data)) = component.as_object_mut() else {
            panic!("component expected");
        };
        data.values[0] = component_alias;
        assert_eq!(freeze(component, &mut gc), Err(CausalValueError::Cycle));

        let mut sum = Value::sum_type(
            &mut gc,
            "Tree".into(),
            "Branch".into(),
            HashMap::from([("child".into(), Value::NIL)]),
        );
        let sum_alias = sum;
        let Some(Object::SumType(data)) = sum.as_object_mut() else {
            panic!("sum expected");
        };
        data.fields.insert("child".into(), sum_alias);
        assert_eq!(freeze(sum, &mut gc), Err(CausalValueError::Cycle));
    }

    #[test]
    fn causal_freeze_enforces_depth_width_nodes_and_encoded_bytes() {
        let mut gc = GcHeap::new();
        let mut deep = Value::NIL;
        for _ in 0..8 {
            deep = Value::list(&mut gc, vec![deep]);
        }
        let mut target = GcHeap::new();
        let limits = CausalValueLimits {
            max_depth: 4,
            ..CausalValueLimits::default()
        };
        assert!(matches!(
            deep.freeze_causal(&mut target, &limits),
            Err(CausalValueError::DepthLimit { .. })
        ));

        let wide = Value::list(&mut gc, vec![Value::NIL; 8]);
        let limits = CausalValueLimits {
            max_collection_items: 4,
            ..CausalValueLimits::default()
        };
        assert!(matches!(
            wide.freeze_causal(&mut target, &limits),
            Err(CausalValueError::CollectionItemLimit { .. })
        ));

        let limits = CausalValueLimits {
            max_nodes: 4,
            ..CausalValueLimits::default()
        };
        assert!(matches!(
            wide.freeze_causal(&mut target, &limits),
            Err(CausalValueError::NodeLimit { .. })
        ));

        let bytes = Value::buffer(&mut gc, "0123456789".into());
        let limits = CausalValueLimits {
            max_encoded_bytes: 4,
            ..CausalValueLimits::default()
        };
        assert!(matches!(
            bytes.freeze_causal(&mut target, &limits),
            Err(CausalValueError::EncodedByteLimit { .. })
        ));
    }

    #[test]
    fn shared_dags_are_charged_by_tree_expansion() {
        let mut gc = GcHeap::new();
        let leaf = Value::list(&mut gc, vec![Value::NIL]);
        let dag = Value::list(&mut gc, vec![leaf, leaf]);
        let mut target = GcHeap::new();
        let limits = CausalValueLimits {
            // root + two leaf visits + two nil visits = five nodes
            max_nodes: 4,
            ..CausalValueLimits::default()
        };
        assert_eq!(
            dag.freeze_causal(&mut target, &limits),
            Err(CausalValueError::NodeLimit { limit: 4 })
        );
    }
}
