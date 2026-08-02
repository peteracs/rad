use std::collections::{HashMap, HashSet};

use super::SystemRuntimeInfo;

fn read_write_sets(info: &SystemRuntimeInfo) -> (HashSet<String>, HashSet<String>) {
    let mut reads = HashSet::new();
    let mut writes = HashSet::new();
    for (_param_name, is_mut, comp_type) in &info.params {
        if *is_mut {
            writes.insert(comp_type.clone());
        } else {
            reads.insert(comp_type.clone());
        }
    }
    for (_param_name, is_mut, comp_type) in &info.resource_params {
        // `accum` resources are tracked separately (info.accum_resources):
        // their writes commute with other accum-writers of the same
        // resource, so they must not count as plain writes here — but they
        // are not reads either (the read is part of the fold).
        if info.accum_resources.contains(comp_type) {
            continue;
        }
        if *is_mut {
            writes.insert(comp_type.clone());
        } else {
            reads.insert(comp_type.clone());
        }
    }
    (reads, writes)
}

fn systems_conflict(a: &SystemRuntimeInfo, b: &SystemRuntimeInfo) -> bool {
    // Members of the same `serial phase` never share a batch, no matter how
    // disjoint their data access is — that is the declared intent ("these
    // systems are ordered, do not race them", dogfood feature seq 83).
    if let (Some(ga), Some(gb)) = (a.serial_group, b.serial_group) {
        if ga == gb {
            return true;
        }
    }
    let (a_reads, a_writes) = read_write_sets(a);
    let (b_reads, b_writes) = read_write_sets(b);
    // "*" is a synthetic body access whose target could not be resolved
    // statically (dynamic set_resource name, or a call to a fn whose
    // effects allow ECS writes — see compile_system_decl): treat it as
    // touching everything, so it serializes against any other ECS toucher.
    let a_touches = !a_reads.is_empty() || !a_writes.is_empty() || !a.accum_resources.is_empty();
    let b_touches = !b_reads.is_empty() || !b_writes.is_empty() || !b.accum_resources.is_empty();
    if a_writes.contains("*") && b_touches {
        return true;
    }
    if b_writes.contains("*") && a_touches {
        return true;
    }
    if a_reads.contains("*") && (!b_writes.is_empty() || !b.accum_resources.is_empty()) {
        return true;
    }
    if b_reads.contains("*") && (!a_writes.is_empty() || !a.accum_resources.is_empty()) {
        return true;
    }
    // `accum` folding only commutes with other accum-writers of the same
    // resource: a plain reader or writer of that resource must not share
    // the batch (it would observe or clobber an unfolded intermediate).
    if a.accum_resources
        .iter()
        .any(|r| b_reads.contains(r) || b_writes.contains(r))
    {
        return true;
    }
    if b.accum_resources
        .iter()
        .any(|r| a_reads.contains(r) || a_writes.contains(r))
    {
        return true;
    }
    !a_writes.is_disjoint(&b_writes)
        || !a_writes.is_disjoint(&b_reads)
        || !a_reads.is_disjoint(&b_writes)
}

pub fn partition_parallel_batches(
    ordered: &[String],
    systems: &HashMap<String, SystemRuntimeInfo>,
) -> Result<Vec<Vec<String>>, String> {
    let mut batches: Vec<Vec<String>> = Vec::new();
    for name in ordered {
        let info = systems
            .get(name)
            .ok_or_else(|| format!("Unknown system '{}'", name))?;
        // Walk candidate batches from the last one backwards: the system may
        // join a batch only if it conflicts with nothing in it, and it must
        // not jump over ANY later batch containing a conflicting system —
        // batches merge in order, so joining an earlier batch reorders the
        // system before everything in the batches it skips. (The previous
        // first-fit scan let a system run before a conflicting system that
        // was scheduled earlier, violating spec §7.2 "merges writes in
        // schedule order".)
        let mut join: Option<usize> = None;
        for (i, batch) in batches.iter().enumerate().rev() {
            let mut conflict = false;
            for existing_name in batch.iter() {
                let existing = systems
                    .get(existing_name)
                    .ok_or_else(|| format!("Unknown system '{}'", existing_name))?;
                if systems_conflict(info, existing) {
                    conflict = true;
                    break;
                }
            }
            if conflict {
                break;
            }
            join = Some(i);
        }
        match join {
            Some(i) => batches[i].push(name.clone()),
            None => batches.push(vec![name.clone()]),
        }
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(reads: &[&str], writes: &[&str]) -> SystemRuntimeInfo {
        SystemRuntimeInfo {
            params: reads
                .iter()
                .map(|r| ("p".to_string(), false, r.to_string()))
                .collect(),
            resource_params: writes
                .iter()
                .map(|w| ("q".to_string(), true, w.to_string()))
                .collect(),
            chunk_id: 0,
            after: Vec::new(),
            before: Vec::new(),
            serial_group: None,
            accum_resources: HashSet::new(),
        }
    }

    fn partition(specs: &[(&str, &[&str], &[&str])]) -> Vec<Vec<String>> {
        let mut systems = HashMap::new();
        let mut ordered = Vec::new();
        for (name, reads, writes) in specs {
            systems.insert(name.to_string(), sys(reads, writes));
            ordered.push(name.to_string());
        }
        partition_parallel_batches(&ordered, &systems).expect("known systems")
    }

    #[test]
    fn non_conflicting_systems_share_one_batch() {
        let batches = partition(&[("A", &[], &["X"]), ("B", &[], &["Y"]), ("C", &[], &["Z"])]);
        assert_eq!(batches, vec![vec!["A", "B", "C"]]);
    }

    #[test]
    fn conflicting_writers_serialize_in_schedule_order() {
        let batches = partition(&[("A", &[], &["X"]), ("B", &[], &["X"])]);
        assert_eq!(batches, vec![vec!["A"], vec!["B"]]);
    }

    /// Regression (dogfood seq 74 cluster): the old first-fit scan let C
    /// join A's batch even though C conflicts with B, reordering C before
    /// the earlier-scheduled B — batches merge in order, so that violated
    /// "merges writes in schedule order" (spec §7.2).
    #[test]
    fn later_system_cannot_jump_over_conflicting_batch() {
        let batches = partition(&[
            ("A", &[], &["X"]),
            ("B", &[], &["X", "Y"]),
            ("C", &["Y"], &[]),
        ]);
        assert_eq!(batches, vec![vec!["A"], vec!["B"], vec!["C"]]);
    }

    #[test]
    fn independent_system_may_join_earliest_conflict_free_batch() {
        let batches = partition(&[("A", &[], &["X"]), ("B", &[], &["X"]), ("C", &[], &["Z"])]);
        assert_eq!(batches, vec![vec!["A", "C"], vec!["B"]]);
    }

    /// "*" (statically unresolvable body write, dogfood seq 45) serializes
    /// against anything that touches ECS state...
    #[test]
    fn wildcard_body_write_conflicts_with_any_ecs_toucher() {
        let batches = partition(&[("A", &[], &["R"]), ("B", &[], &["*"])]);
        assert_eq!(batches, vec![vec!["A"], vec!["B"]]);
    }

    /// ...but not against a system with no ECS surface at all.
    #[test]
    fn wildcard_body_write_still_parallel_with_pure_system() {
        let batches = partition(&[("A", &[], &[]), ("B", &[], &["*"])]);
        assert_eq!(batches, vec![vec!["A", "B"]]);
    }

    /// `accum` (dogfood seq 83 IDEA 02): two accum-writers of the same
    /// resource commute — they share a batch and the merge folds their
    /// deltas — while a plain toucher of that resource still serializes.
    #[test]
    fn accum_writers_of_same_resource_share_a_batch() {
        let mut systems = HashMap::new();
        let mut a = sys(&[], &[]);
        a.resource_params
            .push(("t".to_string(), true, "T".to_string()));
        a.accum_resources.insert("T".to_string());
        let mut b = sys(&[], &[]);
        b.resource_params
            .push(("t".to_string(), true, "T".to_string()));
        b.accum_resources.insert("T".to_string());
        systems.insert("A".to_string(), a);
        systems.insert("B".to_string(), b);
        let ordered = vec!["A".to_string(), "B".to_string()];
        let batches = partition_parallel_batches(&ordered, &systems).expect("known systems");
        assert_eq!(batches, vec![vec!["A", "B"]]);
    }

    #[test]
    fn accum_writer_conflicts_with_plain_reader_of_same_resource() {
        let mut systems = HashMap::new();
        let mut a = sys(&[], &[]);
        a.resource_params
            .push(("t".to_string(), true, "T".to_string()));
        a.accum_resources.insert("T".to_string());
        systems.insert("A".to_string(), a);
        // B reads T without accum: it must not observe an unfolded
        // intermediate, so it lands in a later batch.
        systems.insert("B".to_string(), sys(&["T"], &[]));
        let ordered = vec!["A".to_string(), "B".to_string()];
        let batches = partition_parallel_batches(&ordered, &systems).expect("known systems");
        assert_eq!(batches, vec![vec!["A"], vec!["B"]]);
    }

    /// `serial phase` (dogfood seq 83): members of the same serial group are
    /// forced into separate batches even with fully disjoint data access,
    /// while an unrelated system may still parallelize with them.
    #[test]
    fn serial_phase_members_never_share_a_batch() {
        let mut systems = HashMap::new();
        let mut a = sys(&[], &["X"]);
        a.serial_group = Some(0);
        let mut b = sys(&[], &["Y"]);
        b.serial_group = Some(0);
        systems.insert("A".to_string(), a);
        systems.insert("B".to_string(), b);
        systems.insert("C".to_string(), sys(&[], &["Z"]));
        let ordered = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let batches = partition_parallel_batches(&ordered, &systems).expect("known systems");
        assert_eq!(batches, vec![vec!["A", "C"], vec!["B"]]);
    }
}
