impl VM {

    fn bi_find(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("find() requires 2 arguments (list, predicate)".into());
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else {
            return Err(format!("find() expects list, got {}", list.type_name()));
        };

        for item in items.into_iter() {
            let r = self.call_value(&func, vec![item])?;
            if r.is_truthy() {
                let mut fields = std::collections::HashMap::new();
                fields.insert("value".to_string(), item);
                return Ok(Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                ));
            }
        }
        Ok(Value::sum_type(
            &mut self.gc,
            "Option".to_string(),
            "None".to_string(),
            std::collections::HashMap::new(),
        ))
    }

    /// `any(xs, pred)` / `all(xs, pred)` — short-circuiting predicate
    /// sweeps. `any([])` is false, `all([])` is true (vacuous truth).
    fn bi_any_all(&mut self, args: Vec<Value>, is_any: bool) -> Result<Value, String> {
        let name = if is_any { "any" } else { "all" };
        if args.len() != 2 {
            return Err(format!("{}() requires 2 arguments (list, predicate)", name));
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();
        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else {
            return Err(format!(
                "{}() expects a list, got {}",
                name,
                list.type_name()
            ));
        };
        for item in items.into_iter() {
            let truthy = self.call_value(&func, vec![item])?.is_truthy();
            if is_any && truthy {
                return Ok(Value::from_bool(true));
            }
            if !is_any && !truthy {
                return Ok(Value::from_bool(false));
            }
        }
        Ok(Value::from_bool(!is_any))
    }

    fn bi_max_by(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_extremum_by("max_by", args, false)
    }

    fn bi_min_by(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_extremum_by("min_by", args, true)
    }

    fn bi_extremum_by(
        &mut self,
        name: &str,
        args: Vec<Value>,
        want_min: bool,
    ) -> Result<Value, String> {
        if args.len() < 2 {
            return Err(format!("{}() requires 2 arguments (list, key_fn)", name));
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let key_fn = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else {
            return Err(format!("{}() expects list, got {}", name, list.type_name()));
        };

        if items.is_empty() {
            return Ok(Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                std::collections::HashMap::new(),
            ));
        }

        let mut best = items[0];
        let mut best_key = self.call_value(&key_fn, vec![best])?;
        for item in items.into_iter().skip(1) {
            let key = self.call_value(&key_fn, vec![item])?;
            let ord = helpers::compare_values(&key, &best_key).map_err(|e| {
                format!("{}() key function returned incomparable keys: {}", name, e)
            })?;
            let replace = if want_min {
                ord == std::cmp::Ordering::Less
            } else {
                ord == std::cmp::Ordering::Greater
            };
            if replace {
                best = item;
                best_key = key;
            }
        }

        let mut fields = std::collections::HashMap::new();
        fields.insert("value".to_string(), best);
        Ok(Value::sum_type(
            &mut self.gc,
            "Option".to_string(),
            "Some".to_string(),
            fields,
        ))
    }

    fn bi_reduce(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 3 {
            return Err("reduce() requires 3 arguments".into());
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let mut acc = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else if let Some(s) = list.as_str() {
            s.chars()
                .map(|c| Value::from_string(&mut self.gc, c.to_string()))
                .collect()
        } else {
            return Err(format!(
                "reduce() expects list or string, got {}",
                list.type_name()
            ));
        };

        for item in items.into_iter() {
            acc = self.call_value(&func, vec![acc, item])?;
        }
        Ok(acc)
    }

    fn expect_component_type_name(arg: &Value, fn_name: &str) -> Result<String, String> {
        if let Some(name) = arg.as_str() {
            return Ok(name.to_string());
        }
        if let Some(comp) = arg.as_component() {
            return Ok(comp.type_name.clone());
        }
        Err(format!(
            "{}() expects component type string or component value, got {}",
            fn_name,
            arg.type_name()
        ))
    }

    fn bi_get(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("get() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("get() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "get")?;
        self.sandbox_check_read(&ctype)?;
        match self.world.get_component(eid, &ctype) {
            Some(comp) => {
                let mut fields = HashMap::new();
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
                HashMap::new(),
            )),
        }
    }

    fn bi_get_resource(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("get_resource() requires 1 argument".into());
        }
        let rtype = Self::expect_component_type_name(&args[0], "get_resource")?;
        self.sandbox_check_read(&rtype)?;
        match self.world.get_resource(&rtype) {
            Some(comp) => {
                let mut fields = HashMap::new();
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
                HashMap::new(),
            )),
        }
    }

    /// `res(R) -> R` — direct resource access. Declared resources are
    /// auto-initialized from their field defaults, so the Option dance of
    /// `get_resource(R) |> unwrap` is pure ceremony; this is the shorthand.
    fn bi_res(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("res() requires 1 argument (the resource type)".into());
        }
        let rtype = Self::expect_component_type_name(&args[0], "res")?;
        self.sandbox_check_read(&rtype)?;
        match self.world.get_resource(&rtype) {
            Some(comp) => Ok(Value::from_component_data(&mut self.gc, comp)),
            None => Err(format!(
                "res() found no resource '{}' — is it declared with `resource {} {{ ... }}`?",
                rtype, rtype
            )),
        }
    }

    /// `recent_events(name, window) -> list` — payloads of every `name`
    /// event dispatched within the last `window` flush cycles (game
    /// ticks), oldest first. The queryable past: death recaps, combat
    /// windows, "what hit me" panels — straight off the deterministic
    /// event log instead of a hand-rolled ring buffer.
    fn bi_recent_events(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("recent_events() requires 2 arguments (event name, window ticks)".into());
        }
        let Some(name) = args[0].as_str() else {
            return Err(format!(
                "recent_events() event name must be a string, got {}",
                args[0].type_name()
            ));
        };
        let Some(window) = args[1].as_int() else {
            return Err(format!(
                "recent_events() window must be an int tick count, got {}",
                args[1].type_name()
            ));
        };
        let since = self.causality_frame.saturating_sub(window.max(0) as u64);
        let payloads: Vec<Value> = self
            .event_log
            .iter()
            .filter(|e| e.event_name == name && e.tick >= since)
            .map(|e| e.payload)
            .collect();
        Ok(Value::list(&mut self.gc, payloads))
    }

    fn bi_lookup(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err("lookup() requires 3 arguments".into());
        }
        let ctype = Self::expect_component_type_name(&args[0], "lookup")?;
        self.sandbox_check_read(&ctype)?;
        let field = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "lookup() second argument must be a field name string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        if !self.world.is_field_indexed(&ctype, &field) {
            return Err(format!(
                "lookup() requires an indexed field: '{}.{}' is not indexed",
                ctype, field
            ));
        }
        match self.world.index_lookup(&ctype, &field, args[2]) {
            Some(eid) => {
                let mut fields = HashMap::new();
                fields.insert(
                    "value".to_string(),
                    Value::from_entity_id(&mut self.gc, eid),
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
                HashMap::new(),
            )),
        }
    }

    /// `lookup_all(Comp, "field", value) -> list<entity>` — every entity
    /// whose indexed field equals the value, ids ascending (deterministic
    /// across save/load and replay). The multi-match sibling of `lookup`:
    /// "all open tickets" is one hash probe instead of an O(world) scan.
    fn bi_lookup_all(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err("lookup_all() requires 3 arguments".into());
        }
        let ctype = Self::expect_component_type_name(&args[0], "lookup_all")?;
        self.sandbox_check_read(&ctype)?;
        let field = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "lookup_all() second argument must be a field name string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        if !self.world.is_field_indexed(&ctype, &field) {
            return Err(format!(
                "lookup_all() requires an indexed field: '{}.{}' is not indexed",
                ctype, field
            ));
        }
        let ids = self.world.index_lookup_all(&ctype, &field, args[2]);
        let vals: Vec<Value> = ids
            .into_iter()
            .map(|eid| Value::from_entity_id(&mut self.gc, eid))
            .collect();
        Ok(Value::list(&mut self.gc, vals))
    }

    fn bi_require(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("require() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("require() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "require")?;
        self.sandbox_check_read(&ctype)?;
        match self.world.get_component(eid, &ctype) {
            Some(comp) => Ok(Value::from_component_data(&mut self.gc, comp)),
            None => {
                // The teaching error: who, what's missing, what's actually
                // there. A raw entity id helps nobody.
                let who = self
                    .world
                    .entity_name(eid)
                    .map(|n| format!("'{}'", n))
                    .unwrap_or_else(|| format!("entity {}", eid));
                if !self.world.contains_entity(eid) {
                    return Err(format!(
                        "require() on {}: entity no longer exists (despawned?)",
                        who
                    ));
                }
                let mut has: Vec<String> = self
                    .world
                    .components_on_entity(eid)
                    .iter()
                    .map(|c| c.type_name.clone())
                    .collect();
                has.sort();
                Err(format!(
                    "require() missing component '{}' on {} (has: [{}])",
                    ctype,
                    who,
                    has.join(", ")
                ))
            }
        }
    }

    fn bi_require_all(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("require_all() requires at least 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("require_all() expects entity, got {}", args[0].type_name()))?;
        let mut out = Vec::with_capacity(args.len() - 1);
        for arg in args.iter().skip(1) {
            let ctype = Self::expect_component_type_name(arg, "require_all")?;
            self.sandbox_check_read(&ctype)?;
            match self.world.get_component(eid, &ctype) {
                Some(comp) => out.push(Value::from_component_data(&mut self.gc, comp)),
                None => {
                    return Err(format!(
                        "require_all() missing component '{}' on entity {}",
                        ctype, eid
                    ));
                }
            }
        }
        Ok(Value::list(&mut self.gc, out))
    }

    fn bi_set(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("set() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("set() expects entity, got {}", args[0].type_name()))?;
        let data = args[1]
            .as_component()
            .ok_or_else(|| format!("set() expects component, got {}", args[1].type_name()))?;
        self.sandbox_check_write(&data.type_name)?;
        self.sandbox_check_write_shape(data)?;
        // One persist: the command buffer owns deferred values (they must
        // survive worker GC); the direct path hands ownership to the world
        // via the owned sink. Persisting on both sides of either path
        // abandons a full persistent copy per write.
        let mut data = data.clone();
        Value::persist_component_data(&mut data);
        if self.is_worker {
            self.command_buffer
                .push(crate::vm::EcsCommand::SetComponent(eid, data));
        } else {
            let cname = data.type_name.clone();
            let summary = Self::component_summary(&data);
            if !self.world.add_component_owned(eid, data) {
                return Err(format!("set() called on non-existent entity {}", eid));
            }
            self.record_causal_write(Some(eid), &cname, crate::causality::WriteKind::Set, summary);
        }
        Ok(Value::NIL)
    }

    fn bi_set_resource(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("set_resource() requires 2 arguments".into());
        }
        let rtype = Self::expect_component_type_name(&args[0], "set_resource")?;
        self.sandbox_check_write(&rtype)?;
        let data = args[1].as_component().ok_or_else(|| {
            format!(
                "set_resource() expects component, got {}",
                args[1].type_name()
            )
        })?;
        self.sandbox_check_write_shape(data)?;
        let data = data.clone();
        if self.is_worker {
            let mut buffered = data.clone();
            Value::persist_component_data(&mut buffered);
            self.command_buffer
                .push(crate::vm::EcsCommand::SetResource(rtype.clone(), buffered));
            // A resource is shared by every entity the system visits, so the
            // worker's private world must observe the write — otherwise the
            // next iteration reads the pre-batch snapshot and the buffered
            // absolute values all collapse to a single step.
            self.world.set_resource(&rtype, data);
        } else {
            let mut data = data;
            Value::persist_component_data(&mut data);
            let summary = Self::component_summary(&data);
            self.world.set_resource_owned(&rtype, data);
            self.record_causal_write(None, &rtype, crate::causality::WriteKind::Resource, summary);
        }
        Ok(Value::NIL)
    }

    fn bi_has(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("has() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("has() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "has")?;
        self.sandbox_check_read(&ctype)?;
        Ok(Value::from_bool(self.world.has_component(eid, &ctype)))
    }

    fn bi_spawn(&mut self, args: Vec<Value>) -> Result<Value, String> {
        // ACL before any mutation: the entity must not be spawned if any
        // component in the argument list is outside the capability grant,
        // carries a shape the host did not declare, or would shadow an
        // existing entity name.
        if self.sandbox_caps.is_some() {
            // Entity-name squatting (list item #2): a guest must not spawn
            // under a name that already resolves to a host entity. A
            // duplicate name silently reassigns the registry to the new
            // entity and orphans the old one, so the host's later
            // get_entity(name) would operate on guest-controlled data —
            // while diff/assert_only_changed, which do not cover the name
            // registry, report only the newly written component. Deny it.
            if let Some(name) = args.first().and_then(|v| v.as_str()) {
                if !name.is_empty() && self.world.get_entity_by_name(name).is_some() {
                    return Err(format!(
                        "sandbox: spawn(\"{}\", ...) denied — an entity named '{}' already \
                         exists; a sandboxed guest may not shadow an existing entity name",
                        name, name
                    ));
                }
            }
            for arg in &args {
                if let Some(c) = arg.as_component() {
                    self.sandbox_check_write(&c.type_name)?;
                    self.sandbox_check_write_shape(c)?;
                }
            }
        }
        let name = args.first().and_then(|v| v.as_str().map(|s| s.to_string()));
        let eid = self
            .world
            .spawn_entity(name.as_deref())
            .map_err(|error| error.to_string())?;
        let start_idx = if name.is_some() { 1 } else { 0 };

        if self.is_worker {
            let mut comps = Vec::new();
            for arg in args.iter().skip(start_idx) {
                if let Some(c) = arg.as_component() {
                    let mut data = c.clone();
                    Value::persist_component_data(&mut data);
                    comps.push(data);
                }
            }
            self.command_buffer
                .push(crate::vm::EcsCommand::SpawnEntity(name, comps, eid));
        } else {
            for arg in args.into_iter().skip(start_idx) {
                if let Some(c) = arg.as_component() {
                    let data = c.clone();
                    let cname = data.type_name.clone();
                    let summary = Self::component_summary(&data);
                    // add_component persists; pre-persisting here would
                    // abandon a copy per spawned component.
                    let _ = self.world.add_component(eid, data);
                    self.record_causal_write(
                        Some(eid),
                        &cname,
                        crate::causality::WriteKind::Spawn,
                        summary,
                    );
                }
            }
        }
        Ok(Value::from_entity_id(&mut self.gc, eid))
    }

    fn bi_get_entity(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("get_entity() requires 1 argument".into());
        }
        let name = args[0].as_str().ok_or_else(|| {
            format!(
                "get_entity() expects string name, got {}",
                args[0].type_name()
            )
        })?;
        if let Some(eid) = self.world.get_entity_by_name(name) {
            Ok(Value::from_entity_id(&mut self.gc, eid))
        } else {
            Ok(Value::NIL)
        }
    }

    /// `require_entity(name) -> entity` — the fail-fast dual of
    /// `get_entity` (same pairing as get/require for components).
    fn bi_require_entity(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("require_entity() requires 1 argument".into());
        }
        let name = args[0].as_str().ok_or_else(|| {
            format!(
                "require_entity() expects string name, got {}",
                args[0].type_name()
            )
        })?;
        match self.world.get_entity_by_name(name) {
            Some(eid) => Ok(Value::from_entity_id(&mut self.gc, eid)),
            None => Err(format!("require_entity(): no entity named '{}'", name)),
        }
    }

    /// `name_of(entity) -> str` — the inverse of `get_entity`. Anonymous
    /// entities yield "" (matching how summaries render unnamed ids).
    fn bi_name_of(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("name_of() requires 1 argument".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("name_of() expects entity, got {}", args[0].type_name()))?;
        let name = self.world.entity_name(eid).unwrap_or_default();
        Ok(Value::from_string(&mut self.gc, name))
    }

    fn bi_id_of(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("id_of() requires 1 argument".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("id_of() expects entity, got {}", args[0].type_name()))?;
        Ok(Value::from_int(&mut self.gc, eid as i64))
    }

    fn bi_remove(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("remove() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("remove() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "remove")?;
        self.sandbox_check_write(&ctype)?;
        if self.is_worker {
            self.command_buffer
                .push(crate::vm::EcsCommand::RemoveComponent(eid, ctype.clone()));
            Ok(Value::from_bool(true))
        } else {
            let removed = self.world.remove_component(eid, &ctype);
            if removed {
                self.record_causal_write(
                    Some(eid),
                    &ctype,
                    crate::causality::WriteKind::Remove,
                    String::new(),
                );
            }
            Ok(Value::from_bool(removed))
        }
    }

    fn bi_despawn(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.is_empty() {
            return Err("despawn() requires 1 argument".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("despawn() expects entity, got {}", args[0].type_name()))?;
        self.sandbox_check_despawn()?;
        if self.is_worker {
            self.command_buffer
                .push(crate::vm::EcsCommand::DespawnEntity(eid));
            Ok(Value::from_bool(true))
        } else {
            // Record before destroy: the entity's name is wiped with it.
            self.record_causal_write(
                Some(eid),
                "*",
                crate::causality::WriteKind::Despawn,
                String::new(),
            );
            if !self.world.destroy_entity(eid) {
                return Err(format!("despawn() called on non-existent entity {}", eid));
            }
            Ok(Value::from_bool(true))
        }
    }

    fn bi_entities(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.is_empty() {
            // Unfiltered `entities()` enumerates every entity in the world
            // regardless of component, so it cannot be keyed to a read grant
            // and requires the wildcard.
            self.sandbox_check_bulk_read("entities()")?;
            let ids = self.world.all_entity_ids();
            let mut vals = Vec::with_capacity(ids.len());
            for id in ids {
                vals.push(Value::from_entity_id(&mut self.gc, id));
            }
            Ok(Value::list(&mut self.gc, vals))
        } else {
            let ctypes: Result<Vec<String>, String> = args
                .iter()
                .map(|arg| Self::expect_component_type_name(arg, "entities"))
                .collect();
            let ctypes = ctypes?;
            for ctype in &ctypes {
                self.sandbox_check_read(ctype)?;
            }
            let ids = self.world.query(&ctypes, &[]);
            let mut vals = Vec::with_capacity(ids.len());
            for id in ids {
                vals.push(Value::from_entity_id(&mut self.gc, id));
            }
            Ok(Value::list(&mut self.gc, vals))
        }
    }

    fn bi_transition(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("transition() requires 2 arguments".into());
        }
        let s = args[0]
            .as_state()
            .ok_or_else(|| format!("transition() expects state, got {}", args[0].type_name()))?;
        let machine = s.machine.clone();
        let state = s.state.clone();
        let event = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "transition() expects event string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        self.transition_result(machine, state, event)
    }

    fn bi_map_or(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 3 {
            return Err("map_or() requires 3 arguments (option_or_result, default, fn)".into());
        }
        let container = args[0];
        let default_value = args[1];
        let mapper = args[2];
        if let Some(st) = container.as_sum_type() {
            if (st.type_name == "Option" && st.variant == "Some")
                || (st.type_name == "Result" && st.variant == "Ok")
            {
                let inner = st.fields.get("value").copied().unwrap_or(Value::NIL);
                return self.call_value(&mapper, vec![inner]);
            }
            if (st.type_name == "Option" && st.variant == "None")
                || (st.type_name == "Result" && st.variant == "Err")
            {
                return Ok(default_value);
            }
        }
        Ok(default_value)
    }

    fn bi_buffer_new(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!(
                "buffer_new() takes no arguments, got {}",
                args.len()
            ));
        }
        Ok(Value::buffer(&mut self.gc, String::new()))
    }

    fn bi_buffer_append(&mut self, mut args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "buffer_append() expects 2 arguments, got {}",
                args.len()
            ));
        }
        let s_val = args.pop().unwrap();
        let buf_val = args.pop().unwrap();

        let s = s_val
            .as_str()
            .ok_or_else(|| "buffer_append() second argument must be a string".to_string())?;

        let mut buf = buf_val
            .into_buffer()
            .ok_or_else(|| "buffer_append() first argument must be a buffer".to_string())?;

        buf.push_str(s);
        Ok(Value::buffer(&mut self.gc, buf))
    }

    fn bi_buffer_to_str(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "buffer_to_str() expects 1 argument, got {}",
                args.len()
            ));
        }
        let buf = args[0]
            .as_buffer()
            .ok_or_else(|| "buffer_to_str() argument must be a buffer".to_string())?;
        Ok(Value::from_string(&mut self.gc, buf.clone()))
    }

    fn bi_bytebuf_new(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_new() expects 1 argument, got {}",
                args.len()
            ));
        }
        let size = bytebuf_index_arg(&args[0], "bytebuf_new() size")?;
        Ok(Value::bytebuf(&mut self.gc, vec![0; size]))
    }

    fn bi_bytebuf_len(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_len() expects 1 argument, got {}",
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "bytebuf_len() expects a bytebuf".to_string())?;
        Ok(Value::from_int(&mut self.gc, bytes.len() as i64))
    }

    fn bi_bytebuf_get(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "bytebuf_get() expects 2 arguments, got {}",
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "bytebuf_get() expects a bytebuf".to_string())?;
        let idx = bytebuf_index_arg(&args[1], "bytebuf_get() index")?;
        if idx >= bytes.len() {
            return Err(format!(
                "bytebuf_get() index {} out of bounds (len {})",
                idx,
                bytes.len()
            ));
        }
        Ok(Value::from_int(&mut self.gc, i64::from(bytes[idx])))
    }

    fn bi_bytebuf_set_u8(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "bytebuf_set_u8() expects 3 arguments, got {}",
                args.len()
            ));
        }
        let mut bytes = args[0]
            .into_bytebuf()
            .ok_or_else(|| "bytebuf_set_u8() expects a bytebuf".to_string())?;
        let idx = bytebuf_index_arg(&args[1], "bytebuf_set_u8() index")?;
        let byte = bytebuf_u8_arg(&args[2], "bytebuf_set_u8() value")?;
        if idx >= bytes.len() {
            return Err(format!(
                "bytebuf_set_u8() index {} out of bounds (len {})",
                idx,
                bytes.len()
            ));
        }
        bytes[idx] = byte;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }

    fn bi_bytebuf_set_u32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_set_u32_or_i32_le(args, "bytebuf_set_u32_le()")
    }

    fn bi_bytebuf_set_i32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_set_u32_or_i32_le(args, "bytebuf_set_i32_le()")
    }

    fn bi_bytebuf_set_u32_or_i32_le(
        &mut self,
        args: Vec<Value>,
        fn_name: &str,
    ) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "{} expects 3 arguments, got {}",
                fn_name,
                args.len()
            ));
        }
        let mut bytes = args[0]
            .into_bytebuf()
            .ok_or_else(|| format!("{} expects a bytebuf", fn_name))?;
        let offset = bytebuf_index_arg(&args[1], &format!("{} offset", fn_name))?;
        let value = args[2]
            .as_int()
            .ok_or_else(|| format!("{} expects int value", fn_name))?;
        bytebuf_write_u32_le(&mut bytes, offset, value as u32, fn_name)?;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }

    fn bi_bytebuf_get_u32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_get_u32_or_i32_le(args, false, "bytebuf_get_u32_le()")
    }

    fn bi_bytebuf_get_i32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_get_u32_or_i32_le(args, true, "bytebuf_get_i32_le()")
    }}
