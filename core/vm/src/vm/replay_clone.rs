//! Graph-preserving heap clone used by observational failed-attempt replay.
//!
//! `Value::deep_copy` is intentionally a tree copier for ordinary detached
//! payloads. A VM fork needs a stronger contract: object aliases and cycles
//! must survive inside the child, while closure capture cells must never keep
//! pointers into the authoritative VM. This module performs an iterative
//! discover/allocate pass followed by a pointer-rewrite pass.

use crate::gc::{CaptureCell, GcHeap};
use crate::value::{ClosureValue, MapKey, Object, RadList, Value};
use crate::world::{OperationalWorldEncoder, WorldSnapshot};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_REPLAY_GRAPH_OBJECTS: usize = 1_000_000;
const MAX_REPLAY_GRAPH_BYTES: usize = 256 * 1024 * 1024;
const MAX_REPLAY_FINGERPRINT_WORLDS: usize = 100_000;
const MAX_REPLAY_FINGERPRINT_EDGES: usize = 4_000_000;

// Lexical sections preserve one private semantic namespace.
include!("replay_clone/fingerprint.rs");
include!("replay_clone/clone.rs");
include!("replay_clone/tests.rs");
