//! Mean-payoff relaxation of the shadow-cycle workload.
//!
//! A node is `(r, i)`: `r = x mod 3^k` and bin `i` of `D = log2(x / n0)`, with
//! bins of width `1 / per_bit`. An even step sends `r -> r / 2`, `D -> D - 1`.
//! An odd step sends `r -> (3r + 1) / 2`, `D -> D + log2(3/2)`, landing in one of
//! two bins. The top bin is clamped, so an even step out of it may land anywhere
//! a clamped orbit could. A node is live when `(i + 1) / per_bit` exceeds
//! `barrier(r) - shift`; all quantities are in units of `2^-30`.
//!
//! `certify` is exact: weights are integers, built without floating point, and
//! every edge of a certificate is re-checked. `ceiling` is a floating-point
//! measurement; only `certify` proves anything.

use serde::Serialize;

/// One in fixed point: payoffs, barriers and shifts are in units of `2^-30`.
pub const ONE: i64 = 1 << 30;
const MAX_K: usize = 12;
const MAX_PER_BIT: usize = 8192;
const MAX_NODES: usize = 120_000_000;
/// Added to payoffs and barrier tests so rounding never favours a certificate.
const SLACK: i64 = 2;
/// `floor(2^13 log2(3/2))`, checked exactly by a test.
const ODD_STEP_Q13: usize = 4792;
/// Fraction bits of the `2^(-i / per_bit)` table.
const PAYOFF_BITS: u32 = 62;
const POTENTIAL_CAP: i64 = 1 << 50;
/// Bounds every cycle sum: `2^35 * MAX_NODES < 2^63`.
const MAX_WEIGHT: i64 = 1 << 35;
const CHECK_CYCLE_EVERY: usize = 16;
const NO_EDGE: u32 = u32::MAX;
const MAX_POLICY_ROUNDS: usize = 10_000;
const MAX_RATIO_ROUNDS: usize = 60;
// Weights reach 2^30, where one float ulp is about 2e-7, and a mean or bias sums
// many of them. A tolerance below that noise makes the policy cycle forever.
const MEAN_TOLERANCE: f64 = 1e-4;
/// Relative to the largest bias, which grows with path length.
const BIAS_TOLERANCE: f64 = 1e-11;
const RATIO_TOLERANCE: f64 = 1e-3;

#[derive(Clone, Copy, Debug)]
pub struct Relaxation<'a> {
    pub k: usize,
    pub per_bit: usize,
    pub top_bits: usize,
    pub shift: i64,
    pub barriers: &'a [i64],
}

/// The relaxation in compressed rows; node `s` owns edges `rows[s]..rows[s + 1]`.
struct Graph {
    rows: Vec<usize>,
    targets: Vec<u32>,
    payoffs: Vec<i64>,
    odd: Vec<bool>,
    live: usize,
    bins: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Every closed walk weighs at most zero.
    Certified,
    /// `witness` is a closed walk of positive weight.
    Witness,
    /// Neither: the search ended without a fixed point or a closed walk.
    Unresolved,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Potential {
    pub status: Status,
    pub sweeps: usize,
    pub max_potential: i64,
    pub witness: Vec<u32>,
    pub witness_weight: i64,
}

#[derive(Debug, Serialize)]
pub struct Certificate {
    #[serde(flatten)]
    pub potential: Potential,
    pub live: usize,
    pub edges: usize,
    pub bins: usize,
}

#[derive(Debug, Serialize)]
pub struct Ceiling {
    /// Maximum over closed walks of payoff per odd step, in units of `2^-30`.
    /// Zero when the graph has no closed walk with an odd step.
    pub ceiling: i64,
    pub ratio_rounds: usize,
    pub policy_rounds: usize,
    pub cycle: Vec<u32>,
    pub cycle_odd: usize,
    pub live: usize,
    pub edges: usize,
    pub bins: usize,
}

impl Relaxation<'_> {
    fn modulus(&self) -> Result<usize, String> {
        if self.k > MAX_K {
            return Err(format!("k must be at most {MAX_K}"));
        }
        let modulus = 3usize.pow(self.k as u32);
        if self.barriers.len() != modulus {
            return Err(format!(
                "expected {modulus} barriers, got {}",
                self.barriers.len()
            ));
        }
        if !self.per_bit.is_power_of_two() || self.per_bit > MAX_PER_BIT {
            return Err(format!(
                "bins per bit must be a power of two up to {MAX_PER_BIT}"
            ));
        }
        if !(2..=16).contains(&self.top_bits) {
            return Err("top bits must be in 2..=16".to_string());
        }
        Ok(modulus)
    }

    fn graph(&self) -> Result<Graph, String> {
        let modulus = self.modulus()?;
        let per_bit = self.per_bit;
        let bins = per_bit * self.top_bits;
        if modulus * bins > MAX_NODES {
            return Err(format!("graph too large: {} nodes", modulus * bins));
        }
        let unit = ONE / per_bit as i64;
        let live = |r: usize, i: usize| {
            (modulus == 1 || !r.is_multiple_of(3))
                && (i as i64 + 1) * unit > self.barriers[r] - self.shift - SLACK
        };
        let half = modulus.div_ceil(2);
        let odd_step = ODD_STEP_Q13 >> (MAX_PER_BIT.trailing_zeros() - per_bit.trailing_zeros());
        let payoff = payoff_table(per_bit, bins);
        let mut graph = Graph {
            rows: Vec::with_capacity(modulus * bins + 1),
            targets: Vec::new(),
            payoffs: Vec::new(),
            odd: Vec::new(),
            live: 0,
            bins,
        };
        graph.rows.push(0);
        for r in 0..modulus {
            let even_residue = (r * half) % modulus;
            let odd_residue = ((3 * r + 1) * half) % modulus;
            for (i, &step_payoff) in payoff.iter().enumerate() {
                if live(r, i) {
                    graph.live += 1;
                    let even_targets = if i == bins - 1 {
                        bins.saturating_sub(1 + per_bit)..bins
                    } else if i >= per_bit {
                        i - per_bit..i - per_bit + 1
                    } else {
                        0..0
                    };
                    for t in even_targets {
                        if live(even_residue, t) {
                            graph.push_edge(even_residue * bins + t, 0, false);
                        }
                    }
                    let low = (i + odd_step).min(bins - 1);
                    let high = (i + odd_step + 1).min(bins - 1);
                    if live(odd_residue, low) {
                        graph.push_edge(odd_residue * bins + low, step_payoff, true);
                    }
                    if high != low && live(odd_residue, high) {
                        graph.push_edge(odd_residue * bins + high, step_payoff, true);
                    }
                }
                graph.rows.push(graph.targets.len());
            }
        }
        if graph.targets.len() >= NO_EDGE as usize {
            return Err("graph has too many edges".to_string());
        }
        Ok(graph)
    }
}

impl Graph {
    fn push_edge(&mut self, target: usize, payoff: i64, odd: bool) {
        self.targets.push(target as u32);
        self.payoffs.push(payoff);
        self.odd.push(odd);
    }
}

/// Upper bounds on `2^30 2^(-i / per_bit)` plus `SLACK`, in exact integers.
fn payoff_table(per_bit: usize, bins: usize) -> Vec<i64> {
    let one = 1u128 << PAYOFF_BITS;
    let mut step = one / 2;
    for _ in 0..per_bit.trailing_zeros() {
        step = ceil_sqrt(step << PAYOFF_BITS);
    }
    let mut value = one;
    let shift = PAYOFF_BITS - 30;
    (0..bins)
        .map(|_| {
            let payoff = value.div_ceil(1 << shift) as i64 + SLACK;
            value = (value * step).div_ceil(one);
            payoff
        })
        .collect()
}

fn ceil_sqrt(value: u128) -> u128 {
    let root = value.isqrt();
    root + u128::from(root * root < value)
}

/// `Certified` when every closed walk has payoff at most `requirement` per odd
/// step; otherwise a witness walk that beats it.
pub fn certify(relaxation: &Relaxation, requirement: i64) -> Result<Certificate, String> {
    let graph = relaxation.graph()?;
    let weights: Vec<i64> = graph
        .payoffs
        .iter()
        .zip(&graph.odd)
        .map(|(&payoff, &odd)| if odd { payoff - requirement } else { 0 })
        .collect();
    let potential = longest_potential(&graph.rows, &graph.targets, &weights)?;
    Ok(Certificate {
        potential,
        live: graph.live,
        edges: graph.targets.len(),
        bins: graph.bins,
    })
}

/// Bellman-Ford for `V(s) = max(0, max over s -> t of w + V(t))`, in place from
/// zero. It converges exactly when no closed walk has positive weight, within
/// one sweep per node plus one to observe the fixed point.
fn longest_potential(
    rows: &[usize],
    targets: &[u32],
    weights: &[i64],
) -> Result<Potential, String> {
    let n = rows.len() - 1;
    if n >= MAX_NODES || weights.iter().any(|w| w.abs() > MAX_WEIGHT) {
        return Err("potential input exceeds the bounds that exclude overflow".to_string());
    }
    let mut potential = vec![0i64; n];
    let mut best_edge = vec![NO_EDGE; n];
    let mut last_raised = None;
    let mut max_potential = 0;
    let mut sweeps = 0;
    let converged = loop {
        if sweeps == n + 1 {
            break false;
        }
        sweeps += 1;
        let mut changed = false;
        for s in 0..n {
            let best = (rows[s]..rows[s + 1])
                .map(|e| (weights[e] + potential[targets[e] as usize], e))
                .max_by_key(|&(value, _)| value);
            if let Some((value, edge)) = best.filter(|&(value, _)| value > potential[s]) {
                potential[s] = value;
                best_edge[s] = edge as u32;
                changed = true;
                last_raised = Some(s);
                max_potential = max_potential.max(value);
                if value > POTENTIAL_CAP {
                    break;
                }
            }
        }
        if max_potential > POTENTIAL_CAP {
            break false;
        }
        if !changed {
            break true;
        }
        if sweeps % CHECK_CYCLE_EVERY == 0
            && positive_cycle(n, last_raised, targets, weights, &best_edge).is_some()
        {
            break false;
        }
    };
    let mut report = Potential {
        status: Status::Certified,
        sweeps,
        max_potential,
        witness: Vec::new(),
        witness_weight: 0,
    };
    if converged {
        let violated = (0..n).find(|&s| {
            (rows[s]..rows[s + 1])
                .any(|e| weights[e] + potential[targets[e] as usize] > potential[s])
        });
        if let Some(s) = violated {
            return Err(format!("potential violated at node {s}"));
        }
    } else if let Some((witness, weight)) =
        positive_cycle(n, last_raised, targets, weights, &best_edge)
    {
        report.status = Status::Witness;
        report.witness = witness;
        report.witness_weight = weight;
    } else {
        report.status = Status::Unresolved;
    }
    Ok(report)
}

/// The closed walk reached from `last_raised` along maximising edges, if it
/// has positive weight. Its weight is summed here, never assumed.
fn positive_cycle(
    n: usize,
    last_raised: Option<usize>,
    targets: &[u32],
    weights: &[i64],
    best_edge: &[u32],
) -> Option<(Vec<u32>, i64)> {
    let chosen = |s: usize| (best_edge[s] != NO_EDGE).then_some(best_edge[s] as usize);
    let cycle = first_cycle(n, last_raised?, |s| {
        chosen(s).map(|edge| targets[edge] as usize)
    })?;
    let weight = cycle
        .iter()
        .filter_map(|&s| chosen(s as usize))
        .map(|edge| weights[edge])
        .sum();
    (weight > 0).then_some((cycle, weight))
}

/// Follows a functional graph from `start` to its first repeated node.
fn first_cycle(n: usize, start: usize, next: impl Fn(usize) -> Option<usize>) -> Option<Vec<u32>> {
    let mut position = vec![u32::MAX; n];
    let mut path = Vec::new();
    let mut s = start;
    while position[s] == u32::MAX {
        position[s] = path.len() as u32;
        path.push(s as u32);
        s = next(s)?;
    }
    Some(path.split_off(position[s] as usize))
}

/// Dinkelbach iteration over Howard policy iteration: the maximum over closed
/// walks of payoff per odd step.
pub fn ceiling(relaxation: &Relaxation) -> Result<Ceiling, String> {
    let graph = relaxation.graph()?;
    let n = graph.rows.len() - 1;
    let mut policy: Vec<u32> = (0..n)
        .map(|s| {
            if graph.rows[s] < graph.rows[s + 1] {
                graph.rows[s] as u32
            } else {
                NO_EDGE
            }
        })
        .collect();
    let mut value = 0.0f64;
    let mut report = Ceiling {
        ceiling: 0,
        ratio_rounds: 0,
        policy_rounds: 0,
        cycle: Vec::new(),
        cycle_odd: 0,
        live: graph.live,
        edges: graph.targets.len(),
        bins: graph.bins,
    };
    while report.ratio_rounds < MAX_RATIO_ROUNDS {
        report.ratio_rounds += 1;
        let (mean, rounds) = maximum_cycle_mean(&graph, value, &mut policy)?;
        report.policy_rounds += rounds;
        let top = (0..n).max_by(|&a, &b| mean[a].total_cmp(&mean[b]));
        let Some(top) = top.filter(|&s| mean[s] > MEAN_TOLERANCE) else {
            report.ceiling = value.round() as i64;
            return Ok(report);
        };
        let walk = first_cycle(n, top, |s| {
            (policy[s] != NO_EDGE).then(|| graph.targets[policy[s] as usize] as usize)
        })
        .ok_or("the top cycle mean reached a dead end")?;
        let edges = || walk.iter().map(|&s| policy[s as usize] as usize);
        let payoff: f64 = edges().map(|e| graph.payoffs[e] as f64).sum();
        let odd = edges().filter(|&e| graph.odd[e]).count();
        if odd == 0 {
            return Err("a closed walk without odd steps has positive mean".to_string());
        }
        let ratio = payoff / odd as f64;
        report.cycle_odd = odd;
        report.cycle = walk;
        if ratio <= value + RATIO_TOLERANCE {
            report.ceiling = value.max(ratio).round() as i64;
            return Ok(report);
        }
        value = ratio;
    }
    Err(format!("no ratio fixed point in {MAX_RATIO_ROUNDS} rounds"))
}

/// Howard policy iteration for edge weights `payoff - value * odd`. Returns the
/// cycle mean reached from each node and the rounds used. Nodes that reach a
/// dead end have mean negative infinity and no bias.
fn maximum_cycle_mean(
    graph: &Graph,
    value: f64,
    policy: &mut [u32],
) -> Result<(Vec<f64>, usize), String> {
    const NEW: u32 = u32::MAX;
    const DONE: u32 = u32::MAX - 1;
    let n = policy.len();
    let weight = |e: usize| graph.payoffs[e] as f64 - if graph.odd[e] { value } else { 0.0 };
    let mut mean = vec![f64::NEG_INFINITY; n];
    let mut bias = vec![0f64; n];
    for round in 1..=MAX_POLICY_ROUNDS {
        // Evaluate the functional graph s -> targets[policy[s]]. While a node
        // is on the stack, `state` holds its stack position.
        let mut state: Vec<u32> = policy
            .iter()
            .map(|&edge| if edge == NO_EDGE { DONE } else { NEW })
            .collect();
        mean.fill(f64::NEG_INFINITY);
        bias.fill(0.0);
        let mut stack: Vec<usize> = Vec::new();
        for start in 0..n {
            let mut s = start;
            while state[s] == NEW {
                state[s] = stack.len() as u32;
                stack.push(s);
                s = graph.targets[policy[s] as usize] as usize;
            }
            if state[s] != DONE {
                let cycle = stack.split_off(state[s] as usize);
                let total: f64 = cycle.iter().map(|&x| weight(policy[x] as usize)).sum();
                let cycle_mean = total / cycle.len() as f64;
                for index in (1..cycle.len()).rev() {
                    let x = cycle[index];
                    let next = cycle[(index + 1) % cycle.len()];
                    bias[x] = weight(policy[x] as usize) - cycle_mean + bias[next];
                }
                for &x in &cycle {
                    mean[x] = cycle_mean;
                    state[x] = DONE;
                }
            }
            while let Some(x) = stack.pop() {
                let next = graph.targets[policy[x] as usize] as usize;
                mean[x] = mean[next];
                if mean[x].is_finite() {
                    bias[x] = weight(policy[x] as usize) - mean[x] + bias[next];
                }
                state[x] = DONE;
            }
        }
        if !improve_policy(graph, &weight, &mean, &bias, policy) {
            return Ok((mean, round));
        }
    }
    Err(format!(
        "policy did not converge in {MAX_POLICY_ROUNDS} rounds"
    ))
}

/// One improvement step: by cycle mean first, then by bias.
fn improve_policy(
    graph: &Graph,
    weight: &impl Fn(usize) -> f64,
    mean: &[f64],
    bias: &[f64],
    policy: &mut [u32],
) -> bool {
    let largest_bias = bias.iter().fold(1.0f64, |largest, b| largest.max(b.abs()));
    let bias_tolerance = BIAS_TOLERANCE * largest_bias;
    let mut changed = false;
    for s in 0..policy.len() {
        if policy[s] == NO_EDGE {
            continue;
        }
        let edges = graph.rows[s]..graph.rows[s + 1];
        let target = |e: usize| graph.targets[e] as usize;
        let mut best = policy[s] as usize;
        for e in edges.clone() {
            if mean[target(e)] > mean[target(best)] + MEAN_TOLERANCE {
                best = e;
            }
        }
        if mean[target(best)] > mean[s] + MEAN_TOLERANCE {
            policy[s] = best as u32;
            changed = true;
        } else if mean[s].is_finite() {
            let mut best_bias = bias[s];
            for e in edges {
                let t = target(e);
                if (mean[t] - mean[s]).abs() <= MEAN_TOLERANCE {
                    let candidate = weight(e) - mean[s] + bias[t];
                    if candidate > best_bias + bias_tolerance {
                        best_bias = candidate;
                        policy[s] = e as u32;
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

#[cfg(test)]
#[path = "shadow_walk/tests.rs"]
mod tests;
