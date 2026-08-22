// Reading sets of entities rather than one: indexed lookups, ordered-index
// traversal, materialized-view membership and its revision history.

impl VM {

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
        if args.len() == 2 {
            if let Some(view_name) = args[0].as_str() {
                if let Some(found) = self.world.materialized_view_lookup(view_name, &args[1]) {
                    return match found {
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
                    };
                }
            }
        }
        if args.len() != 3 {
            return Err("lookup() requires (view, key) or (component, field, value)".into());
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
            if args.len() == 1 {
                if let Some(view_name) = args[0].as_str() {
                    if let Some(dependencies) = self.world.materialized_view_dependencies(view_name)
                    {
                        for dependency in &dependencies {
                            self.sandbox_check_read(dependency)?;
                        }
                        let ids = self
                            .world
                            .materialized_view_entities(view_name)
                            .expect("view disappeared after dependency lookup");
                        let values = ids
                            .into_iter()
                            .map(|entity| Value::from_entity_id(&mut self.gc, entity))
                            .collect();
                        return Ok(Value::list(&mut self.gc, values));
                    }
                }
            }
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

    /// Traverse a materialized view without constructing an entity list or
    /// allocating per-entity wrappers. The callback must be a named guest
    /// function so the authority graph and allocation contract stay exact.
    fn bi_visit_view(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "visit_view() expects (view, named_callback), got {} arguments",
                args.len()
            ));
        }
        let view_name = args[0]
            .as_str()
            .ok_or_else(|| "visit_view() first argument must be a materialized view".to_string())?
            .to_string();
        let callback = args[1];
        if callback.as_fn().is_none() {
            return Err(
                "visit_view() requires a named function; capturing closures would violate the allocation-free traversal contract"
                    .to_string(),
            );
        }
        let dependencies = self
            .world
            .materialized_view_dependencies(&view_name)
            .ok_or_else(|| format!("Unknown materialized view '{view_name}'"))?;
        for dependency in &dependencies {
            self.sandbox_check_read(dependency)?;
        }
        let stable_revision = self
            .world
            .materialized_view_revision(&view_name)
            .expect("view disappeared after dependency lookup");
        let mut current = self
            .world
            .materialized_view_first_entity(&view_name)
            .expect("view disappeared after dependency lookup");
        while let Some(entity) = current {
            let next = self
                .world
                .materialized_view_next_entity(&view_name, entity)
                .expect("view disappeared during traversal");
            let argument = Value::from_entity_id(&mut self.gc, entity);
            self.call_named_unary(callback, argument)?;
            let revision = self
                .world
                .materialized_view_revision(&view_name)
                .expect("view disappeared during traversal");
            if revision != stable_revision {
                return Err(format!(
                    "visit_view({view_name}) was invalidated by its callback: revision {stable_revision} -> {revision}; stage membership-changing writes outside allocation-free traversal"
                ));
            }
            current = next;
        }
        Ok(Value::NIL)
    }

    fn bi_view_revision(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "revision() expects 1 view, got {} arguments",
                args.len()
            ));
        }
        let name = args[0]
            .as_str()
            .ok_or_else(|| "revision() expects a materialized view".to_string())?;
        let revision = self
            .world
            .materialized_view_revision(name)
            .ok_or_else(|| format!("unknown materialized view `{name}`"))?;
        let revision = i64::try_from(revision)
            .map_err(|_| format!("view `{name}` revision exceeds RAD int range"))?;
        Ok(Value::from_int(&mut self.gc, revision))
    }

    fn bi_changes_since(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "changes_since() expects 2 arguments, got {}",
                args.len()
            ));
        }
        let name = args[0]
            .as_str()
            .ok_or_else(|| "changes_since() expects a materialized view first".to_string())?;
        let revision = args[1]
            .as_int()
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| "changes_since() revision must be a non-negative integer".to_string())?;
        let changes = self
            .world
            .materialized_view_changes_since(name, revision)
            .ok_or_else(|| format!("unknown materialized view `{name}`"))?;
        let mut values = Vec::with_capacity(changes.len());
        for change in changes {
            let revision = i64::try_from(change.revision)
                .map_err(|_| format!("view `{name}` revision exceeds RAD int range"))?;
            let fields = vec![
                Value::from_int(&mut self.gc, revision),
                Value::from_entity_id(&mut self.gc, change.entity),
                Value::from_string(
                    &mut self.gc,
                    if change.entered { "enter" } else { "exit" }.to_string(),
                ),
                Value::from_string(&mut self.gc, change.reason),
            ];
            values.push(Value::tuple(&mut self.gc, fields));
        }
        Ok(Value::list(&mut self.gc, values))
    }

    fn bi_why_view(&mut self, args: Vec<Value>, expected_member: bool) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "why_{}_view() expects 2 arguments",
                if expected_member { "in" } else { "not_in" }
            ));
        }
        let name = args[0]
            .as_str()
            .ok_or_else(|| "view provenance expects a materialized view first".to_string())?;
        let entity = args[1]
            .as_entity_id()
            .ok_or_else(|| "view provenance expects an entity second".to_string())?;
        let (member, reason) = self
            .world
            .materialized_view_reason(name, entity)
            .ok_or_else(|| format!("unknown materialized view `{name}`"))?;
        let statement = if member {
            format!("entity {entity} is in {name}: {reason}")
        } else {
            format!("entity {entity} is not in {name}: {reason}")
        };
        if member != expected_member {
            return Err(statement);
        }
        Ok(Value::from_string(&mut self.gc, statement))
    }

    fn ordered_index_name(&self, args: &[Value], caller: &str) -> Result<(String, String), String> {
        let component = Self::expect_component_type_name(&args[0], caller)?;
        let field = args[1]
            .as_str()
            .ok_or_else(|| format!("{caller} field must be a string"))?
            .to_string();
        if !self.world.is_field_ordered(&component, &field) {
            return Err(format!(
                "{caller} requires `ordered indexed {field}` on component {component}"
            ));
        }
        Ok((component, field))
    }

    fn bi_ordered_bound(&mut self, args: Vec<Value>, strict: bool) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "{}() expects (component, field, value)",
                if strict {
                    "upper_bound/next"
                } else {
                    "lower_bound"
                }
            ));
        }
        let (component, field) = self.ordered_index_name(&args, "ordered bound")?;
        self.sandbox_check_read(&component)?;
        let entity = self
            .world
            .ordered_bound(&component, &field, &args[2], strict);
        Ok(self.option_entity(entity))
    }

    fn bi_ordered_previous(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err("previous() expects (component, field, value)".to_string());
        }
        let (component, field) = self.ordered_index_name(&args, "previous")?;
        self.sandbox_check_read(&component)?;
        let entity = self.world.ordered_previous(&component, &field, &args[2]);
        Ok(self.option_entity(entity))
    }

    fn bi_ordered_edge(&mut self, args: Vec<Value>, last: bool) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "{}() expects (component, field)",
                if last { "last" } else { "first" }
            ));
        }
        let (component, field) =
            self.ordered_index_name(&args, if last { "last" } else { "first" })?;
        self.sandbox_check_read(&component)?;
        let entity = if last {
            self.world.ordered_last(&component, &field)
        } else {
            self.world.ordered_first(&component, &field)
        };
        Ok(self.option_entity(entity))
    }

    fn bi_range(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return bi_range(&mut self.gc, args);
        }
        let (component, field) = self.ordered_index_name(&args, "range")?;
        self.sandbox_check_read(&component)?;
        let entities = self
            .world
            .ordered_range(&component, &field, &args[2], &args[3])
            .ok_or_else(|| {
                "range() bounds must be indexable values of the field's type".to_string()
            })?;
        let values = entities
            .into_iter()
            .map(|entity| Value::from_entity_id(&mut self.gc, entity))
            .collect();
        Ok(Value::list(&mut self.gc, values))
    }
}
