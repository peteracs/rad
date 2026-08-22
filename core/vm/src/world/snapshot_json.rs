// Human- and tool-facing JSON renderings of world state: whole-world dumps
// and the incremental render deltas the time-travel server streams.

impl WorldSnapshot {

    pub(crate) fn entity_ref(&self, entity: u32) -> Option<crate::relation::runtime::EntityRef> {
        self.entity_archetype
            .contains_key(&entity)
            .then(|| crate::relation::runtime::EntityRef {
                slot: entity,
                generation: self.generations.get(&entity).copied().unwrap_or(0),
            })
    }

    /// Return a copy of this snapshot with resource `name` set to `data`,
    /// leaving entities, in-flight events, delayed timers, and provenance
    /// untouched. Backs `fork_with`: seed a speculative candidate off a fork
    /// without committing to (mutating) the live world (dogfood feature seq
    /// 150). Copy-on-write — only the resource map's `Arc` is cloned.
    pub(crate) fn with_resource(&self, name: &str, mut data: ComponentData) -> WorldSnapshot {
        Value::persist_component_data(&mut data);
        let mut snap = self.clone();
        Arc::make_mut(&mut snap.resources).insert_owned(name.to_string(), data);
        // The override makes this a fresh candidate, not the output of the
        // rollout that (possibly) produced `self` — `fork_seed()` on it is 0.
        snap.rollout_seed = None;
        snap
    }

    /// Renderer/inspector dump of a frozen frame — same shape as
    /// `World::snapshot_json_like`, so RADSCOPE scrubs timelines with the
    /// exact code that renders live worlds.
    pub fn snapshot_json_like(&self) -> String {
        let mut ids: Vec<u32> = self.entity_archetype.keys().copied().collect();
        ids.sort_unstable();
        let mut res_names: Vec<String> = self.resources.keys().cloned().collect();
        res_names.sort_unstable();
        dump_world_json(
            &ids,
            |eid| self.id_to_name.get(&eid).cloned(),
            |eid| {
                let Some(&aid) = self.entity_archetype.get(&eid) else {
                    return Vec::new();
                };
                let arch = &self.archetypes[aid as usize];
                let Some(&row) = arch.entity_row.get(&eid) else {
                    return Vec::new();
                };
                let mut out = Vec::new();
                for &tid in &arch.type_set {
                    if let Some(col) = arch.columns.get(&tid) {
                        out.push(col.get(row));
                    }
                }
                out
            },
            &res_names,
            |name| self.resources.get(name).cloned(),
        )
    }

    /// Renderer-facing INCREMENTAL dump: what changed since `prev`.
    /// `{"upsert":[entity rows],"remove":[ids],"resources":{changed only}}`
    /// — the fix for the full-world-JSON-per-keystroke firehose.
    ///
    /// Change detection rides the CoW structure: archetypes whose entity
    /// list and column-field Arcs are pointer-equal are skipped whole (the
    /// overwhelmingly common case); only rows in actually-written columns
    /// get value compares; only changed entities get serialized. No
    /// per-entity ComponentData clones on the compare path.
    pub fn render_delta_json(&self, prev: &WorldSnapshot) -> String {
        use std::fmt::Write;

        fn comp_eq(a: &ComponentData, b: &ComponentData) -> bool {
            if a.type_name != b.type_name || a.values.len() != b.values.len() {
                return false;
            }
            if !Arc::ptr_eq(&a.layout, &b.layout) && a.layout != b.layout {
                return false;
            }
            a.values.iter().zip(b.values.iter()).all(|(x, y)| x == y)
        }

        let mut upserts: Vec<u32> = Vec::new();

        // positional alignment holds for shared-lineage snapshots (these
        // are successive forks of one live world); if it ever doesn't,
        // fall back to treating every current entity as changed-checkable
        // via the slow row lookup below.
        for (i, arch) in self.archetypes.iter().enumerate() {
            let base = prev.archetypes.get(i);
            let Some(base) = base else {
                // brand-new archetype: every entity in it is an upsert
                upserts.extend(arch.entities.iter().copied());
                continue;
            };
            // O(1) skip: same rows, same column data
            let identical = Arc::ptr_eq(&arch.entities, &base.entities)
                && arch.columns.len() == base.columns.len()
                && arch.columns.iter().all(|(tid, col)| {
                    base.columns.get(tid).is_some_and(|bcol| {
                        col.fields.len() == bcol.fields.len()
                            && col
                                .fields
                                .iter()
                                .zip(&bcol.fields)
                                .all(|(a, b)| Arc::ptr_eq(a, b))
                    })
                });
            if identical {
                continue;
            }
            // something in this archetype was written: row-level check
            for (r, &eid) in arch.entities.iter().enumerate() {
                let Some(&rb) = base.entity_row.get(&eid) else {
                    upserts.push(eid); // entered this archetype since prev
                    continue;
                };
                let mut changed = false;
                for (tid, col) in &arch.columns {
                    let Some(bcol) = base.columns.get(tid) else {
                        changed = true;
                        break;
                    };
                    for (fa, fb) in col.fields.iter().zip(&bcol.fields) {
                        if Arc::ptr_eq(fa, fb) && r == rb {
                            continue;
                        }
                        match (fa.as_slice().get(r), fb.as_slice().get(rb)) {
                            (Some(a), Some(b)) => {
                                if a != b {
                                    changed = true;
                                    break;
                                }
                            }
                            _ => {
                                changed = true;
                                break;
                            }
                        }
                    }
                    if changed {
                        break;
                    }
                }
                if changed {
                    upserts.push(eid);
                }
            }
        }
        // entities whose archetype VANISHED (despawn shrank the vec) are
        // covered by `remove` below; entities that MOVED archetypes were
        // caught by the entity_row miss in their new home.
        upserts.sort_unstable();
        upserts.dedup();

        let mut removed: Vec<u32> = prev
            .entity_archetype
            .keys()
            .filter(|eid| !self.entity_archetype.contains_key(eid))
            .copied()
            .collect();
        removed.sort_unstable();

        let mut res_names: Vec<String> = self.resources.keys().cloned().collect();
        res_names.sort_unstable();

        // reuse the entity-row encoding from the full dump
        let upsert_json = dump_world_json(
            &upserts,
            |eid| self.id_to_name.get(&eid).cloned(),
            |eid| self.components_of(eid),
            &[],
            |_| None,
        );
        // dump_world_json returns {"entities":[...],"resources":{}} — splice
        let entities_part = upsert_json
            .strip_prefix("{\"entities\":")
            .and_then(|s| s.strip_suffix(",\"resources\":{}}"))
            .unwrap_or("[]")
            .to_string();

        let mut s = String::from("{\"upsert\":");
        s.push_str(&entities_part);
        s.push_str(",\"remove\":[");
        for (i, eid) in removed.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(&mut s, "{}", eid);
        }
        s.push_str("],\"resources\":{");
        let mut first = true;
        for name in &res_names {
            let (Some(cur), prevr) = (self.resources.get(name), prev.resources.get(name)) else {
                continue;
            };
            let changed = match prevr {
                Some(p) => !comp_eq(cur, p),
                None => true,
            };
            if changed {
                if !first {
                    s.push(',');
                }
                first = false;
                let _ = write!(&mut s, "\"{}\":{}", name, resource_fields_json(cur));
            }
        }
        s.push_str("}}");
        s
    }
}
