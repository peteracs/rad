//! Exact integer consumer for the finite six-gap cover used by the
//! `zeta-simple-zeros` dogfood.
//!
//! Arb remains responsible for producing directed transcendental lower
//! bounds.  This module independently checks their pinned metadata and then
//! replays every ordinary interval, pressure, tangent-certificate, and split
//! decision with integer arithmetic.  Initial boxes are partitioned into
//! independent lanes so RAD can execute them in speculative worlds.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

const FORMAT: &str = "rad-zeta-seven-v1";
const UPSTREAM_COMMIT: &str = "040c5e899e658aed7b56a2a87f501798fe10761d";
const KERNEL_SHA256: &str = "a9992300d2bf71665aa2b6bd2727e798624cd297103bb200c7f0ca2baea55a2c";
const SECOND_SHA256: &str = "7913c5511a572c32dd573cd53123d8cf3ddf73d3ec63b1aa823faae2ae83570a";
const KERNEL_SCALED_SHA256: &str =
    "28ac77aebac30c55fb94e2fd0cac6ee9c9eb30fea2e7bd1cf44e2afc72e40b6e";
const GRID: i64 = 4_000;
const PRECISION_BITS: i64 = 128;
const PRESSURE_DENOMINATOR: i64 = 3_000;
const SCALE: i64 = 100_000_000_000_000;
const ORIGINAL_TARGET_SCALED: i64 = 380_000_000_000;
const CHECKSUM_MODULUS: i64 = 2_147_483_629;
const CHECKSUM_BASE: i64 = 1_000_003;

const COEFFICIENT_NUMERATORS: [i64; 7] = [0, 1, 2, 1, 2, 1, 2];
const COEFFICIENT_DENOMINATORS: [i64; 7] = [1, 3, 5, 2, 3, 1, 1];

type Bounds = [i64; 12];

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CoverStats {
    pub initial_boxes: i64,
    pub nodes: i64,
    pub pruned: i64,
    pub splits: i64,
    pub maximum_depth: i64,
    pub pressure_pruned: i64,
    pub interval_pruned: i64,
    pub tangent_pruned: i64,
}

impl CoverStats {
    fn empty() -> Self {
        Self {
            initial_boxes: 0,
            nodes: 0,
            pruned: 0,
            splits: 0,
            maximum_depth: 0,
            pressure_pruned: 0,
            interval_pruned: 0,
            tangent_pruned: 0,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Certificate {
    format: String,
    upstream_commit: String,
    kernel_sha256: String,
    second_derivative_sha256: String,
    grid: i64,
    precision_bits: i64,
    pressure_cutoff_cells: i64,
    scale: i64,
    target_scaled: i64,
    kernel_scaled: Vec<i64>,
    kernel_checksum: i64,
    gap_components: Vec<[i64; 2]>,
    tangent_boxes: Vec<Vec<i64>>,
    tangent_checksum: i64,
    stats: CoverStats,
}

#[derive(Clone, Debug)]
pub(super) struct SparseMinimum {
    levels: Vec<Vec<i64>>,
    logs: Vec<usize>,
}

impl SparseMinimum {
    pub(super) fn new(values: &[i64]) -> Result<Self, String> {
        if values.is_empty() {
            return Err("kernel table is empty".to_string());
        }
        let mut logs = vec![0usize; values.len() + 1];
        for width in 2..=values.len() {
            logs[width] = logs[width / 2] + 1;
        }
        let mut levels = vec![values.to_vec()];
        let mut span = 2usize;
        while span <= values.len() {
            let half = span / 2;
            let previous = levels.last().expect("level zero exists");
            let mut level = Vec::with_capacity(values.len() - span + 1);
            for left in 0..=values.len() - span {
                level.push(previous[left].min(previous[left + half]));
            }
            levels.push(level);
            span *= 2;
        }
        Ok(Self { levels, logs })
    }

    pub(super) fn query(&self, left: i64, right: i64) -> Result<i64, String> {
        if left < 0 || right < left {
            return Err(format!("invalid kernel range [{left},{right}]"));
        }
        let left = usize::try_from(left).map_err(|_| "negative range left".to_string())?;
        let right = usize::try_from(right).map_err(|_| "negative range right".to_string())?;
        if right >= self.levels[0].len() {
            return Ok(0);
        }
        let width = right - left + 1;
        let level = self.logs[width];
        let span = 1usize << level;
        Ok(self.levels[level][left].min(self.levels[level][right + 1 - span]))
    }
}

#[derive(Debug)]
struct PreparedCertificate {
    digest: String,
    ranges: SparseMinimum,
    components: Vec<[i64; 2]>,
    tangents: HashMap<Bounds, i64>,
    pressure_cutoff_cells: i64,
    target_scaled: i64,
    expected: CoverStats,
}

#[derive(Clone, Debug)]
pub struct LaneReport {
    pub verified: bool,
    pub digest: String,
    pub lane_index: i64,
    pub lane_count: i64,
    pub stats: CoverStats,
    pub minimum_leaf_lower_scaled: i64,
    pub minimum_leaf_kind: String,
    pub minimum_leaf_box: Vec<i64>,
    pub signature: i64,
}

#[derive(Clone, Copy, Debug)]
struct BoxNode {
    bounds: Bounds,
    depth: i64,
}

type PreparedCertificateCache = Mutex<Option<(blake3::Hash, Arc<PreparedCertificate>)>>;

static CACHE: OnceLock<PreparedCertificateCache> = OnceLock::new();

fn checksum<'a>(values: impl IntoIterator<Item = &'a i64>) -> i64 {
    let mut result = 0i64;
    for value in values {
        result = (result * CHECKSUM_BASE + value.rem_euclid(CHECKSUM_MODULUS)) % CHECKSUM_MODULUS;
    }
    result
}

fn scaled_table_sha256(values: &[i64]) -> String {
    let mut digest = Sha256::new();
    for value in values {
        digest.update(value.to_be_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn derive_gap_components(
    table: &[i64],
    pressure_cutoff_cells: i64,
    target_scaled: i64,
) -> Result<Vec<[i64; 2]>, String> {
    let mut components: Vec<[i64; 2]> = Vec::new();
    for index in 0..pressure_cutoff_cells {
        let pressure = scaled_ratio_floor(index, SCALE, GRID * PRESSURE_DENOMINATOR)?;
        let kernel = usize::try_from(index)
            .ok()
            .and_then(|position| table.get(position))
            .copied()
            .unwrap_or(0);
        let one_body = pressure + kernel / 3;
        if one_body >= target_scaled {
            continue;
        }
        match components.last_mut() {
            Some(component) if component[1] + 1 == index => component[1] = index,
            _ => components.push([index, index]),
        }
    }
    Ok(components)
}

fn prepare(encoded: &str) -> Result<Arc<PreparedCertificate>, String> {
    let digest = blake3::hash(encoded.as_bytes());
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .map_err(|_| "zeta certificate cache mutex was poisoned".to_string())?;
    if let Some((cached_digest, prepared)) = guard.as_ref() {
        if *cached_digest == digest {
            return Ok(Arc::clone(prepared));
        }
    }

    let certificate: Certificate = serde_json::from_str(encoded)
        .map_err(|error| format!("invalid zeta cover certificate JSON: {error}"))?;
    if certificate.format != FORMAT
        || certificate.upstream_commit != UPSTREAM_COMMIT
        || certificate.kernel_sha256 != KERNEL_SHA256
        || certificate.second_derivative_sha256 != SECOND_SHA256
    {
        return Err("zeta certificate identity or pinned table digest mismatch".to_string());
    }
    if certificate.grid != GRID
        || certificate.precision_bits != PRECISION_BITS
        || certificate.scale != SCALE
    {
        return Err("zeta certificate arithmetic parameters mismatch".to_string());
    }
    if certificate.target_scaled < ORIGINAL_TARGET_SCALED {
        return Err("zeta certificate target is weaker than 19/5000".to_string());
    }
    let expected_pressure_cutoff = i64::try_from(
        (i128::from(certificate.target_scaled) * i128::from(GRID * PRESSURE_DENOMINATOR)
            + i128::from(SCALE)
            - 1)
            / i128::from(SCALE),
    )
    .map_err(|_| "zeta pressure cutoff overflow".to_string())?;
    if certificate.pressure_cutoff_cells != expected_pressure_cutoff {
        return Err("zeta certificate pressure cutoff does not match its target".to_string());
    }
    if checksum(certificate.kernel_scaled.iter()) != certificate.kernel_checksum {
        return Err("zeta kernel integer checksum mismatch".to_string());
    }
    if scaled_table_sha256(&certificate.kernel_scaled) != KERNEL_SCALED_SHA256 {
        return Err("zeta scaled-kernel SHA-256 mismatch".to_string());
    }
    if certificate.gap_components.is_empty() {
        return Err("zeta gap-component partition is empty".to_string());
    }
    for (index, component) in certificate.gap_components.iter().enumerate() {
        if component[0] < 0 || component[1] < component[0] {
            return Err(format!("invalid gap component {index}"));
        }
        if index > 0 && component[0] <= certificate.gap_components[index - 1][1] + 1 {
            return Err("gap components are not disjoint maximal intervals".to_string());
        }
    }
    let derived_components = derive_gap_components(
        &certificate.kernel_scaled,
        certificate.pressure_cutoff_cells,
        certificate.target_scaled,
    )?;
    if certificate.gap_components != derived_components {
        return Err("zeta gap components do not cover the exact one-body survivor set".to_string());
    }

    let tangent_checksum = checksum(certificate.tangent_boxes.iter().flat_map(|row| row.iter()));
    if tangent_checksum != certificate.tangent_checksum {
        return Err("zeta tangent integer checksum mismatch".to_string());
    }
    let mut tangents = HashMap::with_capacity(certificate.tangent_boxes.len());
    for (index, row) in certificate.tangent_boxes.iter().enumerate() {
        if row.len() != 13 {
            return Err(format!(
                "tangent row {index} has length {}, expected 13",
                row.len()
            ));
        }
        let mut bounds = [0i64; 12];
        bounds.copy_from_slice(&row[..12]);
        for coordinate in 0..6 {
            if bounds[2 * coordinate] < 0 || bounds[2 * coordinate + 1] < bounds[2 * coordinate] {
                return Err(format!("tangent row {index} has invalid bounds"));
            }
        }
        if row[12] < certificate.target_scaled {
            return Err(format!("tangent row {index} does not prove the target"));
        }
        if tangents.insert(bounds, row[12]).is_some() {
            return Err(format!("duplicate tangent box at row {index}"));
        }
    }

    let component_count = i64::try_from(certificate.gap_components.len())
        .map_err(|_| "too many gap components".to_string())?;
    let initial_boxes = component_count
        .checked_pow(6)
        .ok_or_else(|| "initial box count overflow".to_string())?;
    if initial_boxes != certificate.stats.initial_boxes {
        return Err("initial box product disagrees with certificate stats".to_string());
    }
    if certificate.stats.nodes != certificate.stats.pruned + certificate.stats.splits
        || certificate.stats.pruned
            != certificate.stats.pressure_pruned
                + certificate.stats.interval_pruned
                + certificate.stats.tangent_pruned
        || certificate.stats.tangent_pruned
            != i64::try_from(tangents.len()).map_err(|_| "too many tangents".to_string())?
    {
        return Err("certificate search counters are internally inconsistent".to_string());
    }

    let prepared = Arc::new(PreparedCertificate {
        digest: digest.to_hex().to_string(),
        ranges: SparseMinimum::new(&certificate.kernel_scaled)?,
        components: certificate.gap_components,
        tangents,
        pressure_cutoff_cells: certificate.pressure_cutoff_cells,
        target_scaled: certificate.target_scaled,
        expected: certificate.stats,
    });
    *guard = Some((digest, Arc::clone(&prepared)));
    Ok(prepared)
}

pub(super) fn initial_box(mut index: i64, components: &[[i64; 2]]) -> Result<Bounds, String> {
    let radix =
        i64::try_from(components.len()).map_err(|_| "component count overflow".to_string())?;
    let mut digits = [0usize; 6];
    for coordinate in (0..6).rev() {
        digits[coordinate] =
            usize::try_from(index % radix).map_err(|_| "negative initial box index".to_string())?;
        index /= radix;
    }
    let mut bounds = [0i64; 12];
    for coordinate in 0..6 {
        bounds[2 * coordinate] = components[digits[coordinate]][0];
        bounds[2 * coordinate + 1] = components[digits[coordinate]][1];
    }
    Ok(bounds)
}

fn prefixes(bounds: &Bounds) -> ([i64; 7], [i64; 7]) {
    let mut low = [0i64; 7];
    let mut high = [0i64; 7];
    for coordinate in 0..6 {
        low[coordinate + 1] = low[coordinate] + bounds[2 * coordinate];
        high[coordinate + 1] = high[coordinate] + bounds[2 * coordinate + 1];
    }
    (low, high)
}

fn scaled_ratio_floor(numerator: i64, scale: i64, denominator: i64) -> Result<i64, String> {
    let result = i128::from(numerator) * i128::from(scale) / i128::from(denominator);
    i64::try_from(result).map_err(|_| "scaled rational lower bound overflow".to_string())
}

fn box_lower(prepared: &PreparedCertificate, bounds: &Bounds) -> Result<i64, String> {
    let (low, high) = prefixes(bounds);
    let mut result = i128::from(scaled_ratio_floor(
        low[6],
        SCALE,
        GRID * PRESSURE_DENOMINATOR,
    )?);
    for span in 1..=6usize {
        for start in 0..=6 - span {
            let left = low[start + span] - low[start];
            let right = high[start + span] - high[start] + i64::try_from(span).unwrap() - 1;
            let kernel = prepared.ranges.query(left, right)?;
            result += i128::from(COEFFICIENT_NUMERATORS[span]) * i128::from(kernel)
                / i128::from(COEFFICIENT_DENOMINATORS[span]);
        }
    }
    i64::try_from(result).map_err(|_| "box lower bound overflow".to_string())
}

fn lane_signature(digest: &str, lane_index: i64, stats: &CoverStats, minimum: i64) -> i64 {
    let payload = format!(
        "{digest}:{lane_index}:{}:{}:{}:{}:{}:{}:{}:{}:{minimum}",
        stats.initial_boxes,
        stats.nodes,
        stats.pruned,
        stats.splits,
        stats.maximum_depth,
        stats.pressure_pruned,
        stats.interval_pruned,
        stats.tangent_pruned,
    );
    let hash = blake3::hash(payload.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash.as_bytes()[..8]);
    i64::from_le_bytes(bytes) & i64::MAX
}

pub fn prepare_digest(encoded: &str) -> Result<String, String> {
    Ok(prepare(encoded)?.digest.clone())
}

fn prepared_by_digest(digest: &str) -> Result<Arc<PreparedCertificate>, String> {
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let guard = cache
        .lock()
        .map_err(|_| "zeta certificate cache mutex was poisoned".to_string())?;
    match guard.as_ref() {
        Some((_hash, prepared)) if prepared.digest == digest => Ok(Arc::clone(prepared)),
        _ => Err("zeta prepared certificate digest is not loaded".to_string()),
    }
}

pub fn verify_lane(encoded: &str, lane_index: i64, lane_count: i64) -> Result<LaneReport, String> {
    verify_prepared(prepare(encoded)?, lane_index, lane_count)
}

pub fn verify_prepared_lane(
    digest: &str,
    lane_index: i64,
    lane_count: i64,
) -> Result<LaneReport, String> {
    verify_prepared(prepared_by_digest(digest)?, lane_index, lane_count)
}

fn verify_prepared(
    prepared: Arc<PreparedCertificate>,
    lane_index: i64,
    lane_count: i64,
) -> Result<LaneReport, String> {
    if lane_count <= 0 || lane_index < 0 || lane_index >= lane_count {
        return Err(format!("invalid zeta cover lane {lane_index}/{lane_count}"));
    }
    let mut stats = CoverStats::empty();
    let mut used_tangents = HashSet::new();
    let mut minimum_leaf = i64::MAX;
    let mut minimum_kind = "";
    let mut minimum_box = [0i64; 12];

    for initial_index in 0..prepared.expected.initial_boxes {
        if initial_index % lane_count != lane_index {
            continue;
        }
        stats.initial_boxes += 1;
        let root = initial_box(initial_index, &prepared.components)?;
        let mut stack = vec![BoxNode {
            bounds: root,
            depth: 0,
        }];
        while let Some(node) = stack.pop() {
            stats.nodes += 1;
            stats.maximum_depth = stats.maximum_depth.max(node.depth);
            let pressure_cells = (0..6)
                .map(|coordinate| node.bounds[2 * coordinate])
                .sum::<i64>();

            if pressure_cells >= prepared.pressure_cutoff_cells {
                let pressure_lower =
                    scaled_ratio_floor(pressure_cells, SCALE, GRID * PRESSURE_DENOMINATOR)?;
                // The full 21-term interval expression is also valid here
                // and can reveal whether this existing cover proves a
                // slightly stronger target at no additional subdivision.
                let strengthened = pressure_lower.max(box_lower(&prepared, &node.bounds)?);
                if strengthened < minimum_leaf {
                    minimum_leaf = strengthened;
                    minimum_kind = "pressure";
                    minimum_box = node.bounds;
                }
                stats.pruned += 1;
                stats.pressure_pruned += 1;
                continue;
            }

            let lower = box_lower(&prepared, &node.bounds)?;
            if lower >= prepared.target_scaled {
                if lower < minimum_leaf {
                    minimum_leaf = lower;
                    minimum_kind = "interval";
                    minimum_box = node.bounds;
                }
                stats.pruned += 1;
                stats.interval_pruned += 1;
                continue;
            }

            if let Some(tangent_lower) = prepared.tangents.get(&node.bounds) {
                if *tangent_lower < prepared.target_scaled {
                    return Err("tangent certificate fell below target".to_string());
                }
                if !used_tangents.insert(node.bounds) {
                    return Err("a tangent box was consumed twice".to_string());
                }
                if *tangent_lower < minimum_leaf {
                    minimum_leaf = *tangent_lower;
                    minimum_kind = "tangent";
                    minimum_box = node.bounds;
                }
                stats.pruned += 1;
                stats.tangent_pruned += 1;
                continue;
            }

            let mut coordinate = 0usize;
            let mut maximum_width = node.bounds[1] - node.bounds[0];
            for candidate in 1..6 {
                let width = node.bounds[2 * candidate + 1] - node.bounds[2 * candidate];
                if width > maximum_width {
                    maximum_width = width;
                    coordinate = candidate;
                }
            }
            if maximum_width <= 0 {
                return Err(format!("unresolved terminal zeta box: {:?}", node.bounds));
            }
            let left_index = 2 * coordinate;
            let right_index = left_index + 1;
            let midpoint = (node.bounds[left_index] + node.bounds[right_index]) / 2;
            let mut lower_half = node.bounds;
            let mut upper_half = node.bounds;
            lower_half[right_index] = midpoint;
            upper_half[left_index] = midpoint + 1;
            stack.push(BoxNode {
                bounds: lower_half,
                depth: node.depth + 1,
            });
            stack.push(BoxNode {
                bounds: upper_half,
                depth: node.depth + 1,
            });
            stats.splits += 1;
        }
    }

    if stats.nodes != stats.pruned + stats.splits
        || stats.pruned != stats.pressure_pruned + stats.interval_pruned + stats.tangent_pruned
    {
        return Err("lane search counters are inconsistent".to_string());
    }
    let consumed =
        i64::try_from(used_tangents.len()).map_err(|_| "tangent count overflow".to_string())?;
    if consumed != stats.tangent_pruned {
        return Err("lane tangent-consumption count mismatch".to_string());
    }
    if minimum_leaf < prepared.target_scaled || minimum_leaf == i64::MAX {
        return Err("lane did not establish a finite target lower bound".to_string());
    }

    let signature = lane_signature(&prepared.digest, lane_index, &stats, minimum_leaf);
    Ok(LaneReport {
        verified: true,
        digest: prepared.digest.clone(),
        lane_index,
        lane_count,
        stats,
        minimum_leaf_lower_scaled: minimum_leaf,
        minimum_leaf_kind: minimum_kind.to_string(),
        minimum_leaf_box: minimum_box.to_vec(),
        signature,
    })
}
