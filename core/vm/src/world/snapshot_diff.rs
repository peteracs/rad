// Differences between two world snapshots: which entities were touched and
// what changed on each.

impl WorldSnapshot {

    /// Cheap per-component change summary between two snapshots: component
    /// type name → changed-row count (an upper bound).
    ///
    /// Cost is O(archetypes × columns) `Arc::ptr_eq` comparisons — the CoW
    /// architecture means an untouched column shares its `Arc`ed field
    /// vectors with the base snapshot, so "what changed" is a pointer
    /// comparison, not a scan. When a column's pointer differs we report its
    /// row count (we know *that* it changed, not *which* rows). Resources
    /// count 1 per changed entry.
    ///
    /// Archetypes are paired by index: `restore()` preserves vector order and
    /// mutation only appends, so index i refers to the same archetype in both
    /// snapshots whenever both have one.
    pub fn diff_summary(
        base: &WorldSnapshot,
        new: &WorldSnapshot,
    ) -> std::collections::BTreeMap<String, usize> {
        // The positional fast path below matches archetype i to archetype i,
        // row r to row r — only meaningful for snapshots with shared lineage
        // (CoW forks of one world agree on archetype order and row layout).
        // Worlds rebuilt from scratch — merge results, wire-decoded forks,
        // retro replays — order archetypes and rows differently; positional
        // comparison produces phantom diffs. TypeIds are world-local, so the
        // check compares column type *names* and the row→entity assignment;
        // anything misaligned takes the O(world) semantic path instead.
        let aligned = base
            .archetypes
            .iter()
            .zip(new.archetypes.iter())
            .all(|(a, b)| {
                if !(Arc::ptr_eq(&a.entities, &b.entities) || a.entities == b.entities) {
                    return false;
                }
                fn names(arch: &Archetype) -> Vec<&str> {
                    let mut v: Vec<&str> = arch
                        .columns
                        .values()
                        .map(|c| c.type_name.as_str())
                        .collect();
                    v.sort_unstable();
                    v
                }
                names(a) == names(b)
            });
        if !aligned {
            return Self::diff_summary_by_entity(base, new);
        }
        let mut out = std::collections::BTreeMap::new();
        for (i, arch_new) in new.archetypes.iter().enumerate() {
            let arch_base = base.archetypes.get(i);
            for (tid, col_new) in &arch_new.columns {
                let changed = match arch_base.and_then(|a| a.columns.get(tid)) {
                    Some(col_base) => {
                        let all_shared = col_new.fields.len() == col_base.fields.len()
                            && col_new
                                .fields
                                .iter()
                                .zip(&col_base.fields)
                                .all(|(a, b)| Arc::ptr_eq(a, b));
                        if all_shared {
                            // CoW fast path: shared Arcs mean untouched data.
                            0
                        } else {
                            // Unshared columns happen on CoW writes: compare
                            // row values structurally (`Value ==` is a bit
                            // compare for scalars and a deep structural
                            // compare for heap objects — no allocation).
                            let rows_new = col_new.len();
                            let rows_base = col_base.len();
                            let common = rows_new.min(rows_base);
                            let mut changed = rows_new.max(rows_base) - common;
                            for r in 0..common {
                                let differs =
                                    col_new.fields.iter().zip(&col_base.fields).any(|(fa, fb)| {
                                        if Arc::ptr_eq(fa, fb) {
                                            return false;
                                        }
                                        match (fa.as_slice().get(r), fb.as_slice().get(r)) {
                                            (Some(a), Some(b)) => a != b,
                                            _ => true,
                                        }
                                    });
                                if differs {
                                    changed += 1;
                                }
                            }
                            changed
                        }
                    }
                    // Archetype (or column) absent in base: every row is new.
                    None => col_new.len(),
                };
                if changed > 0 {
                    *out.entry(col_new.type_name.clone()).or_insert(0) += changed;
                }
            }
        }
        // Rows that existed only in base (e.g. all entities of an archetype
        // despawned and the swap-removed columns shrank to zero).
        for (i, arch_base) in base.archetypes.iter().enumerate() {
            if new.archetypes.get(i).is_none() {
                for col in arch_base.columns.values() {
                    if col.len() > 0 {
                        *out.entry(col.type_name.clone()).or_insert(0) += col.len();
                    }
                }
            }
        }
        if !Arc::ptr_eq(&base.resources, &new.resources) {
            for (name, data_new) in new.resources.iter() {
                let changed = match base.resources.get(name) {
                    Some(data_base) => data_base.values != data_new.values,
                    None => true,
                };
                if changed {
                    *out.entry(name.clone()).or_insert(0) += 1;
                }
            }
            for name in base.resources.keys() {
                if !new.resources.contains_key(name) {
                    *out.entry(name.clone()).or_insert(0) += 1;
                }
            }
        }
        out
    }

    /// The set of entities whose state (components, liveness, or name) may
    /// differ between `base` and a CoW `fork` of it — computed by Arc
    /// comparison, so the cost is proportional to the **divergence**, not
    /// the world size. Returns `None` when the snapshots do not share
    /// lineage (archetype lists misaligned), in which case only a full scan
    /// can answer. Conservative: may include entities that turn out equal,
    /// never excludes one that differs.
    pub(crate) fn touched_entities(
        base: &WorldSnapshot,
        fork: &WorldSnapshot,
    ) -> Option<std::collections::BTreeSet<u32>> {
        let mut touched = std::collections::BTreeSet::new();
        let common = base.archetypes.len().min(fork.archetypes.len());
        for i in 0..common {
            let (a, b) = (&base.archetypes[i], &fork.archetypes[i]);
            if a.type_set != b.type_set {
                return None; // not the same lineage; positional pairing is meaningless
            }
            let rows_same = Arc::ptr_eq(&a.entities, &b.entities) || a.entities == b.entities;
            if !rows_same {
                // Spawns/despawns/migrations reorder rows (swap-remove);
                // flag the whole archetype pair rather than chase pairings.
                touched.extend(a.entities.iter().copied());
                touched.extend(b.entities.iter().copied());
                continue;
            }
            for (tid, col_b) in &b.columns {
                let Some(col_a) = a.columns.get(tid) else {
                    touched.extend(b.entities.iter().copied());
                    continue;
                };
                for (fa, fb) in col_a.fields.iter().zip(&col_b.fields) {
                    if Arc::ptr_eq(fa, fb) {
                        continue; // untouched column: zero work
                    }
                    let (va, vb) = (fa.as_slice(), fb.as_slice());
                    if va.len() != vb.len() {
                        touched.extend(b.entities.iter().copied());
                        break;
                    }
                    for r in 0..va.len() {
                        if va[r] != vb[r] {
                            touched.insert(b.entities[r]);
                        }
                    }
                }
            }
        }
        for arch in &fork.archetypes[common..] {
            touched.extend(arch.entities.iter().copied());
        }
        for arch in &base.archetypes[common..] {
            touched.extend(arch.entities.iter().copied());
        }
        // Renames matter too (names are semantic identity for merge).
        if !Arc::ptr_eq(&base.id_to_name, &fork.id_to_name) {
            for (eid, n) in base.id_to_name.iter() {
                if fork.id_to_name.get(eid) != Some(n) {
                    touched.insert(*eid);
                }
            }
            for (eid, n) in fork.id_to_name.iter() {
                if base.id_to_name.get(eid) != Some(n) {
                    touched.insert(*eid);
                }
            }
        }
        Some(touched)
    }

    /// Semantic diff for snapshots without shared lineage: compare per
    /// entity, per component type, structurally (field-name keyed, so layout
    /// order differences don't count). O(world), used only when the
    /// positional fast path would lie.
    fn diff_summary_by_entity(
        base: &WorldSnapshot,
        new: &WorldSnapshot,
    ) -> std::collections::BTreeMap<String, usize> {
        let mut wb = World::new();
        wb.restore(base.clone());
        let mut wn = World::new();
        wn.restore(new.clone());

        fn comp_eq(a: &ComponentData, b: &ComponentData) -> bool {
            if a.layout.len() != b.layout.len() {
                return false;
            }
            if Arc::ptr_eq(&a.layout, &b.layout) || *a.layout == *b.layout {
                return a.values == b.values;
            }
            a.layout.iter().zip(&a.values).all(|(f, v)| {
                b.layout
                    .iter()
                    .position(|n| n == f)
                    .is_some_and(|i| b.values[i] == *v)
            })
        }

        let mut out = std::collections::BTreeMap::new();
        let mut ids: std::collections::BTreeSet<u32> = wb.all_entity_ids().into_iter().collect();
        ids.extend(wn.all_entity_ids());
        for eid in ids {
            let comps_b: std::collections::BTreeMap<String, ComponentData> = wb
                .components_on_entity(eid)
                .into_iter()
                .map(|c| (c.type_name.clone(), c))
                .collect();
            let comps_n: std::collections::BTreeMap<String, ComponentData> = wn
                .components_on_entity(eid)
                .into_iter()
                .map(|c| (c.type_name.clone(), c))
                .collect();
            let mut names: std::collections::BTreeSet<&String> = comps_b.keys().collect();
            names.extend(comps_n.keys());
            for name in names {
                let same = match (comps_b.get(name), comps_n.get(name)) {
                    (Some(a), Some(b)) => comp_eq(a, b),
                    (None, None) => true,
                    _ => false,
                };
                if !same {
                    *out.entry(name.clone()).or_insert(0) += 1;
                }
            }
        }

        let mut res_names: std::collections::BTreeSet<String> =
            wb.resource_names().into_iter().collect();
        res_names.extend(wn.resource_names());
        for rname in res_names {
            let same = match (wb.get_resource(&rname), wn.get_resource(&rname)) {
                (Some(a), Some(b)) => comp_eq(&a, &b),
                (None, None) => true,
                _ => false,
            };
            if !same {
                *out.entry(rname).or_insert(0) += 1;
            }
        }
        out
    }
}
