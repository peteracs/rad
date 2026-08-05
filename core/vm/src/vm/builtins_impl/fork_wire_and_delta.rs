impl VM {

    fn decode_fork_wire(&mut self, text: &str) -> Result<crate::world::WorldSnapshot, String> {
        // Open the current RADPACK envelope (inflate + digest check) before
        // decoding the current RADFORK payload.
        let text = crate::radpack::open(text).map_err(|e| format!("fork_from_bytes: {}", e))?;
        let text: &str = &text;
        // Header: `RADFORK2 <blake3-hex> <body>` — the digest is verified
        // against the raw body bytes before any parsing.
        let rest = text
            .strip_prefix("RADFORK2 ")
            .ok_or("fork_from_bytes: not a rad-fork payload (expected RADFORK2 header)")?;
        let (claimed, body_text) = rest
            .split_once(' ')
            .ok_or("fork_from_bytes: malformed header")?;
        let actual = blake3::hash(body_text.as_bytes()).to_hex();
        if claimed != actual.as_str() {
            return Err(format!(
                "fork_from_bytes: integrity digest mismatch (claimed {}…, computed {}…) — \
                 payload corrupted or tampered",
                crate::radpack::preview(claimed, 12),
                &actual.as_str()[..12]
            ));
        }
        let body: serde_json::Value = serde_json::from_str(body_text)
            .map_err(|e| format!("fork_from_bytes: invalid JSON: {}", e))?;

        // Schema: wire layout per type, [[name, [fields]], ...].
        let mut schema: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for entry in body["schema"].as_array().into_iter().flatten() {
            let pair = entry.as_array().filter(|a| a.len() == 2);
            let (Some(pair),) = (pair,) else {
                return Err("fork_from_bytes: malformed schema entry".into());
            };
            let tname = pair[0]
                .as_str()
                .ok_or("fork_from_bytes: malformed schema entry")?;
            let fields: Vec<String> = pair[1]
                .as_array()
                .ok_or("fork_from_bytes: malformed schema entry")?
                .iter()
                .filter_map(|f| f.as_str().map(String::from))
                .collect();
            schema.insert(tname.to_string(), fields);
        }

        // Realize plan, computed once per type instead of once per instance:
        // identical field sets decode straight into declared order; drift
        // goes through the declared `migrate` block per instance.
        enum Plan {
            Direct {
                declared: std::sync::Arc<Vec<String>>,
                // declared index -> stored index
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

        // Allocator first, entities second. Every consistent world satisfies
        // next_id == live + free (every issued id is one or the other), so a
        // payload whose next_id exceeds what its own tables account for is
        // malformed — and validating that *before* inserting is what keeps a
        // hostile id (fuzzer finding: a single entity with id 2^64-1) from
        // flooding the free-list gap-fill for 60 seconds and then aborting
        // on u32 overflow.
        let allocator =
            Self::decode_validated_transport_entity_allocator(&body, "fork_from_bytes")?;

        let mut w = crate::world::World::new();
        // Seed the program's `indexed` declarations BEFORE inserting rows:
        // the bulk insert maintains indices as it goes, so a snapshot that
        // crossed a wire carries working indexes instead of wiping the live
        // world's on commit (Tier-1 finding: every RADTRACK client lost its
        // indexes the moment it pulled).
        w.share_indexed_fields_from(&self.world);

        let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for ent in body["entities"].as_array().into_iter().flatten() {
            let parts = ent
                .as_array()
                .filter(|a| a.len() == 3)
                .ok_or("fork_from_bytes: malformed entity entry")?;
            let eid_u64 = parts[0]
                .as_u64()
                .ok_or("fork_from_bytes: entity without id")?;
            let eid = u32::try_from(eid_u64).map_err(|_| {
                format!("fork_from_bytes: entity id {eid_u64} is outside the u32 range")
            })?;
            let name = parts[1].as_str();
            Self::validate_loaded_entity_name("fork_from_bytes", name, &mut seen_names)?;
            let comps_json = parts[2]
                .as_array()
                .ok_or("fork_from_bytes: malformed entity components")?;

            let mut comps: Vec<crate::value::ComponentData> = Vec::with_capacity(comps_json.len());
            for centry in comps_json {
                let cpair = centry
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("fork_from_bytes: malformed component entry")?;
                let cname = cpair[0]
                    .as_str()
                    .ok_or("fork_from_bytes: malformed component entry")?;
                let vals = cpair[1]
                    .as_array()
                    .ok_or("fork_from_bytes: malformed component values")?;

                if !plans.contains_key(cname) {
                    let stored = schema.get(cname).ok_or_else(|| {
                        format!("fork_from_bytes: no schema entry for '{}'", cname)
                    })?;
                    let declared = self.component_layouts.get(cname).cloned().ok_or_else(|| {
                        format!(
                            "fork_from_bytes: payload contains component '{}' which is \
                                 not declared in this program",
                            cname
                        )
                    })?;
                    plans.insert(cname.to_string(), make_plan(stored, declared));
                }
                let data = match plans.get(cname).unwrap() {
                    Plan::Direct { declared, map } => {
                        if vals.len() != map.len() {
                            return Err(format!(
                                "fork_from_bytes: '{}' row has {} values, schema says {}",
                                cname,
                                vals.len(),
                                map.len()
                            ));
                        }
                        // Decode into the gc heap; the bulk insert persists
                        // rows exactly once (persisting twice would leak the
                        // manually ref-counted persistent objects).
                        let mut values = Vec::with_capacity(map.len());
                        for &si in map {
                            values.push(crate::wire::decode_value(&mut self.gc, &vals[si])?);
                        }
                        let data = crate::value::ComponentData {
                            type_name: cname.to_string(),
                            layout: declared.clone(),
                            values,
                        };
                        self.validate_loaded_row("fork_from_bytes", &data)?;
                        data
                    }
                    Plan::Migrate { stored, declared } => {
                        let (stored, declared) = (stored.clone(), declared.clone());
                        self.migrate_wire_row(cname, &stored, declared, vals, 0)?
                    }
                };
                comps.push(data);
            }
            if !w.insert_entity_with_components(eid, name, comps) {
                return Err(format!("fork_from_bytes: duplicate entity id {}", eid));
            }
        }

        for rentry in body["resources"].as_array().into_iter().flatten() {
            let rpair = rentry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("fork_from_bytes: malformed resource entry")?;
            let rname = rpair[0]
                .as_str()
                .ok_or("fork_from_bytes: malformed resource entry")?;
            let vals = rpair[1]
                .as_array()
                .ok_or("fork_from_bytes: malformed resource values")?;
            let stored = schema
                .get(rname)
                .ok_or_else(|| format!("fork_from_bytes: no schema entry for '{}'", rname))?;
            let declared = self
                .world
                .get_resource(rname)
                .map(|d| d.layout)
                .ok_or_else(|| {
                    format!(
                        "fork_from_bytes: payload contains resource '{}' which is not \
                         declared in this program",
                        rname
                    )
                })?;
            let data = match make_plan(stored, declared) {
                Plan::Direct { declared, map } => {
                    if vals.len() != map.len() {
                        return Err(format!(
                            "fork_from_bytes: resource '{}' has {} values, schema says {}",
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
                    self.validate_loaded_row("fork_from_bytes", &data)?;
                    data
                }
                Plan::Migrate { stored, declared } => {
                    self.migrate_wire_row(rname, &stored, declared, vals, 0)?
                }
            };
            // set_resource persists; persisting here too would abandon a
            // full copy per ingested resource (leak-lab finding).
            w.set_resource(rname, data);
        }

        self.restore_authoritative_world_transport_with_allocator(
            &mut w,
            &body,
            "fork_from_bytes",
            allocator,
        )?;

        let mut events: Vec<(String, Value, u64)> = Vec::new();
        let mut emit_ids: Vec<u64> = Vec::new();
        for ev in body["events"].as_array().into_iter().flatten() {
            let parts = ev
                .as_array()
                .filter(|a| a.len() == 4)
                .ok_or("fork_from_bytes: malformed event entry")?;
            let name = parts[0]
                .as_str()
                .ok_or("fork_from_bytes: event without name")?
                .to_string();
            let tid = parts[1].as_u64().unwrap_or(0);
            let emit_id = parts[2].as_u64().unwrap_or(0);
            // Event payloads live in the snapshot itself: decode straight
            // into the persistent store (nothing persists them later).
            let payload = crate::wire::decode_value(&mut crate::value::PersistentStore, &parts[3])?;
            events.push((name, payload, tid));
            emit_ids.push(emit_id);
        }

        // delayed timers: optional section, absent in pre-delayed tapes
        let mut delayed: Vec<(i64, String, Value, u64)> = Vec::new();
        for ev in body["delayed"].as_array().into_iter().flatten() {
            let parts = ev
                .as_array()
                .filter(|a| a.len() == 3 || a.len() == 4)
                .ok_or("fork_from_bytes: malformed delayed entry")?;
            let left = parts[0]
                .as_i64()
                .ok_or("fork_from_bytes: delayed entry without tick count")?;
            let name = parts[1]
                .as_str()
                .ok_or("fork_from_bytes: delayed entry without name")?
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
        // The sender's provenance closure rides along; commit() ingests it
        // into the local ledger so why() can answer across the seam. The
        // origin label is the payload digest — the receiver names what it
        // can verify, not what the sender claims.
        if let Some(pj) = body.get("prov") {
            let mut prov = crate::wire::decode_prov(pj)?;
            prov.origin = format!("wire {}", crate::radpack::preview(claimed, 8));
            snap.provenance = Some(std::sync::Arc::new(prov));
        }
        Ok(snap)
    }

    /// Wire decode for a row whose schema drifted: decode the stored fields
    /// (into the gc heap — migration runs user code) and feed them through
    /// the declared `migrate` block, like `load_world` does. `from_version`
    /// is the save's declared schema version for this type (dogfood seq 69)
    /// — 0 for wire payloads and versionless saves.
    fn migrate_wire_row(
        &mut self,
        tname: &str,
        stored: &[String],
        declared: std::sync::Arc<Vec<String>>,
        vals: &[serde_json::Value],
        from_version: i64,
    ) -> Result<crate::value::ComponentData, String> {
        if vals.len() != stored.len() {
            return Err(format!(
                "fork_from_bytes: '{}' row has {} values, schema says {}",
                tname,
                vals.len(),
                stored.len()
            ));
        }
        let mut pairs = Vec::with_capacity(stored.len());
        for (f, j) in stored.iter().zip(vals) {
            pairs.push((f.clone(), crate::wire::decode_value(&mut self.gc, j)?));
        }
        self.migrate_loaded(tname, pairs, &declared, from_version)
    }

    /// `fork_delta(base, fork) -> str` — delta sync, encode half. Ships only
    /// the **divergence** of `fork` relative to `base`: upserted entities
    /// (full rows), despawns, changed resources, the in-flight queue, the
    /// allocator, the schema of shipped types, and the provenance closure
    /// **restricted to touched values** — delta sync pays double, shrinking
    /// state and history at once. The receiver reconstructs the fork with
    /// `fork_apply(its_own_base, delta)`; both sides must hold the same base
    /// (the payload carries a fingerprint, the protocol carries identity).
    pub(crate) fn bi_fork_delta(&mut self, args: Vec<Value>) -> Result<Value, String> {
        use std::fmt::Write as _;
        if args.len() != 2 {
            return Err(format!(
                "fork_delta() expects 2 arguments (base, fork), got {}",
                args.len()
            ));
        }
        let base = args[0]
            .as_world_fork()
            .cloned()
            .ok_or_else(|| "fork_delta() first argument must be a world_fork".to_string())?;
        let fork = args[1]
            .as_world_fork()
            .cloned()
            .ok_or_else(|| "fork_delta() second argument must be a world_fork".to_string())?;

        let mut wb = crate::world::World::new();
        wb.restore((*base).clone());
        let mut wf = crate::world::World::new();
        wf.restore((*fork).clone());

        let sorted_comps = |w: &crate::world::World, eid: u32| {
            let mut c = w.components_on_entity(eid);
            c.sort_by(|a, b| a.type_name.cmp(&b.type_name));
            c
        };

        // Touched set: CoW pointer walk when the snapshots share lineage
        // (O(divergence)), full semantic scan otherwise (e.g. the fork was
        // itself wire-ingested). The fast path is conservative, so every
        // candidate is re-checked by value below — false positives cost a
        // comparison, never bytes.
        let candidates: std::collections::BTreeSet<u32> =
            match crate::world::WorldSnapshot::touched_entities(&base, &fork) {
                Some(t) => t,
                None => {
                    let mut t = std::collections::BTreeSet::new();
                    t.extend(wb.all_entity_ids());
                    t.extend(wf.all_entity_ids());
                    t
                }
            };

        // An entity the base already holds travels as a surgical patch:
        // only the changed fields of the changed components (plus removed
        // component names). Full upsert rows remain for spawns, renames,
        // newly attached components, and layout drift.
        struct EntPatch {
            eid: u32,
            comps: Vec<(crate::value::ComponentData, Vec<usize>)>,
            removed: Vec<String>,
        }
        let mut despawns: Vec<u32> = Vec::new();
        let mut upserts: Vec<u32> = Vec::new();
        let mut ent_patches: Vec<EntPatch> = Vec::new();
        for &eid in &candidates {
            match (wb.contains_entity(eid), wf.contains_entity(eid)) {
                (true, false) => despawns.push(eid),
                (false, true) => upserts.push(eid),
                (true, true) => {
                    if wb.entity_name(eid) != wf.entity_name(eid) {
                        upserts.push(eid);
                        continue;
                    }
                    let bcomps = sorted_comps(&wb, eid);
                    let fcomps = sorted_comps(&wf, eid);
                    if bcomps == fcomps {
                        continue;
                    }
                    let mut patchable = true;
                    let mut comps: Vec<(crate::value::ComponentData, Vec<usize>)> = Vec::new();
                    for fc in &fcomps {
                        match bcomps.iter().find(|bc| bc.type_name == fc.type_name) {
                            // a component the base lacks: whole row needed
                            None => {
                                patchable = false;
                                break;
                            }
                            Some(bc) => {
                                if bc.layout != fc.layout || bc.values.len() != fc.values.len() {
                                    patchable = false;
                                    break;
                                }
                                let idxs: Vec<usize> = (0..fc.values.len())
                                    .filter(|&i| bc.values[i] != fc.values[i])
                                    .collect();
                                if !idxs.is_empty() {
                                    comps.push((fc.clone(), idxs));
                                }
                            }
                        }
                    }
                    if patchable {
                        let removed: Vec<String> = bcomps
                            .iter()
                            .filter(|bc| !fcomps.iter().any(|fc| fc.type_name == bc.type_name))
                            .map(|c| c.type_name.clone())
                            .collect();
                        ent_patches.push(EntPatch {
                            eid,
                            comps,
                            removed,
                        });
                    } else {
                        upserts.push(eid);
                    }
                }
                (false, false) => {}
            }
        }

        let mut changed_res: Vec<String> = Vec::new();
        {
            let mut rnames: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            rnames.extend(wb.resource_names());
            rnames.extend(wf.resource_names());
            for rname in rnames {
                match (wb.get_resource(&rname), wf.get_resource(&rname)) {
                    (Some(a), Some(b)) if a == b => {}
                    (None, None) => {}
                    (_, Some(_)) => changed_res.push(rname),
                    (_, None) => {} // resources are never removed
                }
            }
        }

        let mut schema: std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>> =
            std::collections::BTreeMap::new();
        let mut body = String::with_capacity(8 * 1024);

        // The base fingerprint: positional counters (cheap) PLUS a full
        // content digest (`bdig`). The counters alone let a resource-only
        // divergence slip through — the JS test harness caught exactly
        // that on its first run — so the digest is what actually refuses
        // out-of-order deltas. `sdig` scopes the digest to a schema
        // vintage: across a rolling migration the receiver's migrated
        // base legitimately hashes differently, so the content check
        // only binds same-schema peers.
        let bdig = Self::fork_digest(&base, &self.transient_resources)?;
        let sdig = self.schema_digest_value();
        let _ = write!(
            body,
            "{{\"check\":[{},{},{}],\"bdig\":\"{}\",\"sdig\":\"{}\",\"despawns\":[",
            base.next_id,
            base.entity_archetype.len(),
            base.events.len(),
            bdig,
            sdig
        );
        for (i, eid) in despawns.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "{}", eid);
        }

        body.push_str("],\"upserts\":[");
        for (i, &eid) in upserts.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "[{},", eid);
            match wf.entity_name(eid) {
                Some(name) => crate::wire::escape_json_into(&mut body, &name),
                None => body.push_str("null"),
            }
            body.push_str(",[");
            for (j, data) in sorted_comps(&wf, eid).iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                body.push('[');
                crate::wire::write_row_into(&mut schema, data, &mut body)
                    .map_err(|e| format!("fork_delta: {}", e))?;
                body.push(']');
            }
            body.push_str("]]");
        }

        // Surgical entity patches: [eid, [[comp, [[field, value]…]]…], [removed…]]
        // Patched components register in the schema table so the receiver can
        // detect shape drift and re-run its `migrate` block on patched rows.
        body.push_str("],\"ent_patch\":[");
        for (i, p) in ent_patches.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "[{},[", p.eid);
            for (j, (data, idxs)) in p.comps.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                schema.insert(data.type_name.clone(), data.layout.clone());
                body.push('[');
                crate::wire::escape_json_into(&mut body, &data.type_name);
                body.push_str(",[");
                for (k, &fi) in idxs.iter().enumerate() {
                    if k > 0 {
                        body.push(',');
                    }
                    body.push('[');
                    crate::wire::escape_json_into(&mut body, &data.layout[fi]);
                    body.push(',');
                    crate::wire::encode_value_into(&data.values[fi], &mut body)
                        .map_err(|e| format!("fork_delta: {}", e))?;
                    body.push(']');
                }
                body.push_str("]]");
            }
            body.push_str("],[");
            for (j, rname) in p.removed.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                crate::wire::escape_json_into(&mut body, rname);
            }
            body.push_str("]]");
        }

        // The in-flight queue ships whole (small, and append-only relative
        // to base); emit ids cross into the foreign namespace like the full
        // codec's.
        body.push_str("],\"events\":[");
        for (i, (name, payload, tid)) in fork.events.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, name);
            let _ = write!(
                body,
                ",{},{},",
                tid,
                crate::causality::foreign_emit_id(fork.emit_ids.get(i).copied().unwrap_or(0))
            );
            crate::wire::encode_value_into(payload, &mut body)
                .map_err(|e| format!("fork_delta: {}", e))?;
            body.push(']');
        }

        body.push_str("],\"delayed\":[");
        for (i, (left, name, payload, emit_id)) in fork.delayed.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            let _ = write!(body, "{},", left);
            crate::wire::escape_json_into(&mut body, name);
            let _ = write!(body, ",{},", crate::causality::foreign_emit_id(*emit_id));
            crate::wire::encode_value_into(payload, &mut body)
                .map_err(|e| format!("fork_delta: {}", e))?;
            body.push(']');
        }

        body.push_str("],\"resources\":[");

        // Changed resources travel as per-field patches when the base holds
        // the same layout: a 40-round battle journal must not re-ship its
        // whole log string because `round` ticked. Whole rows remain the
        // fallback for resources the base lacks (or whose layout differs).
        let mut patch_res: Vec<(&str, crate::value::ComponentData, Vec<usize>)> = Vec::new();
        let mut whole_res: Vec<&str> = Vec::new();
        for rname in &changed_res {
            match (wb.get_resource(rname), wf.get_resource(rname)) {
                (Some(a), Some(b)) if a.layout == b.layout && a.values.len() == b.values.len() => {
                    let idxs: Vec<usize> = (0..b.values.len())
                        .filter(|&i| a.values[i] != b.values[i])
                        .collect();
                    patch_res.push((rname.as_str(), b, idxs));
                }
                _ => whole_res.push(rname.as_str()),
            }
        }

        for (i, rname) in whole_res.iter().enumerate() {
            if let Some(data) = wf.get_resource(rname) {
                if i > 0 {
                    body.push(',');
                }
                body.push('[');
                crate::wire::write_row_into(&mut schema, &data, &mut body)
                    .map_err(|e| format!("fork_delta: {}", e))?;
                body.push(']');
            }
        }

        body.push_str("],\"res_patch\":[");
        for (i, (rname, data, idxs)) in patch_res.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, rname);
            body.push_str(",[");
            for (j, &fi) in idxs.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                body.push('[');
                crate::wire::escape_json_into(&mut body, &data.layout[fi]);
                body.push(',');
                crate::wire::encode_value_into(&data.values[fi], &mut body)
                    .map_err(|e| format!("fork_delta: {}", e))?;
                body.push(']');
            }
            body.push_str("]]");
        }

        body.push_str("],\"schema\":[");
        for (i, (tname, layout)) in schema.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, tname);
            body.push_str(",[");
            for (j, f) in layout.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                crate::wire::escape_json_into(&mut body, f);
            }
            body.push_str("]]");
        }

        // Provenance restricted to the divergence: the receiver already
        // holds the base's history (it ingested the base), so only records
        // for touched values need to travel.
        body.push(']');
        Self::append_authoritative_world_transport(&wf, &mut body)?;
        body.push_str(",\"prov\":");
        {
            let keep_ids: std::collections::HashSet<u32> = despawns
                .iter()
                .chain(upserts.iter())
                .chain(ent_patches.iter().map(|p| &p.eid))
                .copied()
                .collect();
            let keep_res: std::collections::HashSet<&str> =
                changed_res.iter().map(|s| s.as_str()).collect();
            let prov = match fork.provenance.as_deref() {
                Some(p) => {
                    // A wire-ingested fork carries its records; filter to
                    // the divergence, keep the emit chain whole (it is
                    // already a bounded closure).
                    let mut filtered = p.clone();
                    filtered.writes.retain(|w| match w.entity {
                        Some(e) => keep_ids.contains(&e),
                        None => keep_res.contains(w.component.as_str()),
                    });
                    filtered
                }
                None => self.ledger.provenance_closure(
                    |w| match w.entity {
                        Some(e) => keep_ids.contains(&e),
                        None => keep_res.contains(w.component.as_str()),
                    },
                    &fork
                        .emit_ids
                        .iter()
                        .copied()
                        .chain(fork.delayed.iter().map(|(_, _, _, id)| *id))
                        .collect::<Vec<_>>(),
                ),
            };
            crate::wire::encode_prov_into(&prov, &mut body);
        }
        body.push('}');

        let out = crate::radpack::seal("RADDELTA1", &body);
        Ok(Value::from_string(&mut self.gc, out))
    }

    /// `fork_apply(base, delta) -> Result<world_fork, str>` — delta sync,
    /// apply half. Rebuilds the sender's fork on top of the receiver's copy
    /// of the same base: CoW restore + only the shipped divergence, so the
    /// result **shares lineage with the local base** — the O(divergence)
    /// merge fast path works on wire-delivered forks. Shipped rows migrate
    /// on schema drift exactly like the full codec; corruption and
    /// wrong-base application are an `Err`, not a crash.
    pub(crate) fn bi_fork_apply(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "fork_apply() expects 2 arguments (base, delta), got {}",
                args.len()
            ));
        }
        let base = args[0]
            .as_world_fork()
            .cloned()
            .ok_or_else(|| "fork_apply() first argument must be a world_fork".to_string())?;
        let text = args[1]
            .as_str()
            .ok_or_else(|| format!("fork_apply() expects str, got {}", args[1].type_name()))?
            .to_string();

        match self.apply_fork_delta(&base, &text) {
            Ok(snap) => {
                let v = Value::world_fork(&mut self.gc, std::sync::Arc::new(snap));
                Ok(self.make_result(true, v))
            }
            Err(msg) => {
                let e = Value::from_string(&mut self.gc, msg);
                Ok(self.make_result(false, e))
            }
        }
    }}
