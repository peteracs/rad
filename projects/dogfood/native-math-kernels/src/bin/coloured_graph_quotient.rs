//! Exact stream evaluator for Boolean quotients encoded as vertex-coloured graphs.
//!
//! A colour-one vertex represents a singleton incidence column and an edge
//! represents a two-vertex incidence column.  The input format is nauty
//! `vcolg -T` text, but the evaluator depends only on the documented records.

use serde_json::{json, Value as JsonValue};
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

struct Arguments {
    input: PathBuf,
    output: PathBuf,
    generator_count: usize,
    minimum_columns: usize,
    minimum_family_size: usize,
    maximum_family_size: usize,
}

fn arguments() -> Result<Arguments, String> {
    let values = env::args().skip(1).collect::<Vec<_>>();
    if values.len() != 6 {
        return Err(
            "usage: coloured_graph_quotient INPUT OUTPUT GENERATORS MIN_COLUMNS MIN_FAMILY MAX_FAMILY"
                .to_string(),
        );
    }
    let number = |index: usize, name: &str| {
        values[index]
            .parse::<usize>()
            .map_err(|error| format!("invalid {name}: {error}"))
    };
    Ok(Arguments {
        input: PathBuf::from(&values[0]),
        output: PathBuf::from(&values[1]),
        generator_count: number(2, "generator count")?,
        minimum_columns: number(3, "minimum column count")?,
        minimum_family_size: number(4, "minimum family size")?,
        maximum_family_size: number(5, "maximum family size")?,
    })
}

fn edge_pairs(generator_count: usize) -> Vec<(usize, usize)> {
    (0..generator_count)
        .flat_map(|left| ((left + 1)..generator_count).map(move |right| (left, right)))
        .collect()
}

fn induced_edge_masks(generator_count: usize, edges: &[(usize, usize)]) -> Vec<u64> {
    (0..(1usize << generator_count))
        .map(|subset| {
            edges
                .iter()
                .enumerate()
                .fold(0u64, |mask, (index, &(left, right))| {
                    if subset >> left & 1 != 0 && subset >> right & 1 != 0 {
                        mask | (1u64 << index)
                    } else {
                        mask
                    }
                })
        })
        .collect()
}

fn parse_record(
    line: &str,
    generator_count: usize,
    edges: &[(usize, usize)],
) -> Result<(u64, u64), String> {
    let fields = line
        .split_whitespace()
        .map(|field| {
            field
                .parse::<usize>()
                .map_err(|error| format!("invalid integer `{field}`: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if fields.len() < 2 + generator_count || fields[0] != generator_count {
        return Err("record has the wrong vertex count".to_string());
    }
    let edge_count = fields[1];
    if fields.len() != 2 + generator_count + 2 * edge_count {
        return Err("record has the wrong number of edge endpoints".to_string());
    }
    let mut loop_mask = 0u64;
    for (vertex, &colour) in fields[2..(2 + generator_count)].iter().enumerate() {
        if colour > 1 {
            return Err("only vertex colours zero and one are supported".to_string());
        }
        loop_mask |= (colour as u64) << vertex;
    }
    let mut edge_mask = 0u64;
    for endpoints in fields[(2 + generator_count)..].chunks_exact(2) {
        let edge = if endpoints[0] < endpoints[1] {
            (endpoints[0], endpoints[1])
        } else {
            (endpoints[1], endpoints[0])
        };
        let index = edges
            .iter()
            .position(|candidate| *candidate == edge)
            .ok_or_else(|| "record contains an invalid edge".to_string())?;
        edge_mask |= 1u64 << index;
    }
    if edge_mask.count_ones() as usize != edge_count {
        return Err("record contains a duplicate edge".to_string());
    }
    Ok((loop_mask, edge_mask))
}

fn evaluate(
    generator_count: usize,
    edge_pattern_count: usize,
    loop_mask: u64,
    edge_mask: u64,
    induced_edges: &[u64],
) -> (usize, Vec<usize>, i64) {
    let mut outputs = (0..(1usize << generator_count))
        .map(|subset| {
            (loop_mask & subset as u64) | ((edge_mask & induced_edges[subset]) << generator_count)
        })
        .collect::<Vec<_>>();
    outputs.sort_unstable();
    outputs.dedup();
    let mut absent_counts = vec![0usize; generator_count + edge_pattern_count];
    for &output in &outputs {
        let mut loops = output & ((1u64 << generator_count) - 1);
        while loops != 0 {
            absent_counts[loops.trailing_zeros() as usize] += 1;
            loops &= loops - 1;
        }
        let mut present_edges = output >> generator_count;
        while present_edges != 0 {
            absent_counts[generator_count + present_edges.trailing_zeros() as usize] += 1;
            present_edges &= present_edges - 1;
        }
    }
    let family_size = outputs.len();
    let frequencies = (0..generator_count)
        .filter(|vertex| loop_mask & (1u64 << vertex) != 0)
        .map(|vertex| family_size - absent_counts[vertex])
        .chain(
            (0..edge_pattern_count)
                .filter(|index| edge_mask & (1u64 << index) != 0)
                .map(|index| family_size - absent_counts[generator_count + index]),
        )
        .collect::<Vec<_>>();
    let maximum = frequencies.iter().copied().max().unwrap_or(0);
    (
        family_size,
        frequencies,
        2 * maximum as i64 - family_size as i64,
    )
}

fn witness(
    loop_mask: u64,
    edge_mask: u64,
    family_size: usize,
    frequencies: &[usize],
    margin: i64,
) -> JsonValue {
    json!({
        "loop_mask": loop_mask,
        "edge_mask": edge_mask,
        "column_count": loop_mask.count_ones() + edge_mask.count_ones(),
        "family_size": family_size,
        "frequencies": frequencies,
        "margin": margin,
    })
}

fn run(arguments: &Arguments) -> Result<JsonValue, String> {
    if !(1..=10).contains(&arguments.generator_count) {
        return Err("generator count must lie in 1..=10".to_string());
    }
    if arguments.minimum_family_size > arguments.maximum_family_size {
        return Err("family-size interval is invalid".to_string());
    }
    let edges = edge_pairs(arguments.generator_count);
    let induced_edges = induced_edge_masks(arguments.generator_count, &edges);
    let input = BufReader::new(File::open(&arguments.input).map_err(|error| error.to_string())?);
    let mut digest = blake3::Hasher::new();
    let mut coloured_graph_orbits = 0u64;
    let mut scanned_orbits = 0u64;
    let mut frontier_orbits = 0u64;
    let mut smallest_family_size = usize::MAX;
    let mut smallest_family = JsonValue::Null;
    let mut minimum_margin = i64::MAX;
    let mut best = JsonValue::Null;
    let mut counterexample = JsonValue::Null;
    for (line_number, line) in input.lines().enumerate() {
        let line = line.map_err(|error| error.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        coloured_graph_orbits += 1;
        let (loop_mask, edge_mask) = parse_record(&line, arguments.generator_count, &edges)
            .map_err(|error| format!("line {}: {error}", line_number + 1))?;
        let column_count = loop_mask.count_ones() as usize + edge_mask.count_ones() as usize;
        if column_count < arguments.minimum_columns {
            continue;
        }
        scanned_orbits += 1;
        let (family_size, frequencies, margin) = evaluate(
            arguments.generator_count,
            edges.len(),
            loop_mask,
            edge_mask,
            &induced_edges,
        );
        if family_size < smallest_family_size {
            smallest_family_size = family_size;
            smallest_family = witness(loop_mask, edge_mask, family_size, &frequencies, margin);
        }
        digest.update(&loop_mask.to_le_bytes());
        digest.update(&edge_mask.to_le_bytes());
        digest.update(&(family_size as u32).to_le_bytes());
        digest.update(&margin.to_le_bytes());
        if (arguments.minimum_family_size..=arguments.maximum_family_size).contains(&family_size) {
            frontier_orbits += 1;
            let candidate = witness(loop_mask, edge_mask, family_size, &frequencies, margin);
            if margin < minimum_margin {
                minimum_margin = margin;
                best = candidate.clone();
            }
            if margin < 0 && counterexample.is_null() {
                counterexample = candidate;
            }
        }
    }
    Ok(json!({
        "schema": "rad.boolean-lattice.coloured-graph-quotient.v1",
        "generator_count": arguments.generator_count,
        "minimum_column_count": arguments.minimum_columns,
        "maximum_column_count": arguments.generator_count + edges.len(),
        "maximum_column_weight": 2,
        "minimum_family_size": arguments.minimum_family_size,
        "maximum_family_size": arguments.maximum_family_size,
        "coloured_graph_orbits": coloured_graph_orbits,
        "scanned_orbits": scanned_orbits,
        "frontier_orbits": frontier_orbits,
        "smallest_family_size": if scanned_orbits == 0 { JsonValue::Null } else { json!(smallest_family_size) },
        "smallest_family": smallest_family,
        "minimum_margin": if frontier_orbits == 0 { JsonValue::Null } else { json!(minimum_margin) },
        "best": best,
        "counterexample": counterexample,
        "signature": digest.finalize().to_hex().to_string(),
    }))
}

fn main() -> Result<(), String> {
    let arguments = arguments()?;
    let result = run(&arguments)?;
    let encoded = serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?;
    std::fs::write(&arguments.output, format!("{encoded}\n")).map_err(|error| error.to_string())?;
    println!("{encoded}");
    if result["counterexample"].is_null() {
        Ok(())
    } else {
        Err("counterexample found".to_string())
    }
}
