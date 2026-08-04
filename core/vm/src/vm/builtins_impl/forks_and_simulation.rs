impl VM {

    fn bi_bytebuf_get_u32_or_i32_le(
        &mut self,
        args: Vec<Value>,
        signed: bool,
        fn_name: &str,
    ) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "{} expects 2 arguments, got {}",
                fn_name,
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| format!("{} expects a bytebuf", fn_name))?;
        let offset = bytebuf_index_arg(&args[1], &format!("{} offset", fn_name))?;
        let value = bytebuf_read_u32_le(bytes, offset, fn_name)?;
        let result = if signed {
            i64::from(value as i32)
        } else {
            i64::from(value)
        };
        Ok(Value::from_int(&mut self.gc, result))
    }

    fn bi_bytebuf_to_list(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_to_list() expects 1 argument, got {}",
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "bytebuf_to_list() expects a bytebuf".to_string())?;
        let mut values = Vec::with_capacity(bytes.len());
        for byte in bytes {
            values.push(Value::from_int(&mut self.gc, i64::from(*byte)));
        }
        Ok(Value::list(&mut self.gc, values))
    }

    fn bi_bytebuf_from_list(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_from_list() expects 1 argument, got {}",
                args.len()
            ));
        }
        let bytes = bytes_from_list_arg(&args[0], "bytebuf_from_list()")?;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }

    fn bi_fork(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!("fork() takes no arguments, got {}", args.len()));
        }
        // Full program state: world + in-flight events. A fork that drops
        // pending events is not a fork (composition pass, #7).
        let snapshot = self.snapshot_with_events();
        Ok(Value::world_fork(
            &mut self.gc,
            std::sync::Arc::new(snapshot),
        ))
    }

    fn bi_simulate(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "simulate() expects 3 arguments, got {}",
                args.len()
            ));
        }
        let fork_snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "simulate() first argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        let system_names: Vec<String> = {
            let list = args[1].as_list().ok_or_else(|| {
                "simulate() second argument must be a list of systems".to_string()
            })?;
            list.iter()
                .map(|v| {
                    v.as_system_ref().map(|s| s.to_string()).ok_or_else(|| {
                        format!(
                            "simulate() schedule must be a list of `system` values, got {}",
                            v.type_name()
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let ticks = args[2]
            .as_int()
            .ok_or_else(|| "simulate() third argument must be an integer".to_string())?;
        if ticks < 0 {
            return Err("simulate() tick count must be non-negative".to_string());
        }

        let saved_world = std::mem::take(self.get_world_mut());
        let saved_events_current = std::mem::take(&mut self.events_current);
        let saved_events_next = std::mem::take(&mut self.events_next);
        let saved_emit_ids_current = std::mem::take(&mut self.emit_ids_current);
        let saved_emit_ids_next = std::mem::take(&mut self.emit_ids_next);
        // delayed (`emit … after N`) queues swap too: sim ticks must not
        // age the live queue. The fork's own timers seed via
        // restore_events_from; sim leftovers ride the result snapshot.
        let saved_delayed = std::mem::take(&mut self.delayed_events);

        // The saved timeline's event payloads now live in Rust locals where
        // the collector cannot see them: auto-GC stays off until they are
        // restored, or the simulation sweeps the main timeline's pending
        // events out from under it (web arena crash, 1-in-3 runs).
        self.gc_pause += 1;

        // The fork's pending events run inside the simulation — they are
        // part of the state being speculated on, not main-timeline residue.
        self.restore_events_from(&fork_snap);
        self.get_world_mut().restore(fork_snap);
        self.in_simulation_fork += 1;

        let sim_result = (|| -> Result<(), String> {
            for _ in 0..ticks {
                for name in &system_names {
                    self.run_system_by_name(name)?;
                }
                self.bi_flush_events(vec![])?;
            }
            Ok(())
        })();

        self.in_simulation_fork -= 1;

        // Whatever the simulation left in flight travels with the result.
        let new_snapshot = self.snapshot_with_events();

        *self.get_world_mut() = saved_world;
        self.events_current = saved_events_current;
        self.events_next = saved_events_next;
        self.emit_ids_current = saved_emit_ids_current;
        self.emit_ids_next = saved_emit_ids_next;
        self.delayed_events = saved_delayed;
        self.gc_pause -= 1;

        sim_result?;
        Ok(Value::world_fork(
            &mut self.gc,
            std::sync::Arc::new(new_snapshot),
        ))
    }

    fn bi_commit(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!("commit() expects 1 argument, got {}", args.len()));
        }
        let fork_snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "commit() argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        // The snapshot's pending events come back with it — commit restores
        // the *whole* program state, it does not launder the event queue.
        self.restore_events_from(&fork_snap);
        // Foreign provenance riding the fork (it crossed a wire) lands in
        // the local ledger now — adopting a timeline adopts its history.
        // Foreign emit ids are remapped to fresh local ids, including the
        // in-flight queue's, so handler writes that fire *after* this
        // commit still chain back to the remote emit records.
        if self.in_simulation_fork == 0 {
            if let Some(prov) = fork_snap.provenance.as_deref() {
                let id_map = self.ledger.ingest(prov, &std::collections::HashMap::new());
                for id in self.emit_ids_next.iter_mut() {
                    if let Some(&local) = id_map.get(id) {
                        *id = local;
                    }
                }
                for (_, _, _, id) in self.delayed_events.iter_mut() {
                    if let Some(&local) = id_map.get(id) {
                        *id = local;
                    }
                }
            }
        }
        self.get_world_mut().restore(fork_snap);
        // The program's `indexed` declarations are the source of truth;
        // snapshots carry only derived state. A snapshot from a foreign
        // lineage (old save, pre-fix wire decode) must not wipe the live
        // world's indexes — reconcile (no-op when they already agree).
        let decl = std::sync::Arc::clone(&self.indexed_decl);
        self.get_world_mut().ensure_indexed_fields(&decl);
        // Causality seam: provenance recorded before this point describes
        // the pre-fork timeline. `why()` discloses that honestly.
        if self.in_simulation_fork == 0 {
            self.ledger.record_commit(self.causality_frame);
        }
        Ok(Value::NIL)
    }

    /// `merge_forks(base, ours, theirs) -> Result<world_fork, str>` —
    /// three-way world merge (#7). Field-level: a conflict is the *same
    /// field* of the same entity/resource diverging from base in both forks.
    /// Id collisions between independent spawns are remapped (with deep
    /// reference rewriting), not conflicted; name collisions are honest
    /// conflicts. `commit()` the Ok fork to adopt the merged timeline.
    fn bi_merge_forks(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "merge_forks() expects 3 arguments (base, ours, theirs), got {}",
                args.len()
            ));
        }
        let snaps: Vec<std::sync::Arc<crate::world::WorldSnapshot>> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                a.as_world_fork().cloned().ok_or_else(|| {
                    format!(
                        "merge_forks() argument {} must be a world_fork, got {}",
                        i + 1,
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        self.run_merge(&snaps, crate::merge::Resolutions::default())
    }

    /// `merge_forks_with(base, ours, theirs, resolutions) -> Result<world_fork, list<Conflict>>`
    /// — the programmable half of conflicts-as-data. `resolutions` is a list
    /// of `(conflict, value)` pairs: each field conflict named by the pair
    /// merges as the given value instead of refusing; a NameConflict takes a
    /// list of new names (one per claiming entity) and a RenameConflict takes
    /// the chosen name. Despawn and event conflicts are not mechanically
    /// resolvable.
    fn bi_merge_forks_with(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(format!(
                "merge_forks_with() expects 4 arguments (base, ours, theirs, resolutions), got {}",
                args.len()
            ));
        }
        let snaps: Vec<std::sync::Arc<crate::world::WorldSnapshot>> = args[..3]
            .iter()
            .enumerate()
            .map(|(i, a)| {
                a.as_world_fork().cloned().ok_or_else(|| {
                    format!(
                        "merge_forks_with() argument {} must be a world_fork, got {}",
                        i + 1,
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        let list = args[3].as_list().ok_or_else(|| {
            format!(
                "merge_forks_with() resolutions must be a list of (conflict, value) pairs, got {}",
                args[3].type_name()
            )
        })?;
        let mut resolutions = crate::merge::Resolutions::default();
        for (i, pair) in list.iter().enumerate() {
            let items = pair.as_tuple().ok_or_else(|| {
                format!(
                    "merge_forks_with() resolution {} must be a (conflict, value) tuple, got {}",
                    i,
                    pair.type_name()
                )
            })?;
            if items.len() != 2 {
                return Err(format!(
                    "merge_forks_with() resolution {} must be a (conflict, value) pair",
                    i
                ));
            }
            let st = items[0].as_sum_type().ok_or_else(|| {
                format!(
                    "merge_forks_with() resolution {}: first element must be a Conflict, got {}",
                    i,
                    items[0].type_name()
                )
            })?;
            if st.type_name != "Conflict" {
                return Err(format!(
                    "merge_forks_with() resolution {}: expected a Conflict, got {}",
                    i, st.type_name
                ));
            }
            let key = match st.variant.as_str() {
                "FieldConflict" => {
                    let eid = st
                        .fields
                        .get("ent")
                        .and_then(|v| v.as_entity_id())
                        .ok_or("merge_forks_with(): FieldConflict missing entity")?;
                    let component = st
                        .fields
                        .get("comp")
                        .and_then(|v| v.as_str().map(String::from))
                        .ok_or("merge_forks_with(): FieldConflict missing component")?;
                    let field = st
                        .fields
                        .get("field")
                        .and_then(|v| v.as_str().map(String::from))
                        .ok_or("merge_forks_with(): FieldConflict missing field")?;
                    (Some(eid), component, field)
                }
                "ResourceFieldConflict" => {
                    let resource = st
                        .fields
                        .get("res")
                        .and_then(|v| v.as_str().map(String::from))
                        .ok_or("merge_forks_with(): ResourceFieldConflict missing resource")?;
                    let field = st
                        .fields
                        .get("field")
                        .and_then(|v| v.as_str().map(String::from))
                        .ok_or("merge_forks_with(): ResourceFieldConflict missing field")?;
                    (None, resource, field)
                }
                // Name claims are resolvable by *renaming*: the value is a
                // list of names parallel to the conflict's `entities` list
                // ("keep both as T-5/a, T-5/b"). "" unnames. The merge
                // re-validates: chosen names that still collide come back
                // as conflicts, so a rename can never steal a name unnoticed.
                "NameConflict" => {
                    let ents = st
                        .fields
                        .get("entities")
                        .and_then(|v| v.as_list().map(|l| l.to_vec()))
                        .ok_or("merge_forks_with(): NameConflict missing entities")?;
                    let names = items[1].as_list().ok_or_else(|| {
                        format!(
                            "merge_forks_with() resolution {}: a NameConflict resolution \
                             must be a list of new names (one per claiming entity), got {}",
                            i,
                            items[1].type_name()
                        )
                    })?;
                    if names.len() != ents.len() {
                        return Err(format!(
                            "merge_forks_with() resolution {}: NameConflict has {} claiming \
                             entities but {} names were given (one name per entity, \
                             \"\" to unname)",
                            i,
                            ents.len(),
                            names.len()
                        ));
                    }
                    for (ent, name) in ents.iter().zip(names.iter()) {
                        let eid = ent.as_entity_id().ok_or(
                            "merge_forks_with(): NameConflict entities must be entity ids",
                        )?;
                        let n = name.as_str().ok_or_else(|| {
                            format!(
                                "merge_forks_with() resolution {}: NameConflict names must \
                                 be strings, got {}",
                                i,
                                name.type_name()
                            )
                        })?;
                        resolutions
                            .renames
                            .insert(eid, Some(n.to_string()).filter(|s| !s.is_empty()));
                    }
                    continue;
                }
                // Renamed-differently-in-both-forks: the value is the one
                // name the entity should carry.
                "RenameConflict" => {
                    let eid = st
                        .fields
                        .get("ent")
                        .and_then(|v| v.as_entity_id())
                        .ok_or("merge_forks_with(): RenameConflict missing entity")?;
                    let n = items[1].as_str().ok_or_else(|| {
                        format!(
                            "merge_forks_with() resolution {}: a RenameConflict resolution \
                             must be the chosen name (a str), got {}",
                            i,
                            items[1].type_name()
                        )
                    })?;
                    resolutions
                        .renames
                        .insert(eid, Some(n.to_string()).filter(|s| !s.is_empty()));
                    continue;
                }
                other => {
                    return Err(format!(
                        "merge_forks_with(): {} is not mechanically resolvable \
                         (field, resource-field, name-claim, and rename conflicts are; \
                         despawns and event consumption have no honest 'pick a side')",
                        other
                    ));
                }
            };
            resolutions.fields.insert(key, items[1]);
        }
        self.run_merge(&snaps, resolutions)
    }

    fn run_merge(
        &mut self,
        snaps: &[std::sync::Arc<crate::world::WorldSnapshot>],
        resolutions: crate::merge::Resolutions,
    ) -> Result<Value, String> {
        match crate::merge::merge_worlds(
            &snaps[0],
            &snaps[1],
            &snaps[2],
            &mut self.gc,
            &resolutions,
        ) {
            Ok(outcome) => {
                let mut snap = outcome.world.snapshot();
                // The merged in-flight event queue travels with the fork;
                // commit() will restore it. Never silently dropped.
                snap.events = std::sync::Arc::new(outcome.events);
                snap.emit_ids = std::sync::Arc::new(outcome.emit_ids);
                snap.delayed = std::sync::Arc::new(outcome.delayed);
                // Foreign provenance survives the merge: records from either
                // input ride the merged fork (theirs' entity ids follow the
                // spawn-collision remap), so commit() can stitch the remote
                // history into the local ledger.
                let remap: std::collections::HashMap<u32, u32> =
                    outcome.remapped.iter().copied().collect();
                let mut combined = crate::causality::WireProvenance::default();
                for (src, apply_remap) in [(&snaps[1], false), (&snaps[2], true)] {
                    if let Some(p) = src.provenance.as_deref() {
                        // Materialize each record's origin now — the two
                        // sides may have arrived from different machines.
                        let label = || {
                            Some(if p.origin.is_empty() {
                                "wire".to_string()
                            } else {
                                p.origin.clone()
                            })
                        };
                        for w in &p.writes {
                            let mut w = w.clone();
                            if apply_remap {
                                if let Some(e) = w.entity {
                                    w.entity = Some(remap.get(&e).copied().unwrap_or(e));
                                }
                            }
                            w.origin = w.origin.take().or_else(label);
                            combined.writes.push(w);
                        }
                        for e in &p.emits {
                            let mut e = e.clone();
                            e.origin = e.origin.take().or_else(label);
                            combined.emits.push(e);
                        }
                    }
                }
                if !combined.writes.is_empty() || !combined.emits.is_empty() {
                    snap.provenance = Some(std::sync::Arc::new(combined));
                }
                let v = Value::world_fork(&mut self.gc, std::sync::Arc::new(snap));
                Ok(self.make_result(true, v))
            }
            Err(conflicts) => {
                let items: Vec<Value> = conflicts
                    .iter()
                    .map(|c| self.conflict_to_value(c))
                    .collect();
                let v = Value::list(&mut self.gc, items);
                Ok(self.make_result(false, v))
            }
        }
    }

    /// One [`crate::merge::MergeConflict`] as a rad `Conflict` sum value —
    /// the boundary where merge conflicts become user-space data.
    fn conflict_to_value(&mut self, c: &crate::merge::MergeConflict) -> Value {
        use crate::merge::MergeConflict as MC;
        let mut fields = std::collections::HashMap::new();
        let variant = match c {
            MC::Field {
                entity,
                entity_name,
                component,
                field,
                base,
                ours,
                theirs,
            } => {
                fields.insert("ent".into(), Value::from_entity_id(&mut self.gc, *entity));
                let n = entity_name.clone().unwrap_or_default();
                fields.insert("name".into(), Value::from_string(&mut self.gc, n));
                fields.insert(
                    "comp".into(),
                    Value::from_string(&mut self.gc, component.clone()),
                );
                fields.insert(
                    "field".into(),
                    Value::from_string(&mut self.gc, field.clone()),
                );
                fields.insert("base".into(), *base);
                fields.insert("ours".into(), *ours);
                fields.insert("theirs".into(), *theirs);
                "FieldConflict"
            }
            MC::Component {
                entity,
                entity_name,
                component,
                detail,
            } => {
                fields.insert("ent".into(), Value::from_entity_id(&mut self.gc, *entity));
                let n = entity_name.clone().unwrap_or_default();
                fields.insert("name".into(), Value::from_string(&mut self.gc, n));
                fields.insert(
                    "comp".into(),
                    Value::from_string(&mut self.gc, component.clone()),
                );
                fields.insert(
                    "detail".into(),
                    Value::from_string(&mut self.gc, detail.clone()),
                );
                "ComponentConflict"
            }
            MC::Despawn {
                entity,
                entity_name,
                detail,
            } => {
                fields.insert("ent".into(), Value::from_entity_id(&mut self.gc, *entity));
                let n = entity_name.clone().unwrap_or_default();
                fields.insert("name".into(), Value::from_string(&mut self.gc, n));
                fields.insert(
                    "detail".into(),
                    Value::from_string(&mut self.gc, detail.clone()),
                );
                "DespawnConflict"
            }
            MC::Rename {
                entity,
                base,
                ours,
                theirs,
            } => {
                fields.insert("ent".into(), Value::from_entity_id(&mut self.gc, *entity));
                fields.insert(
                    "base".into(),
                    Value::from_string(&mut self.gc, base.clone()),
                );
                fields.insert(
                    "ours".into(),
                    Value::from_string(&mut self.gc, ours.clone()),
                );
                fields.insert(
                    "theirs".into(),
                    Value::from_string(&mut self.gc, theirs.clone()),
                );
                "RenameConflict"
            }
            MC::NameClaim { name, entities } => {
                fields.insert(
                    "name".into(),
                    Value::from_string(&mut self.gc, name.clone()),
                );
                let ids: Vec<Value> = entities
                    .iter()
                    .map(|&e| Value::from_entity_id(&mut self.gc, e))
                    .collect();
                fields.insert("entities".into(), Value::list(&mut self.gc, ids));
                "NameConflict"
            }
            MC::ResourceField {
                resource,
                field,
                base,
                ours,
                theirs,
            } => {
                fields.insert(
                    "res".into(),
                    Value::from_string(&mut self.gc, resource.clone()),
                );
                fields.insert(
                    "field".into(),
                    Value::from_string(&mut self.gc, field.clone()),
                );
                fields.insert("base".into(), *base);
                fields.insert("ours".into(), *ours);
                fields.insert("theirs".into(), *theirs);
                "ResourceFieldConflict"
            }
            MC::Resource { resource, detail } => {
                fields.insert(
                    "res".into(),
                    Value::from_string(&mut self.gc, resource.clone()),
                );
                fields.insert(
                    "detail".into(),
                    Value::from_string(&mut self.gc, detail.clone()),
                );
                "ResourceConflict"
            }
            MC::Events {
                detail,
                base,
                ours,
                theirs,
            } => {
                fields.insert(
                    "detail".into(),
                    Value::from_string(&mut self.gc, detail.clone()),
                );
                fields.insert("base".into(), Value::from_int(&mut self.gc, *base as i64));
                fields.insert("ours".into(), Value::from_int(&mut self.gc, *ours as i64));
                fields.insert(
                    "theirs".into(),
                    Value::from_int(&mut self.gc, *theirs as i64),
                );
                "EventConflict"
            }
            MC::Relations { detail } => {
                fields.insert(
                    "detail".into(),
                    Value::from_string(&mut self.gc, detail.clone()),
                );
                "RelationConflict"
            }
        };
        Value::sum_type(
            &mut self.gc,
            "Conflict".to_string(),
            variant.to_string(),
            fields,
        )
    }

    fn bi_clock(&self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!("clock() takes no arguments, got {}", args.len()));
        }
        // SystemTime::now() traps on wasm32-unknown-unknown; the browser's
        // Date.now() is the wall clock there.
        #[cfg(target_arch = "wasm32")]
        {
            Ok(Value::from_float(js_sys::Date::now() / 1000.0))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::time::SystemTime;
            let dur = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default();
            Ok(Value::from_float(dur.as_secs_f64()))
        }
    }

    fn bi_peek(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!("peek() expects 3 arguments, got {}", args.len()));
        }
        let snapshot = args[0]
            .as_world_fork()
            .ok_or_else(|| "peek() first argument must be a world_fork".to_string())?;
        let eid = args[1].as_entity_id().ok_or_else(|| {
            format!(
                "peek() second argument must be an entity, got {}",
                args[1].type_name()
            )
        })?;
        let ctype = Self::expect_component_type_name(&args[2], "peek")?;

        match snapshot.get_component(eid, &ctype) {
            Some(comp) => {
                let mut fields = std::collections::HashMap::new();
                fields.insert(
                    "value".to_string(),
                    Value::from_component_data(&mut self.gc, comp),
                );
                Ok(Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                ))
            }
            None => Ok(Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                std::collections::HashMap::new(),
            )),
        }
    }

    /// `peek_resource(fork, Resource) -> Option<value>` — the resource
    /// dual of `peek`: read a fork's resource without committing.
    fn bi_peek_resource(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "peek_resource() expects 2 arguments, got {}",
                args.len()
            ));
        }
        let snapshot = args[0]
            .as_world_fork()
            .ok_or_else(|| "peek_resource() first argument must be a world_fork".to_string())?;
        let rtype = Self::expect_component_type_name(&args[1], "peek_resource")?;

        match snapshot.get_resource(&rtype) {
            Some(data) => {
                let mut fields = std::collections::HashMap::new();
                fields.insert(
                    "value".to_string(),
                    Value::from_component_data(&mut self.gc, data),
                );
                Ok(Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                ))
            }
            None => Ok(Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                std::collections::HashMap::new(),
            )),
        }
    }

    fn make_result(&mut self, ok: bool, value: Value) -> Value {
        let mut fields = std::collections::HashMap::new();
        // Language convention: `Ok { value }` but `Err { message }` — the
        // parser desugars `Err(x)` patterns to the `message` field.
        let field = if ok { "value" } else { "message" };
        fields.insert(field.to_string(), value);
        Value::sum_type(
            &mut self.gc,
            "Result".to_string(),
            if ok { "Ok" } else { "Err" }.to_string(),
            fields,
        )
    }

    fn schedule_from_value(value: &Value, fn_name: &str) -> Result<Vec<String>, String> {
        let list = value
            .as_list()
            .ok_or_else(|| format!("{}() schedule argument must be a list of systems", fn_name))?;
        list.iter()
            .map(|v| {
                v.as_system_ref().map(|s| s.to_string()).ok_or_else(|| {
                    format!(
                        "{}() schedule must be a list of `system` values, got {}",
                        fn_name,
                        v.type_name()
                    )
                })
            })
            .collect()
    }

    /// `simulate_par(fork, schedule, ticks, n_forks, seed) -> [world_fork]`
    ///
    /// Runs `n_forks` independent simulations of the same starting fork in
    /// parallel on the worker-VM pool. Each fork gets a deterministic RNG seed
    /// derived from `seed` and its index, so results are bit-identical for the
    /// same inputs regardless of thread count or scheduling order.
    fn bi_simulate_par(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 5 && args.len() != 6 {
            return Err(format!(
                "simulate_par() expects 5 arguments (plus an optional list of resource overrides), got {}",
                args.len()
            ));
        }
        let mut fork_snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "simulate_par() first argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        // Optional 6th argument: resource overrides applied to the base fork
        // before any rollout runs — seed a candidate policy at the call site
        // instead of commit()ing it into the live world (dogfood feature seq
        // 150 #2). Same validation as `fork_with`, applied left to right.
        if let Some(overrides) = args.get(5) {
            let list = overrides.as_list().ok_or_else(|| {
                format!(
                    "simulate_par() sixth argument must be a list of resource values, got {}",
                    overrides.type_name()
                )
            })?;
            for (i, item) in list.iter().enumerate() {
                let data = item.as_component().ok_or_else(|| {
                    format!(
                        "simulate_par() override {} must be a resource value, got {}",
                        i,
                        item.type_name()
                    )
                })?;
                let name = data.type_name.clone();
                fork_snap = fork_snap.with_resource(&name, data.clone());
            }
        }
        let system_names = Self::schedule_from_value(&args[1], "simulate_par")?;
        let ticks = args[2].as_int().ok_or_else(|| {
            "simulate_par() third argument (ticks) must be an integer".to_string()
        })?;
        if ticks < 0 {
            return Err("simulate_par() tick count must be non-negative".to_string());
        }
        let n_forks = args[3].as_int().ok_or_else(|| {
            "simulate_par() fourth argument (n_forks) must be an integer".to_string()
        })?;
        if n_forks < 0 {
            return Err("simulate_par() fork count must be non-negative".to_string());
        }
        let seed = args[4]
            .as_int()
            .ok_or_else(|| "simulate_par() fifth argument (seed) must be an integer".to_string())?
            as u64;

        for name in &system_names {
            if !self.systems.contains_key(name) {
                return Err(format!("simulate_par(): unknown system '{}'", name));
            }
        }

        let shared = self.shared_state();
        let run_fork = |i: u64| {
            super::exec::with_worker_vm(&shared, |worker| {
                // Pending events are part of the forked state: each
                // worker timeline starts with the same in-flight queue.
                worker.restore_events_from(&fork_snap);
                worker.get_world_mut().restore(fork_snap.clone());
                worker.set_random_seed(crate::sandbox::fork_seed(seed, i));
                // The worker owns a private copy of the world, so ECS
                // writes apply directly instead of being deferred into the
                // command buffer (which would hide tick N's writes from
                // tick N+1).
                let was_worker = worker.is_worker;
                worker.is_worker = false;
                worker.in_simulation_fork += 1;

                let sim_result = (|| -> Result<(), String> {
                    for _ in 0..ticks {
                        for name in &system_names {
                            worker.run_system_by_name(name)?;
                        }
                        worker.bi_flush_events(vec![])?;
                    }
                    Ok(())
                })();

                worker.in_simulation_fork -= 1;
                worker.is_worker = was_worker;

                let snap = worker.snapshot_with_events();
                *worker.get_world_mut() = crate::world::World::new();
                worker.events_current.clear();
                worker.events_next.clear();
                worker.emit_ids_current.clear();
                worker.emit_ids_next.clear();
                // pooled workers must not carry timers into the next call
                worker.delayed_events.clear();
                sim_result.map(|_| snap)
            })
        };
        // wasm32 has no threads: the futures run sequentially on the same
        // pooled worker VM — identical results (each fork is seeded), no rayon.
        #[cfg(target_arch = "wasm32")]
        let snapshots: Vec<Result<crate::world::WorldSnapshot, String>> =
            (0..n_forks as u64).map(run_fork).collect();
        #[cfg(not(target_arch = "wasm32"))]
        let snapshots: Vec<Result<crate::world::WorldSnapshot, String>> = {
            use rayon::prelude::*;
            (0..n_forks as u64).into_par_iter().map(run_fork).collect()
        };

        let mut forks = Vec::with_capacity(snapshots.len());
        for (i, snap) in snapshots.into_iter().enumerate() {
            let mut snap = snap.map_err(|e| format!("simulate_par() fork {}: {}", i, e))?;
            // `fork_seed()` answers "which rng seed produced this rollout" —
            // hand that to `simulate_seeded` to reproduce it in isolation.
            snap.rollout_seed = Some(crate::sandbox::fork_seed(seed, i as u64));
            forks.push(Value::world_fork(&mut self.gc, std::sync::Arc::new(snap)));
        }
        Ok(Value::list(&mut self.gc, forks))
    }}
