//! Exact project-owned kernels for affine parity dynamics.
//!
//! The kernel studies the parameterized integer map
//!
//! ```text
//! F(n) = n / 2                 when n is even
//!        (multiplier*n+addend)/2 when n is odd.
//! ```
//!
//! On one residue class modulo `2^d`, every prefix is affine:
//!
//! ```text
//! F^j(n) = (coefficient*n + offset) / 2^j.
//! ```
//!
//! A prefix with `coefficient < 2^j` descends for every
//! `n > offset/(2^j-coefficient)`.  Once that threshold lies below an
//! independently verified convergence bound, the whole residue subtree is
//! impossible for a *least* counterexample and can be pruned exactly. The
//! companion valuation-word kernel is parameterized by the same multiplier
//! and addend. This module belongs to the dogfood extension, not the RAD VM.

use std::collections::BTreeMap;

use num_bigint::BigUint;

const MAX_DEPTH: u32 = 50;
const MAX_LANES: u64 = 64;
const MAX_SURVIVOR_SAMPLE: usize = 32;
const MAX_SPARSE_FRONTIER: usize = 8_000_000;
const MAX_SPARSE_ANCHORS: u64 = 350_000_000;
const MAX_SPARSE_DEPTH: u32 = 2048;
const MAX_SPARSE_INPUT_ONES: u32 = 64;
const MAX_CYCLE_ODD_STEPS: u32 = 12;
const MAX_CYCLE_DIVISIONS: u32 = 28;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResidueLaneProfile {
    pub depth: u32,
    pub lane_index: u64,
    pub lane_count: u64,
    pub classes: u64,
    pub residue_sum: u128,
    pub pruned_classes: u64,
    pub survivor_classes: u64,
    pub contracting_survivors: u64,
    pub noncontracting_survivors: u64,
    pub expanded_nodes: u64,
    pub max_odd_steps: u32,
    pub max_odd_residue: u64,
    pub max_threshold: u128,
    pub max_threshold_residue: u64,
    pub prune_depth_histogram: Vec<u64>,
    pub survivor_odd_histogram: Vec<u64>,
    pub survivor_sample: Vec<u64>,
    pub signature: u64,
}

#[derive(Clone, Copy, Debug)]
struct ResidueNode {
    residue: u64,
    coefficient: u128,
    offset: u128,
    denominator: u128,
    probe: u128,
    peak: u128,
    odd_steps: u32,
    input_ones: u32,
}
// Each section owns one affine-analysis responsibility while sharing these
// private arithmetic primitives.
include!("affine_parity/residue_lanes.rs");
include!("affine_parity/sparse_support.rs");
include!("affine_parity/natural_tails.rs");
include!("affine_parity/cycles.rs");
