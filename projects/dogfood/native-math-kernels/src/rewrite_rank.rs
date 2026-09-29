//! Exact local search for nonnegative affine matrix rewrite interpretations.
//! The caller supplies the rewriting system. A zero search score is a
//! candidate certificate, never a trusted termination verdict.

use serde::{Deserialize, Serialize};

const MODULUS: i64 = 2_147_483_647;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub dimension: usize,
    pub maximum: i64,
    pub matrices: Vec<Vec<Vec<i64>>>,
    pub rules: Vec<(Vec<usize>, Vec<usize>)>,
    pub active_mask: u32,
    pub strict_mask: u32,
    pub reverse: bool,
    pub ground: Option<usize>,
    pub seed: i64,
    pub trials: usize,
    pub strict_weight: i64,
    pub temperature: i64,
    pub stop_on_zero: bool,
    #[serde(default)]
    pub full_rescore: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Score {
    pub weak: i64,
    pub strict: i64,
    pub total: i64,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub best: Score,
    pub initial: Score,
    pub final_score: Score,
    pub matrices: Vec<Vec<Vec<i64>>>,
    pub seed: i64,
    pub evaluated: usize,
    pub accepted: usize,
    pub checksum: i64,
    pub rule_evaluations: usize,
}

type Matrix = [i64; 81];

fn advance(seed: &mut i64) -> i64 {
    *seed = (*seed * 48_271) % MODULUS;
    *seed
}

fn validate(request: &Request) -> Result<(), String> {
    let d = request.dimension;
    if !(1..=8).contains(&d) || !(1..=31).contains(&request.maximum) {
        return Err("rewrite rank matrix domain".into());
    }
    if request.matrices.is_empty()
        || request.matrices.len() > 16
        || request.rules.is_empty()
        || request.rules.len() > 31
        || request.trials > 2_000_000
        || !(1..MODULUS).contains(&request.seed)
        || !(1..=1024).contains(&request.strict_weight)
        || !(0..=1_000_000).contains(&request.temperature)
    {
        return Err("rewrite rank search domain".into());
    }
    let allowed = (1u32 << request.rules.len()) - 1;
    if request.active_mask & !allowed != 0
        || request.strict_mask == 0
        || request.strict_mask & !request.active_mask != 0
    {
        return Err("rewrite rank active/strict masks".into());
    }
    if request.ground.is_some_and(|i| i >= request.matrices.len()) {
        return Err("rewrite rank ground symbol".into());
    }
    for (symbol, a) in request.matrices.iter().enumerate() {
        if a.len() != d + 1 || a.iter().any(|row| row.len() != d + 1) {
            return Err("rewrite rank matrix shape".into());
        }
        for (i, row) in a.iter().enumerate() {
            for (j, &v) in row.iter().enumerate() {
                if i == d {
                    if v != i64::from(j == d) {
                        return Err("rewrite rank homogeneous row".into());
                    }
                } else if !(0..=request.maximum).contains(&v) {
                    return Err("rewrite rank coefficient range".into());
                } else if request.ground == Some(symbol) && j < d && v != 0 {
                    return Err("rewrite rank nonconstant ground symbol".into());
                }
            }
        }
    }
    for (left, right) in &request.rules {
        for word in [left, right] {
            if word.is_empty()
                || word.len() > 3
                || word.iter().any(|&s| s >= request.matrices.len())
            {
                return Err("rewrite rank word domain".into());
            }
        }
    }
    Ok(())
}

fn flatten(request: &Request) -> Vec<Matrix> {
    request
        .matrices
        .iter()
        .map(|a| {
            let mut result = [0; 81];
            for (i, row) in a.iter().enumerate() {
                for (j, &v) in row.iter().enumerate() {
                    result[i * 9 + j] = v;
                }
            }
            result
        })
        .collect()
}

fn unpack(matrices: &[Matrix], d: usize) -> Vec<Vec<Vec<i64>>> {
    matrices
        .iter()
        .map(|a| {
            (0..=d)
                .map(|i| (0..=d).map(|j| a[i * 9 + j]).collect())
                .collect()
        })
        .collect()
}

fn multiply(a: &Matrix, b: &Matrix, d: usize) -> Matrix {
    let mut result = [0; 81];
    for i in 0..d {
        for k in 0..=d {
            let value = a[i * 9 + k];
            if value != 0 {
                for j in 0..=d {
                    result[i * 9 + j] += value * b[k * 9 + j];
                }
            }
        }
    }
    result[d * 9 + d] = 1;
    result
}

fn interpret(word: &[usize], matrices: &[Matrix], d: usize, reverse: bool) -> Matrix {
    // Avoid multiplying by an identity for the first symbol.
    let index = |i: usize| if reverse { word.len() - 1 - i } else { i };
    let mut result = matrices[word[index(0)]];
    for i in 1..word.len() {
        result = multiply(&result, &matrices[word[index(i)]], d);
    }
    result
}

fn rule_score(request: &Request, matrices: &[Matrix], rule: usize) -> (i64, i64) {
    if request.active_mask & (1 << rule) == 0 {
        return (0, 0);
    }
    let (left, right) = &request.rules[rule];
    let a = interpret(left, matrices, request.dimension, request.reverse);
    let b = interpret(right, matrices, request.dimension, request.reverse);
    let mut weak = 0;
    for i in 0..request.dimension {
        for j in 0..=request.dimension {
            weak += (b[i * 9 + j] - a[i * 9 + j]).max(0);
        }
    }
    let strict = if request.strict_mask & (1 << rule) != 0 {
        (1 + b[request.dimension] - a[request.dimension]).max(0)
    } else {
        0
    };
    (weak, strict)
}

fn score(parts: &[(i64, i64)], weight: i64) -> Score {
    let weak = parts.iter().map(|p| p.0).sum::<i64>();
    let strict = parts.iter().map(|p| p.1).sum::<i64>();
    Score {
        weak,
        strict,
        total: weak + weight * strict,
    }
}

pub fn search(request: &Request) -> Result<Report, String> {
    validate(request)?;
    let d = request.dimension;
    let mut matrices = flatten(request);
    let mutable = (0..matrices.len())
        .flat_map(|s| {
            (0..d).flat_map(move |i| {
                (0..=d).filter_map(move |j| {
                    (request.ground != Some(s) || j == d).then_some((s, i * 9 + j))
                })
            })
        })
        .collect::<Vec<_>>();
    let dependencies = (0..matrices.len())
        .map(|s| {
            request
                .rules
                .iter()
                .enumerate()
                .filter_map(|(i, (l, r))| {
                    ((request.active_mask & (1 << i) != 0) && (l.contains(&s) || r.contains(&s)))
                        .then_some(i)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let active = (0..request.rules.len())
        .filter(|&i| request.active_mask & (1 << i) != 0)
        .collect::<Vec<_>>();
    let mut parts = (0..request.rules.len())
        .map(|i| rule_score(request, &matrices, i))
        .collect::<Vec<_>>();
    let initial = score(&parts, request.strict_weight);
    let mut current = initial.clone();
    let mut best = current.clone();
    let mut best_matrices = matrices.clone();
    let mut seed = request.seed;
    let mut evaluated = 0;
    let mut accepted = 0;
    let mut checksum = 0;
    let mut rule_evaluations = active.len();
    for iteration in 0..request.trials {
        if request.stop_on_zero && best.total == 0 {
            break;
        }
        let (symbol, offset) = mutable[(advance(&mut seed) as usize) % mutable.len()];
        let value = advance(&mut seed) % (request.maximum + 1);
        let chance = advance(&mut seed);
        let old = matrices[symbol][offset];
        matrices[symbol][offset] = value;
        let affected = if request.full_rescore {
            &active
        } else {
            &dependencies[symbol]
        };
        let old_parts = parts.clone();
        for &i in affected {
            parts[i] = rule_score(request, &matrices, i);
        }
        rule_evaluations += affected.len();
        let candidate = score(&parts, request.strict_weight);
        checksum = (checksum + (candidate.total % MODULUS) * (1 + iteration as i64 % 97)) % MODULUS;
        evaluated += 1;
        let temperature = request.temperature * (1024 - iteration as i64 % 1024) / 1024;
        let delta = candidate.total - current.total;
        if delta <= 0 || (temperature > 0 && chance % (delta + temperature) < temperature) {
            current = candidate;
            accepted += 1;
            if current.total < best.total {
                best = current.clone();
                best_matrices = matrices.clone();
            }
        } else {
            matrices[symbol][offset] = old;
            parts = old_parts;
        }
    }
    Ok(Report {
        best,
        initial,
        final_score: current,
        matrices: unpack(&best_matrices, d),
        seed,
        evaluated,
        accepted,
        checksum,
        rule_evaluations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        Request {
            dimension: 2,
            maximum: 7,
            matrices: vec![vec![vec![1, 1, 0], vec![1, 0, 0], vec![0, 0, 1]]; 3],
            rules: vec![
                (vec![0, 1], vec![1, 0]),
                (vec![2, 0], vec![2, 1, 1]),
                (vec![0, 2], vec![2]),
            ],
            active_mask: 7,
            strict_mask: 2,
            reverse: false,
            ground: None,
            seed: 1937,
            trials: 2000,
            strict_weight: 8,
            temperature: 8,
            stop_on_zero: false,
            full_rescore: false,
        }
    }

    #[test]
    fn incremental_walk_matches_full_recomputation() {
        for reverse in [false, true] {
            let mut req = request();
            req.reverse = reverse;
            let mut fast = search(&req).unwrap();
            req.full_rescore = true;
            let full = search(&req).unwrap();
            assert!(fast.rule_evaluations < full.rule_evaluations);
            fast.rule_evaluations = full.rule_evaluations;
            assert_eq!(fast, full);
        }
    }

    #[test]
    fn malformed_certificates_are_rejected() {
        let mut req = request();
        req.matrices[0][2][0] = 1;
        assert!(search(&req).is_err());
        req = request();
        req.strict_mask = 8;
        assert!(search(&req).is_err());
        req = request();
        req.ground = Some(0);
        assert!(search(&req).is_err());
        req = request();
        req.matrices[0][0][0] = -1;
        assert!(search(&req).is_err());
    }
}
