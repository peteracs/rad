use num_bigint::BigUint;
use serde::Serialize;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy)]
struct Node {
    residue: u64,
    coefficient: u128,
    offset: u128,
    denominator: u128,
    probe: u128,
    peak: u128,
    odd_steps: u32,
}

#[derive(Clone, Serialize)]
struct TailReport {
    depth: u32,
    survivor_classes: u64,
    coefficient_stops: u64,
    descents: u64,
    unresolved: u64,
    max_coefficient_stop_step: u32,
    max_coefficient_stop_residue: u64,
    max_descent_step: u32,
    max_descent_residue: u64,
    max_additive_delay: u32,
    max_peak: u128,
    max_peak_residue: u64,
    coefficient_stop_histogram: Vec<u64>,
    descent_histogram: Vec<u64>,
}

fn empty_report(depth: u32, max_steps: u32) -> TailReport {
    TailReport {
        depth,
        survivor_classes: 0,
        coefficient_stops: 0,
        descents: 0,
        unresolved: 0,
        max_coefficient_stop_step: 0,
        max_coefficient_stop_residue: 0,
        max_descent_step: 0,
        max_descent_residue: 0,
        max_additive_delay: 0,
        max_peak: 0,
        max_peak_residue: 0,
        coefficient_stop_histogram: vec![0; max_steps as usize + 1],
        descent_histogram: vec![0; max_steps as usize + 1],
    }
}

fn step_node(node: Node, extension: u64) -> Result<Node, String> {
    let residue = node
        .residue
        .checked_add(
            u64::try_from(node.denominator)
                .map_err(|_| "denominator exceeds residue range".to_string())?
                * extension,
        )
        .ok_or_else(|| "residue overflow".to_string())?;
    let source = node
        .probe
        .checked_add(node.coefficient * u128::from(extension))
        .ok_or_else(|| "probe overflow".to_string())?;
    let odd = source & 1 == 1;
    let probe = if odd {
        source
            .checked_mul(3)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| "trajectory overflow".to_string())?
            / 2
    } else {
        source / 2
    };
    Ok(Node {
        residue,
        coefficient: if odd {
            node.coefficient
                .checked_mul(3)
                .ok_or_else(|| "coefficient overflow".to_string())?
        } else {
            node.coefficient
        },
        offset: if odd {
            node.offset
                .checked_mul(3)
                .and_then(|value| value.checked_add(node.denominator))
                .ok_or_else(|| "offset overflow".to_string())?
        } else {
            node.offset
        },
        denominator: node
            .denominator
            .checked_mul(2)
            .ok_or_else(|| "denominator overflow".to_string())?,
        probe,
        peak: node.peak.max(probe),
        odd_steps: node.odd_steps + u32::from(odd),
    })
}

fn prunable(node: Node, verified_bound: u128) -> bool {
    node.coefficient < node.denominator
        && node.offset / (node.denominator - node.coefficient) < verified_bound
}

fn minimum_noncontracting_odds(max_steps: u32) -> Vec<u32> {
    let mut three_power = BigUint::from(1u8);
    let mut two_power = BigUint::from(1u8);
    let three = BigUint::from(3u8);
    let mut odds = 0;
    let mut minimums = vec![0];
    for _ in 1..=max_steps {
        two_power <<= 1usize;
        while three_power < two_power {
            three_power *= &three;
            odds += 1;
        }
        minimums.push(odds);
    }
    minimums
}

fn choose_record(current_step: u32, current_residue: u64, step: u32, residue: u64) -> bool {
    step > current_step || (step == current_step && residue < current_residue)
}

fn record_tail(
    report: &mut TailReport,
    node: Node,
    max_steps: u32,
    minimum_odds: &[u32],
) -> Result<(), String> {
    if node.residue == 0 {
        return Err("zero survived the least-counterexample sieve".to_string());
    }
    let mut value = node.probe;
    let mut peak = u128::from(node.residue).max(node.peak);
    let mut odd_steps = node.odd_steps;
    let mut coefficient_stop =
        (odd_steps < minimum_odds[report.depth as usize]).then_some(report.depth);
    let mut descent = (value < u128::from(node.residue)).then_some(report.depth);
    for step in (report.depth + 1)..=max_steps {
        if value & 1 == 1 {
            value = value
                .checked_mul(3)
                .and_then(|next| next.checked_add(1))
                .ok_or_else(|| "natural-tail trajectory overflow".to_string())?
                / 2;
            odd_steps += 1;
        } else {
            value /= 2;
        }
        peak = peak.max(value);
        if coefficient_stop.is_none() && odd_steps < minimum_odds[step as usize] {
            coefficient_stop = Some(step);
        }
        if descent.is_none() && value < u128::from(node.residue) {
            descent = Some(step);
        }
        if coefficient_stop.is_some() && descent.is_some() {
            break;
        }
    }

    report.survivor_classes += 1;
    if let Some(step) = coefficient_stop {
        report.coefficient_stops += 1;
        report.coefficient_stop_histogram[step as usize] += 1;
        if choose_record(
            report.max_coefficient_stop_step,
            report.max_coefficient_stop_residue,
            step,
            node.residue,
        ) {
            report.max_coefficient_stop_step = step;
            report.max_coefficient_stop_residue = node.residue;
        }
    }
    if let Some(step) = descent {
        report.descents += 1;
        report.descent_histogram[step as usize] += 1;
        if choose_record(
            report.max_descent_step,
            report.max_descent_residue,
            step,
            node.residue,
        ) {
            report.max_descent_step = step;
            report.max_descent_residue = node.residue;
        }
    }
    match (coefficient_stop, descent) {
        (Some(coefficient), Some(descent)) => {
            report.max_additive_delay = report.max_additive_delay.max(descent - coefficient);
        }
        _ => report.unresolved += 1,
    }
    if peak > report.max_peak || (peak == report.max_peak && node.residue < report.max_peak_residue)
    {
        report.max_peak = peak;
        report.max_peak_residue = node.residue;
    }
    Ok(())
}

fn merge_report(target: &mut TailReport, source: TailReport) {
    target.survivor_classes += source.survivor_classes;
    target.coefficient_stops += source.coefficient_stops;
    target.descents += source.descents;
    target.unresolved += source.unresolved;
    target.max_additive_delay = target.max_additive_delay.max(source.max_additive_delay);
    for (target, source) in target
        .coefficient_stop_histogram
        .iter_mut()
        .zip(source.coefficient_stop_histogram)
    {
        *target += source;
    }
    for (target, source) in target
        .descent_histogram
        .iter_mut()
        .zip(source.descent_histogram)
    {
        *target += source;
    }
    if choose_record(
        target.max_coefficient_stop_step,
        target.max_coefficient_stop_residue,
        source.max_coefficient_stop_step,
        source.max_coefficient_stop_residue,
    ) {
        target.max_coefficient_stop_step = source.max_coefficient_stop_step;
        target.max_coefficient_stop_residue = source.max_coefficient_stop_residue;
    }
    if choose_record(
        target.max_descent_step,
        target.max_descent_residue,
        source.max_descent_step,
        source.max_descent_residue,
    ) {
        target.max_descent_step = source.max_descent_step;
        target.max_descent_residue = source.max_descent_residue;
    }
    if source.max_peak > target.max_peak
        || (source.max_peak == target.max_peak && source.max_peak_residue < target.max_peak_residue)
    {
        target.max_peak = source.max_peak;
        target.max_peak_residue = source.max_peak_residue;
    }
}

fn parse_arguments() -> Result<(Vec<u32>, u32, u32), String> {
    let mut arguments = std::env::args().skip(1);
    let depths = arguments
        .next()
        .ok_or_else(|| "usage: verifier DEPTHS VERIFIED_POWER MAX_STEPS".to_string())?
        .split(',')
        .map(|value| value.parse::<u32>().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let verified_power = arguments
        .next()
        .ok_or_else(|| "missing verified power".to_string())?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    let max_steps = arguments
        .next()
        .ok_or_else(|| "missing maximum steps".to_string())?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    if arguments.next().is_some()
        || depths.is_empty()
        || depths.windows(2).any(|pair| pair[0] >= pair[1])
        || *depths.last().unwrap() > 32
        || verified_power > 120
        || max_steps > 2048
    {
        return Err("invalid verifier bounds".to_string());
    }
    Ok((depths, verified_power, max_steps))
}

fn run() -> Result<Vec<TailReport>, String> {
    let (depths, verified_power, max_steps) = parse_arguments()?;
    let minimum_odds = minimum_noncontracting_odds(max_steps);
    let split_depth = depths[0];
    let task_count = 1usize << split_depth;
    let worker_count = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(16)
        .min(task_count);
    let next_task = AtomicUsize::new(0);
    let verified_bound = 1u128 << verified_power;
    let maximum_depth = *depths.last().unwrap();
    let workers = std::thread::scope(|scope| {
        let handles = (0..worker_count)
            .map(|_| {
                let depths = &depths;
                let minimum_odds = &minimum_odds;
                let next_task = &next_task;
                scope.spawn(move || {
                    let mut reports = depths
                        .iter()
                        .map(|depth| empty_report(*depth, max_steps))
                        .collect::<Vec<_>>();
                    loop {
                        let prefix = next_task.fetch_add(1, Ordering::Relaxed);
                        if prefix >= task_count {
                            break;
                        }
                        let mut node = Node {
                            residue: 0,
                            coefficient: 1,
                            offset: 0,
                            denominator: 1,
                            probe: 0,
                            peak: 0,
                            odd_steps: 0,
                        };
                        let mut prefix_pruned = false;
                        for bit in 0..split_depth {
                            node = step_node(node, ((prefix >> bit) & 1) as u64)?;
                            if prunable(node, verified_bound) {
                                prefix_pruned = true;
                                break;
                            }
                        }
                        if prefix_pruned {
                            continue;
                        }
                        let mut stack = vec![(split_depth, node)];
                        while let Some((depth, node)) = stack.pop() {
                            if let Ok(index) = depths.binary_search(&depth) {
                                record_tail(&mut reports[index], node, max_steps, minimum_odds)?;
                            }
                            if depth == maximum_depth {
                                continue;
                            }
                            for extension in [1, 0] {
                                let child = step_node(node, extension)?;
                                if !prunable(child, verified_bound) {
                                    stack.push((depth + 1, child));
                                }
                            }
                        }
                    }
                    Ok::<_, String>(reports)
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "verifier worker panicked".to_string())?
            })
            .collect::<Result<Vec<_>, String>>()
    })?;
    let mut reports = depths
        .iter()
        .map(|depth| empty_report(*depth, max_steps))
        .collect::<Vec<_>>();
    for worker in workers {
        for (target, source) in reports.iter_mut().zip(worker) {
            merge_report(target, source);
        }
    }
    Ok(reports)
}

fn main() {
    match run()
        .and_then(|reports| serde_json::to_string(&reports).map_err(|error| error.to_string()))
    {
        Ok(encoded) => println!("{encoded}"),
        Err(error) => {
            eprintln!("verification failed: {error}");
            std::process::exit(1);
        }
    }
}
