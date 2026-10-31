//! Trapped-set survival on the maps `x -> x/2` (even), `x -> (3x + d)/2` (odd).
//!
//! For every exactly-`bits`-bit start whose parity walk has not contracted
//! through step `bits` (`3^q >= 2^t` on every prefix), this continues the real
//! dynamics and counts survivors at each mark. It also computes the exact
//! fair-coin null: survival probabilities from each state's odd count, by
//! backward recursion, giving mean and variance.
//!
//! Trapped states come from Terras's rule `T^t(r + 2^t u) = T^t(r) + 3^q u`, so
//! only surviving prefixes are visited. Arithmetic is overflow-checked `i128`;
//! subtrees run in parallel and the counts are order-independent sums.

use rayon::prelude::*;
use serde::Serialize;

const LOG2_3: f64 = 1.584_962_500_721_156_2;
const MAX_BITS: usize = 40;
const MAX_MARK: usize = 4000;
/// Parallel tasks start at this enumeration depth.
const SPLIT_DEPTH: usize = 16;

#[derive(Debug, PartialEq, Serialize)]
pub struct MarkSurvival {
    pub mark: usize,
    pub observed: u64,
    pub expected_x1000: i64,
    pub variance_x1000: i64,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Survival {
    pub bits: usize,
    pub d: i64,
    pub states: u64,
    pub marks: Vec<MarkSurvival>,
}

/// Smallest `q` with `3^q >= 2^t`; equality is impossible for `t >= 1`.
fn contraction_floor(t: usize) -> Result<i64, String> {
    if t == 0 {
        return Ok(0);
    }
    let x = t as f64 / LOG2_3;
    if (x - x.round()).abs() < 1e-9 {
        return Err(format!("floor at t = {t} is numerically ambiguous"));
    }
    Ok(x.ceil() as i64)
}

fn step(x: i128, d: i128) -> Result<i128, String> {
    if x % 2 == 0 {
        return Ok(x / 2);
    }
    x.checked_mul(3)
        .and_then(|tripled| tripled.checked_add(d))
        .map(|value| value / 2)
        .ok_or_else(|| "overflow in the real dynamics".to_string())
}

/// A prefix of the depth-first enumeration of trapped residues.
#[derive(Clone, Copy)]
struct Prefix {
    r: i128,
    t: usize,
    x: i128,
    power: i128,
    q: i64,
}

impl Prefix {
    /// Pushes the one-bit extensions that have not contracted.
    fn extend(self, d: i128, floors: &[i64], out: &mut Vec<Prefix>) -> Result<(), String> {
        for bit in 0..2i128 {
            let x = self.x + bit * self.power;
            let odd = x.rem_euclid(2) as i64;
            let q = self.q + odd;
            if q >= floors[self.t + 1] {
                let power = if odd == 1 {
                    self.power.checked_mul(3).ok_or("overflow in enumeration")?
                } else {
                    self.power
                };
                out.push(Prefix {
                    r: self.r + (bit << self.t),
                    t: self.t + 1,
                    x: step(x, d)?,
                    power,
                    q,
                });
            }
        }
        Ok(())
    }
}

/// Order-independent totals. States are never stored: the null needs only how
/// many start from each odd count.
struct Tally {
    states: u64,
    by_odd_count: Vec<u64>,
    observed: Vec<u64>,
}

impl Tally {
    fn new(bits: usize, marks: usize) -> Self {
        Tally {
            states: 0,
            by_odd_count: vec![0; bits + 1],
            observed: vec![0; marks],
        }
    }

    fn merge(mut self, other: Tally) -> Tally {
        self.states += other.states;
        for (total, count) in self.by_odd_count.iter_mut().zip(other.by_odd_count) {
            *total += count;
        }
        for (total, hits) in self.observed.iter_mut().zip(other.observed) {
            *total += hits;
        }
        self
    }
}

/// Follows one trapped start through the real dynamics, counting it at each
/// mark it survives to.
fn follow(
    start: i128,
    odd_count: i64,
    bits: usize,
    d: i128,
    marks: &[usize],
    floors: &[i64],
    observed: &mut [u64],
) -> Result<(), String> {
    let mut x = start;
    for _ in 0..bits {
        x = step(x, d)?;
    }
    let mut q = odd_count;
    for (t, &floor) in floors.iter().enumerate().skip(bits + 1) {
        q += x.rem_euclid(2) as i64;
        x = step(x, d)?;
        if q < floor {
            break;
        }
        for (slot, &mark) in observed.iter_mut().zip(marks) {
            *slot += u64::from(mark == t);
        }
    }
    Ok(())
}

/// Enumerates everything below `seed` on one thread.
fn survey(
    seed: Prefix,
    bits: usize,
    d: i128,
    marks: &[usize],
    floors: &[i64],
) -> Result<Tally, String> {
    let mut tally = Tally::new(bits, marks.len());
    let low = 1i128 << (bits - 1);
    let mut stack = vec![seed];
    while let Some(prefix) = stack.pop() {
        if prefix.t < bits {
            prefix.extend(d, floors, &mut stack)?;
        } else if prefix.r >= low {
            tally.states += 1;
            tally.by_odd_count[prefix.q as usize] += 1;
            follow(
                prefix.r,
                prefix.q,
                bits,
                d,
                marks,
                floors,
                &mut tally.observed,
            )?;
        }
    }
    Ok(tally)
}

/// Mean and variance of the survivor count under fair coins, by backward
/// recursion from the mark to step `bits`.
fn fair_coin_null(by_odd_count: &[u64], bits: usize, mark: usize, floors: &[i64]) -> (f64, f64) {
    let width = mark + 2;
    let mut row: Vec<f64> = (0..width)
        .map(|q| f64::from(u8::from(q as i64 >= floors[mark])))
        .collect();
    for t in (bits..mark).rev() {
        let mut previous = vec![0.0; width];
        for q in 0..width - 1 {
            if q as i64 >= floors[t] {
                previous[q] = 0.5 * (row[q] + row[q + 1]);
            }
        }
        row = previous;
    }
    let mut mean = 0.0;
    let mut variance = 0.0;
    for (q, &count) in by_odd_count.iter().enumerate() {
        let p = row[q.min(width - 1)];
        mean += count as f64 * p;
        variance += count as f64 * p * (1.0 - p);
    }
    (mean, variance)
}

pub fn trapped_survival(bits: usize, d: i64, marks: &[usize]) -> Result<Survival, String> {
    if !(2..=MAX_BITS).contains(&bits) {
        return Err(format!("bits must be in 2..={MAX_BITS}"));
    }
    if d % 2 == 0 {
        return Err("offset must be odd".to_string());
    }
    if marks.iter().any(|&mark| mark <= bits || mark > MAX_MARK) {
        return Err(format!("each mark must be in (bits, {MAX_MARK}]"));
    }
    let horizon = *marks.iter().max().ok_or("needs at least one mark")?;
    let floors = (0..=horizon)
        .map(contraction_floor)
        .collect::<Result<Vec<_>, _>>()?;
    let offset = i128::from(d);
    // The lift keeps every tracked value positive when d is negative.
    let root = Prefix {
        r: 0,
        t: 0,
        x: 1i128 << (bits + 2),
        power: 1,
        q: 0,
    };
    let mut seeds = vec![root];
    for _ in 0..SPLIT_DEPTH.min(bits) {
        let mut next = Vec::with_capacity(2 * seeds.len());
        for seed in seeds {
            seed.extend(offset, &floors, &mut next)?;
        }
        seeds = next;
    }
    let tally = seeds
        .par_iter()
        .map(|&seed| survey(seed, bits, offset, marks, &floors))
        .try_reduce(|| Tally::new(bits, marks.len()), |a, b| Ok(a.merge(b)))?;
    let marks = marks
        .iter()
        .zip(&tally.observed)
        .map(|(&mark, &observed)| {
            let (mean, variance) = fair_coin_null(&tally.by_odd_count, bits, mark, &floors);
            MarkSurvival {
                mark,
                observed,
                expected_x1000: (mean * 1000.0).round() as i64,
                variance_x1000: (variance * 1000.0).round() as i64,
            }
        })
        .collect();
    Ok(Survival {
        bits,
        d,
        states: tally.states,
        marks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_independent_rad_computation() {
        let found = trapped_survival(16, 1, &[32, 48, 64, 80, 96]).unwrap();
        let observed: Vec<u64> = found.marks.iter().map(|mark| mark.observed).collect();
        assert_eq!(observed, [313, 113, 48, 12, 8]);
        assert_eq!(found.marks[0].expected_x1000 / 100, 3139);
    }

    #[test]
    fn matches_direct_iteration_on_a_negative_offset() {
        let (bits, d, mark) = (10usize, -1i128, 30usize);
        let floors: Vec<i64> = (0..=mark).map(|t| contraction_floor(t).unwrap()).collect();
        let mut survivors = 0;
        for start in (1i128 << (bits - 1))..(1i128 << bits) {
            let mut x = start;
            let mut q = 0;
            let alive = (1..=mark).all(|t| {
                q += x.rem_euclid(2) as i64;
                x = step(x, d).unwrap();
                q >= floors[t]
            });
            survivors += u64::from(alive);
        }
        let found = trapped_survival(bits, -1, &[mark]).unwrap();
        assert_eq!(found.marks[0].observed, survivors);
    }

    #[test]
    fn invalid_parameters_are_refused() {
        assert!(trapped_survival(16, 2, &[40]).unwrap_err().contains("odd"));
        assert!(trapped_survival(16, 1, &[10]).unwrap_err().contains("mark"));
        assert!(trapped_survival(16, 1, &[]).unwrap_err().contains("mark"));
        assert!(trapped_survival(1, 1, &[10]).unwrap_err().contains("bits"));
    }
}
