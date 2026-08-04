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
// Lexical sections preserve one private semantic namespace.
include!("host_value/frozen_values.rs");
include!("host_value/tests.rs");
