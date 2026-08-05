struct ValidatedForkDeltaEntityPlan {
    allocator: crate::world::ValidatedEntityAllocatorState,
    despawns: Vec<u32>,
    upserts: Vec<u32>,
    final_live_ids: Vec<u32>,
}

impl VM {
    fn decode_fork_delta_entity_plan(
        base: &crate::world::WorldSnapshot,
        body: &serde_json::Value,
    ) -> Result<ValidatedForkDeltaEntityPlan, String> {
        fn ordered_id(
            value: &serde_json::Value,
            previous: &mut Option<u32>,
            kind: &str,
        ) -> Result<u32, String> {
            let id = value
                .as_u64()
                .and_then(|number| u32::try_from(number).ok())
                .ok_or_else(|| format!("fork_apply: {kind} entity ID out of range"))?;
            if let Some(previous) = *previous {
                if id == previous {
                    return Err(format!("fork_apply: duplicate {kind} entity ID {id}"));
                }
                if id < previous {
                    return Err(format!(
                        "fork_apply: {kind} entity IDs are not strictly ascending"
                    ));
                }
            }
            *previous = Some(id);
            Ok(id)
        }

        let mut final_live = base
            .sorted_entity_ids()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let despawn_rows = body
            .get("despawns")
            .and_then(serde_json::Value::as_array)
            .ok_or("fork_apply: malformed or missing despawn list")?;
        let mut despawns = Vec::with_capacity(despawn_rows.len());
        let mut previous = None;
        for value in despawn_rows {
            let id = ordered_id(value, &mut previous, "despawn")?;
            if !final_live.remove(&id) {
                return Err(format!(
                    "fork_apply: delta despawns entity {id} which the base does not have"
                ));
            }
            despawns.push(id);
        }

        let upsert_rows = body
            .get("upserts")
            .and_then(serde_json::Value::as_array)
            .ok_or("fork_apply: malformed or missing upsert list")?;
        let despawn_set = despawns
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let mut upserts = Vec::with_capacity(upsert_rows.len());
        let mut previous = None;
        for value in upsert_rows {
            let row = value
                .as_array()
                .filter(|row| row.len() == 3)
                .ok_or("fork_apply: malformed upsert entry")?;
            let id = ordered_id(&row[0], &mut previous, "upsert")?;
            if despawn_set.contains(&id) {
                return Err(format!(
                    "fork_apply: entity {id} cannot be both despawned and upserted"
                ));
            }
            final_live.insert(id);
            upserts.push(id);
        }

        let final_live_ids = final_live.into_iter().collect::<Vec<_>>();
        let allocator = Self::decode_transport_entity_allocator(body, "fork_apply")?
            .validate(&final_live_ids, "fork_apply")?;
        Ok(ValidatedForkDeltaEntityPlan {
            allocator,
            despawns,
            upserts,
            final_live_ids,
        })
    }

    fn apply_fork_delta(
        &mut self,
        base: &std::sync::Arc<crate::world::WorldSnapshot>,
        text: &str,
    ) -> Result<crate::world::WorldSnapshot, String> {
        let text = crate::radpack::open(text).map_err(|e| format!("fork_apply: {}", e))?;
        let text: &str = &text;
        let rest = text
            .strip_prefix("RADDELTA1 ")
            .ok_or("fork_apply: not a rad-delta payload (expected RADDELTA1 header)")?;
        let (claimed, body_text) = rest.split_once(' ').ok_or("fork_apply: malformed header")?;
        let actual = blake3::hash(body_text.as_bytes()).to_hex();
        if claimed != actual.as_str() {
            return Err(format!(
                "fork_apply: integrity digest mismatch (claimed {}…, computed {}…) — \
                 payload corrupted or tampered",
                crate::radpack::preview(claimed, 12),
                &actual.as_str()[..12]
            ));
        }
        let body: serde_json::Value = serde_json::from_str(body_text)
            .map_err(|e| format!("fork_apply: invalid JSON: {}", e))?;

        // Base fingerprint: a delta describes a divergence *from somewhere*;
        // applying it elsewhere would silently fabricate a world.
        let check = body["check"]
            .as_array()
            .filter(|a| a.len() == 3)
            .ok_or("fork_apply: malformed check section")?;
        let (cn, ce, cv) = (
            check[0].as_u64().unwrap_or(u64::MAX),
            check[1].as_u64().unwrap_or(u64::MAX),
            check[2].as_u64().unwrap_or(u64::MAX),
        );
        if cn != base.next_id as u64
            || ce != base.entity_archetype.len() as u64
            || cv != base.events.len() as u64
        {
            return Err(format!(
                "fork_apply: delta was made against a different base \
                 (expected allocator {} / {} entities / {} pending events, \
                  local base has {} / {} / {})",
                cn,
                ce,
                cv,
                base.next_id,
                base.entity_archetype.len(),
                base.events.len()
            ));
        }
        // Content digest: catches divergences the counters can't see
        // (resource changes, in-place component writes). Across a rolling
        // migration the receiver's migrated base hashes differently by
        // design, so the base digest binds only same-schema peers. Both
        // identity fields are mandatory in every current delta.
        let sender_schema = body["sdig"]
            .as_str()
            .ok_or("fork_apply: missing schema digest")?;
        let claimed_bdig = body["bdig"]
            .as_str()
            .ok_or("fork_apply: missing base digest")?;
        if sender_schema == self.schema_digest_value() {
            let local_bdig = Self::fork_digest(base, &self.transient_resources)?;
            if claimed_bdig != local_bdig {
                return Err(format!(
                    "fork_apply: delta was made against a different base \
                     (base digest {}… != local {}…) — apply deltas in order",
                    crate::radpack::preview(claimed_bdig, 12),
                    &local_bdig[..12]
                ));
            }
        }

        // Validate the complete final allocator partition before reconstructing
        // any row. Final upserts cannot price identity history: a fork may
        // allocate and destroy an entity without retaining an upsert. The
        // transported live/free/retired partition is the exact authority.
        let entity_plan = Self::decode_fork_delta_entity_plan(base, &body)?;

        // Schema of shipped types only.
        let mut schema: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for entry in body["schema"].as_array().into_iter().flatten() {
            let pair = entry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("fork_apply: malformed schema entry")?;
            let tname = pair[0]
                .as_str()
                .ok_or("fork_apply: malformed schema entry")?;
            let fields: Vec<String> = pair[1]
                .as_array()
                .ok_or("fork_apply: malformed schema entry")?
                .iter()
                .filter_map(|f| f.as_str().map(String::from))
                .collect();
            schema.insert(tname.to_string(), fields);
        }

        enum Plan {
            Direct {
                declared: std::sync::Arc<Vec<String>>,
                map: Vec<usize>,
            },
            Migrate {
                stored: Vec<String>,
                declared: std::sync::Arc<Vec<String>>,
            },
        }
        let make_plan = |stored: &[String], declared: std::sync::Arc<Vec<String>>| -> Plan {
            if stored.len() == declared.len() {
                let mut map = Vec::with_capacity(declared.len());
                for f in declared.iter() {
                    match stored.iter().position(|s| s == f) {
                        Some(i) => map.push(i),
                        None => {
                            return Plan::Migrate {
                                stored: stored.to_vec(),
                                declared,
                            }
                        }
                    }
                }
                Plan::Direct { declared, map }
            } else {
                Plan::Migrate {
                    stored: stored.to_vec(),
                    declared,
                }
            }
        };
        let mut plans: std::collections::HashMap<String, Plan> = std::collections::HashMap::new();

        // CoW restore of the local base: untouched columns stay shared with
        // it, which is what keeps the later merge O(divergence).
        let mut w = crate::world::World::new();
        w.restore((**base).clone());

        for &eid in &entity_plan.despawns {
            if !w.destroy_entity_storage(eid) {
                return Err(format!(
                    "fork_apply: failed to remove entity {eid} from the candidate"
                ));
            }
        }

        for (ent, &eid) in body["upserts"]
            .as_array()
            .into_iter()
            .flatten()
            .zip(&entity_plan.upserts)
        {
            let parts = ent
                .as_array()
                .filter(|a| a.len() == 3)
                .ok_or("fork_apply: malformed upsert entry")?;
            let name = parts[1].as_str();
            let comps_json = parts[2]
                .as_array()
                .ok_or("fork_apply: malformed upsert components")?;

            let mut comps: Vec<crate::value::ComponentData> = Vec::with_capacity(comps_json.len());
            for centry in comps_json {
                let cpair = centry
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("fork_apply: malformed component entry")?;
                let cname = cpair[0]
                    .as_str()
                    .ok_or("fork_apply: malformed component entry")?;
                let vals = cpair[1]
                    .as_array()
                    .ok_or("fork_apply: malformed component values")?;

                if !plans.contains_key(cname) {
                    let stored = schema
                        .get(cname)
                        .ok_or_else(|| format!("fork_apply: no schema entry for '{}'", cname))?;
                    let declared = self.component_layouts.get(cname).cloned().ok_or_else(|| {
                        format!(
                            "fork_apply: delta contains component '{}' which is not \
                                 declared in this program",
                            cname
                        )
                    })?;
                    plans.insert(cname.to_string(), make_plan(stored, declared));
                }
                let data = match plans.get(cname).unwrap() {
                    Plan::Direct { declared, map } => {
                        if vals.len() != map.len() {
                            return Err(format!(
                                "fork_apply: '{}' row has {} values, schema says {}",
                                cname,
                                vals.len(),
                                map.len()
                            ));
                        }
                        let mut values = Vec::with_capacity(map.len());
                        for &si in map {
                            values.push(crate::wire::decode_value(&mut self.gc, &vals[si])?);
                        }
                        let data = crate::value::ComponentData {
                            type_name: cname.to_string(),
                            layout: declared.clone(),
                            values,
                        };
                        self.validate_loaded_row("fork_apply", &data)?;
                        data
                    }
                    Plan::Migrate { stored, declared } => {
                        let (stored, declared) = (stored.clone(), declared.clone());
                        self.migrate_wire_row(cname, &stored, declared, vals, 0)?
                    }
                };
                comps.push(data);
            }

            if w.contains_entity(eid) {
                // Surgical update against the base row: only differing
                // components move, so untouched columns stay CoW-shared.
                if w.entity_name(eid).as_deref() != name {
                    w.set_entity_name(eid, name);
                }
                let existing = w.components_on_entity(eid);
                let new_names: std::collections::HashSet<&str> =
                    comps.iter().map(|c| c.type_name.as_str()).collect();
                for old in &existing {
                    if !new_names.contains(old.type_name.as_str()) {
                        w.remove_component(eid, &old.type_name);
                    }
                }
                for data in comps {
                    let unchanged = existing
                        .iter()
                        .find(|c| c.type_name == data.type_name)
                        .is_some_and(|c| *c == data);
                    if !unchanged {
                        w.add_component(eid, data);
                    }
                }
            } else if let Err(error) = w.restore_entity_with_components(eid, name, comps) {
                return Err(format!("fork_apply: {error}"));
            }
        }

        // Surgical entity patches: change only the named fields of the named
        // components on rows the base already holds. Addressed by field name,
        // so only the receiver's declared layout matters.
        for pentry in body["ent_patch"].as_array().into_iter().flatten() {
            let parts = pentry
                .as_array()
                .filter(|a| a.len() == 3)
                .ok_or("fork_apply: malformed entity patch entry")?;
            // try_from, not `as`: a truncating cast would silently patch
            // whatever entity the low 32 bits happen to name.
            let eid = parts[0]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("fork_apply: entity patch without id")?;
            if !w.contains_entity(eid) {
                return Err(format!(
                    "fork_apply: delta patches entity {} which the base does not have",
                    eid
                ));
            }
            let comps = parts[1]
                .as_array()
                .ok_or("fork_apply: malformed entity patch components")?;
            let removed = parts[2]
                .as_array()
                .ok_or("fork_apply: malformed entity patch removals")?;

            let existing = w.components_on_entity(eid);
            for centry in comps {
                let cpair = centry
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("fork_apply: malformed entity patch component")?;
                let cname = cpair[0]
                    .as_str()
                    .ok_or("fork_apply: malformed entity patch component")?;
                let fields = cpair[1]
                    .as_array()
                    .ok_or("fork_apply: malformed entity patch fields")?;
                let mut row = existing
                    .iter()
                    .find(|c| c.type_name == cname)
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "fork_apply: delta patches component '{}' on entity {} \
                             which the base row does not carry",
                            cname, eid
                        )
                    })?;
                for f in fields {
                    let fp = f
                        .as_array()
                        .filter(|a| a.len() == 2)
                        .ok_or("fork_apply: malformed entity patch field")?;
                    let fname = fp[0]
                        .as_str()
                        .ok_or("fork_apply: malformed entity patch field")?;
                    let pos = row.layout.iter().position(|l| l == fname).ok_or_else(|| {
                        format!(
                            "fork_apply: component '{}' has no field '{}' on this \
                             machine — a delta can only patch fields that survive \
                             the receiver's migration",
                            cname, fname
                        )
                    })?;
                    row.values[pos] = crate::wire::decode_value(&mut self.gc, &fp[1])?;
                }
                // Shape drift: the sender wrote this row under a different
                // field set than ours. The patched row re-enters through the
                // declared `migrate` block, exactly like a shipped whole row
                // would — derived fields (e.g. shield = hp/2) stay coherent.
                let drifted = schema.get(cname).is_some_and(|sender| {
                    let a: std::collections::HashSet<&str> =
                        sender.iter().map(|s| s.as_str()).collect();
                    let b: std::collections::HashSet<&str> =
                        row.layout.iter().map(|s| s.as_str()).collect();
                    a != b
                });
                if drifted {
                    let declared = row.layout.clone();
                    let pairs: Vec<(String, Value)> = declared
                        .iter()
                        .cloned()
                        .zip(row.values.iter().cloned())
                        .collect();
                    // Patch payloads carry no schema versions: 0.
                    row = self.migrate_loaded(cname, pairs, &declared, 0)?;
                }
                self.validate_loaded_row("fork_apply", &row)?;
                w.add_component(eid, row);
            }
            for r in removed {
                let rname = r
                    .as_str()
                    .ok_or("fork_apply: malformed entity patch removal")?;
                w.remove_component(eid, rname);
            }
        }

        for rentry in body["resources"].as_array().into_iter().flatten() {
            let rpair = rentry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("fork_apply: malformed resource entry")?;
            let rname = rpair[0]
                .as_str()
                .ok_or("fork_apply: malformed resource entry")?;
            let vals = rpair[1]
                .as_array()
                .ok_or("fork_apply: malformed resource values")?;
            let stored = schema
                .get(rname)
                .ok_or_else(|| format!("fork_apply: no schema entry for '{}'", rname))?;
            let declared = self
                .world
                .get_resource(rname)
                .map(|d| d.layout)
                .ok_or_else(|| {
                    format!(
                        "fork_apply: delta contains resource '{}' which is not declared \
                         in this program",
                        rname
                    )
                })?;
            let data = match make_plan(stored, declared) {
                Plan::Direct { declared, map } => {
                    if vals.len() != map.len() {
                        return Err(format!(
                            "fork_apply: resource '{}' has {} values, schema says {}",
                            rname,
                            vals.len(),
                            map.len()
                        ));
                    }
                    let mut values = Vec::with_capacity(map.len());
                    for si in map {
                        values.push(crate::wire::decode_value(&mut self.gc, &vals[si])?);
                    }
                    let data = crate::value::ComponentData {
                        type_name: rname.to_string(),
                        layout: declared,
                        values,
                    };
                    self.validate_loaded_row("fork_apply", &data)?;
                    data
                }
                Plan::Migrate { stored, declared } => {
                    self.migrate_wire_row(rname, &stored, declared, vals, 0)?
                }
            };
            w.set_resource(rname, data);
        }

        // Per-field resource patches: surgical edits against the base row,
        // addressed by field name so the receiver's declared layout is the
        // only one that matters.
        for pentry in body["res_patch"].as_array().into_iter().flatten() {
            let pair = pentry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("fork_apply: malformed resource patch entry")?;
            let rname = pair[0]
                .as_str()
                .ok_or("fork_apply: malformed resource patch entry")?;
            let fields = pair[1]
                .as_array()
                .ok_or("fork_apply: malformed resource patch fields")?;
            let mut row = w.get_resource(rname).ok_or_else(|| {
                format!(
                    "fork_apply: delta patches resource '{}' which the base does not have",
                    rname
                )
            })?;
            for f in fields {
                let fp = f
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("fork_apply: malformed resource patch field")?;
                let fname = fp[0]
                    .as_str()
                    .ok_or("fork_apply: malformed resource patch field")?;
                let pos = row.layout.iter().position(|l| l == fname).ok_or_else(|| {
                    format!(
                        "fork_apply: resource '{}' has no field '{}' \
                         (schema drift across a delta session is not supported)",
                        rname, fname
                    )
                })?;
                row.values[pos] = crate::wire::decode_value(&mut self.gc, &fp[1])?;
            }
            self.validate_loaded_row("fork_apply", &row)?;
            w.set_resource(rname, row);
        }

        if w.all_entity_ids() != entity_plan.final_live_ids {
            return Err(
                "fork_apply: reconstructed entity set differs from validated plan".to_string(),
            );
        }
        self.restore_authoritative_world_transport_with_allocator(
            &mut w,
            &body,
            "fork_apply",
            entity_plan.allocator,
        )?;

        let mut events: Vec<(String, Value, u64)> = Vec::new();
        let mut emit_ids: Vec<u64> = Vec::new();
        for ev in body["events"].as_array().into_iter().flatten() {
            let parts = ev
                .as_array()
                .filter(|a| a.len() == 4)
                .ok_or("fork_apply: malformed event entry")?;
            let name = parts[0]
                .as_str()
                .ok_or("fork_apply: event without name")?
                .to_string();
            let tid = parts[1].as_u64().unwrap_or(0);
            let emit_id = parts[2].as_u64().unwrap_or(0);
            let payload = crate::wire::decode_value(&mut crate::value::PersistentStore, &parts[3])?;
            events.push((name, payload, tid));
            emit_ids.push(emit_id);
        }

        let mut delayed: Vec<(i64, String, Value, u64)> = Vec::new();
        for ev in body["delayed"].as_array().into_iter().flatten() {
            let parts = ev
                .as_array()
                .filter(|a| a.len() == 3 || a.len() == 4)
                .ok_or("fork_apply: malformed delayed entry")?;
            let left = parts[0]
                .as_i64()
                .ok_or("fork_apply: delayed entry without tick count")?;
            let name = parts[1]
                .as_str()
                .ok_or("fork_apply: delayed entry without name")?
                .to_string();
            let (emit_id, payload_idx) = if parts.len() == 4 {
                (parts[2].as_u64().unwrap_or(0), 3)
            } else {
                (0, 2)
            };
            let payload =
                crate::wire::decode_value(&mut crate::value::PersistentStore, &parts[payload_idx])?;
            delayed.push((left, name, payload, emit_id));
        }

        let mut snap = w.snapshot();
        snap.events = std::sync::Arc::new(events);
        snap.emit_ids = std::sync::Arc::new(emit_ids);
        snap.delayed = std::sync::Arc::new(delayed);
        if let Some(pj) = body.get("prov") {
            let mut prov = crate::wire::decode_prov(pj)?;
            prov.origin = format!("wire {}", crate::radpack::preview(claimed, 8));
            snap.provenance = Some(std::sync::Arc::new(prov));
        }
        Ok(snap)
    }

}
