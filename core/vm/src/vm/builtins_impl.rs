use super::*;

use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;

use crate::gc::GcHeap;
use crate::value::{Builtin, MapKey, MapStorage, Value};
// Lexical sections preserve one private semantic namespace.
include!("builtins_impl/dispatch_and_effects.rs");
include!("builtins_impl/higher_order_and_components.rs");
include!("builtins_impl/forks_and_simulation.rs");
include!("builtins_impl/sandbox_and_rollouts.rs");
include!("builtins_impl/fork_wire_and_delta.rs");
include!("builtins_impl/fork_delta_application.rs");
include!("builtins_impl/relation_transport.rs");
include!("builtins_impl/world_persistence.rs");
include!("builtins_impl/world_loading_and_sandbox.rs");
include!("builtins_impl/scalars_and_sequences.rs");
include!("builtins_impl/sequence_collections.rs");
include!("builtins_impl/map_collections.rs");
include!("builtins_impl/text.rs");
include!("builtins_impl/formatting.rs");
include!("builtins_impl/bitsets.rs");
include!("builtins_impl/property_testing.rs");
include!("builtins_impl/network_transport_helpers.rs");
include!("builtins_impl/higher_order_io_and_network.rs");
include!("builtins_impl/network_and_system.rs");
