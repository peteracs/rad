//! RFC-0001 runtime kernel.
//!
//! This module owns the whole causal transaction: proposal capture,
//! canonical grouping, isolated resolver patches, conflict validation, and
//! copy-on-write atomic adoption. The bytecode dispatcher only delegates to
//! these operations.

use super::*;
use crate::value::{ComponentData, Value};
use crate::world::{World, WorldSnapshot};
use sha2::Digest;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

// Lexical sections preserve one private semantic namespace.
include!("settlement/model.rs");
include!("settlement/candidate.rs");
include!("settlement/fact_reads.rs");
include!("settlement/commit.rs");
#[cfg(test)]
include!("settlement/relation_tests.rs");
#[cfg(test)]
include!("settlement/fact_read_tests.rs");
