//! Enumerate inclusion-minimal Boolean test systems reaching a quotient threshold.
//!
//! Tests are nonempty subsets of a small Boolean universe.  A test observes
//! whether an input subset intersects it.  The program enumerates test systems
//! up to a column bound, keeps the inclusion-minimal systems whose observation
//! quotient reaches a requested size, and canonicalizes them under all
//! permutations of the underlying variables.

use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
struct Arguments {
    variables: usize,
    maximum_weight: usize,
    maximum_tests: usize,
    quotient_threshold: usize,
    output: PathBuf,
}

fn arguments() -> Result<Arguments, String> {
    let values = env::args().skip(1).collect::<Vec<_>>();
    if values.len() != 5 {
        return Err(
            "usage: projected_partition_frontier VARIABLES MAX_WEIGHT MAX_TESTS THRESHOLD OUTPUT"
                .into(),
        );
    }
    let number = |index: usize, name: &str| {
        values[index]
            .parse::<usize>()
            .map_err(|error| format!("invalid {name}: {error}"))
    };
    let variables = number(0, "variable count")?;
    if !(1..=6).contains(&variables) {
        return Err("variable count must lie in 1..=6".into());
    }
    Ok(Arguments {
        variables,
        maximum_weight: number(1, "maximum weight")?,
        maximum_tests: number(2, "maximum tests")?,
        quotient_threshold: number(3, "quotient threshold")?,
        output: PathBuf::from(&values[4]),
    })
}

fn permutations(size: usize) -> Vec<Vec<usize>> {
    fn extend(prefix: &mut Vec<usize>, used: &mut [bool], result: &mut Vec<Vec<usize>>) {
        if prefix.len() == used.len() {
            result.push(prefix.clone());
            return;
        }
        for value in 0..used.len() {
            if !used[value] {
                used[value] = true;
                prefix.push(value);
                extend(prefix, used, result);
                prefix.pop();
                used[value] = false;
            }
        }
    }
    let mut result = Vec::new();
    extend(&mut Vec::new(), &mut vec![false; size], &mut result);
    result
}

fn permute_pattern(pattern: usize, permutation: &[usize]) -> usize {
    permutation
        .iter()
        .enumerate()
        .fold(0usize, |result, (source, &target)| {
            if pattern >> source & 1 != 0 {
                result | (1usize << target)
            } else {
                result
            }
        })
}

fn refine(partition: &[u64], hit_rows: u64) -> Vec<u64> {
    let mut refined = Vec::with_capacity(partition.len() + 1);
    for &class in partition {
        let hit = class & hit_rows;
        let miss = class & !hit_rows;
        if miss != 0 {
            refined.push(miss);
        }
        if hit != 0 {
            refined.push(hit);
        }
    }
    refined
}

fn quotient_size(selection: u64, hit_rows: &[u64], row_count: usize) -> usize {
    let mut partition = vec![if row_count == 64 {
        u64::MAX
    } else {
        (1u64 << row_count) - 1
    }];
    for (index, &hits) in hit_rows.iter().enumerate() {
        if selection >> index & 1 != 0 {
            partition = refine(&partition, hits);
        }
    }
    partition.len()
}

struct Search<'a> {
    patterns: &'a [usize],
    hit_rows: &'a [u64],
    pattern_index: &'a BTreeMap<usize, usize>,
    permutations: &'a [Vec<usize>],
    maximum_tests: usize,
    threshold: usize,
    row_count: usize,
    visited: u64,
    labelled_cores: u64,
    orbit_cores: BTreeSet<u64>,
    labelled_by_size: BTreeMap<usize, u64>,
}

impl Search<'_> {
    fn canonical(&self, selection: u64) -> u64 {
        self.permutations
            .iter()
            .map(|permutation| {
                self.patterns
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| selection >> index & 1 != 0)
                    .fold(0u64, |mask, (_, &pattern)| {
                        let image = permute_pattern(pattern, permutation);
                        mask | (1u64 << self.pattern_index[&image])
                    })
            })
            .min()
            .expect("the permutation group contains the identity")
    }

    fn visit(&mut self, start: usize, selection: u64, chosen: usize, partition: &[u64]) {
        self.visited += 1;
        if partition.len() >= self.threshold {
            let minimal = (0..self.patterns.len())
                .filter(|&index| selection >> index & 1 != 0)
                .all(|index| {
                    quotient_size(selection & !(1u64 << index), self.hit_rows, self.row_count)
                        < self.threshold
                });
            if minimal {
                self.labelled_cores += 1;
                *self.labelled_by_size.entry(chosen).or_default() += 1;
                self.orbit_cores.insert(self.canonical(selection));
            }
            return;
        }
        if chosen == self.maximum_tests {
            return;
        }
        for index in start..self.patterns.len() {
            let refined = refine(partition, self.hit_rows[index]);
            self.visit(index + 1, selection | (1u64 << index), chosen + 1, &refined);
        }
    }
}

fn main() -> Result<(), String> {
    let args = arguments()?;
    let row_count = 1usize << args.variables;
    if args.quotient_threshold > row_count {
        return Err("quotient threshold exceeds the Boolean cube".into());
    }
    let patterns = (1usize..(1usize << args.variables))
        .filter(|pattern| pattern.count_ones() as usize <= args.maximum_weight)
        .collect::<Vec<_>>();
    if patterns.len() > 63 {
        return Err("the selected test class exceeds the 63-test mask".into());
    }
    let pattern_index = patterns
        .iter()
        .enumerate()
        .map(|(index, &pattern)| (pattern, index))
        .collect::<BTreeMap<_, _>>();
    let hit_rows = patterns
        .iter()
        .map(|&pattern| {
            (0..row_count).fold(0u64, |rows, subset| {
                if subset & pattern != 0 {
                    rows | (1u64 << subset)
                } else {
                    rows
                }
            })
        })
        .collect::<Vec<_>>();
    let group = permutations(args.variables);
    let initial_partition = vec![if row_count == 64 {
        u64::MAX
    } else {
        (1u64 << row_count) - 1
    }];
    let mut search = Search {
        patterns: &patterns,
        hit_rows: &hit_rows,
        pattern_index: &pattern_index,
        permutations: &group,
        maximum_tests: args.maximum_tests,
        threshold: args.quotient_threshold,
        row_count,
        visited: 0,
        labelled_cores: 0,
        orbit_cores: BTreeSet::new(),
        labelled_by_size: BTreeMap::new(),
    };
    search.visit(0, 0, 0, &initial_partition);

    let orbit_documents = search
        .orbit_cores
        .iter()
        .map(|&selection| {
            let tests = patterns
                .iter()
                .enumerate()
                .filter_map(|(index, &pattern)| (selection >> index & 1 != 0).then_some(pattern))
                .collect::<Vec<_>>();
            json!({
                "tests": tests,
                "test_count": tests.len(),
                "quotient_size": quotient_size(selection, &hit_rows, row_count),
            })
        })
        .collect::<Vec<_>>();
    let document = json!({
        "schema": "rad.boolean-lattice.projected-partition-frontier.v1",
        "variables": args.variables,
        "maximum_weight": args.maximum_weight,
        "maximum_tests": args.maximum_tests,
        "quotient_threshold": args.quotient_threshold,
        "test_patterns": patterns.len(),
        "permutations": group.len(),
        "visited_search_nodes": search.visited,
        "labelled_minimal_cores": search.labelled_cores,
        "symmetry_orbits": search.orbit_cores.len(),
        "labelled_by_test_count": search.labelled_by_size,
        "orbit_cores": orbit_documents,
    });
    let encoded = serde_json::to_string_pretty(&document)
        .map_err(|error| format!("failed to encode result: {error}"))?;
    fs::write(&args.output, encoded + "\n")
        .map_err(|error| format!("failed to write {}: {error}", args.output.display()))?;
    println!("{document}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singleton_tests_distinguish_the_boolean_cube() {
        let patterns = vec![1usize, 2, 4, 8, 16];
        let row_count = 32;
        let hit_rows = patterns
            .iter()
            .map(|pattern| {
                (0..row_count).fold(0u64, |rows, subset| {
                    if subset & pattern != 0 {
                        rows | (1u64 << subset)
                    } else {
                        rows
                    }
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(quotient_size(0b1_1111, &hit_rows, row_count), 32);
        assert_eq!(quotient_size(0b0_1111, &hit_rows, row_count), 16);
    }

    #[test]
    fn permutation_preserves_pattern_weight() {
        for permutation in permutations(5) {
            for pattern in 1usize..32 {
                assert_eq!(
                    pattern.count_ones(),
                    permute_pattern(pattern, &permutation).count_ones()
                );
            }
        }
    }
}
