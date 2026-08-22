// Entity identity and lifetime: spawning, despawning, naming, and the
// state-machine transition that retires one entity shape for another.

impl VM {

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
        self.transaction_check_write("$entities")?;
        self.transaction_check_write("$entity_identity")?;
        if name.is_some() {
            self.transaction_check_write("$entity_names")?;
        }
        for arg in &args {
            if let Some(component) = arg.as_component() {
                self.transaction_check_write(&component.type_name)?;
            }
        }
        let spawn_checkpoint = self
            .transaction
            .is_some()
            .then(|| self.world.transaction_spawn_checkpoint(name.as_deref()));
        let eid = self
            .world
            .spawn_entity(name.as_deref())
            .map_err(|error| error.to_string())?;
        if let Some(checkpoint) = spawn_checkpoint {
            self.transaction_record_spawn(eid, checkpoint);
        }
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
            if self.transaction.is_some() {
                let buffered = match self.command_buffer.last() {
                    Some(crate::vm::EcsCommand::SpawnEntity(_, components, _)) => {
                        components.clone()
                    }
                    _ => unreachable!("spawn command was just appended"),
                };
                for data in buffered {
                    let cname = data.type_name.clone();
                    let summary = Self::component_summary(&data);
                    let _ = self.world.add_component_owned(eid, data);
                    self.record_causal_write(
                        Some(eid),
                        &cname,
                        crate::causality::WriteKind::Spawn,
                        summary,
                    );
                }
            }
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

    fn bi_despawn(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.is_empty() {
            return Err("despawn() requires 1 argument".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("despawn() expects entity, got {}", args[0].type_name()))?;
        self.sandbox_check_despawn()?;
        self.transaction_check_write("*")?;
        let transaction_undo = self.transaction_prepare_despawn(eid)?;
        if self.is_worker {
            self.command_buffer
                .push(crate::vm::EcsCommand::DespawnEntity(eid));
            if self.transaction.is_some() {
                let name = self.world.entity_name(eid);
                if !self.world.destroy_entity(eid) {
                    return Err(format!("despawn() called on non-existent entity {}", eid));
                }
                self.transaction_record_despawn(transaction_undo);
                self.record_causal_write_named(
                    Some(eid),
                    name,
                    "*",
                    crate::causality::WriteKind::Despawn,
                    String::new(),
                );
            }
            Ok(Value::from_bool(true))
        } else {
            let entity_name = self.world.entity_name(eid);
            if !self.world.destroy_entity(eid) {
                return Err(format!("despawn() called on non-existent entity {}", eid));
            }
            self.transaction_record_despawn(transaction_undo);
            self.record_causal_write_named(
                Some(eid),
                entity_name,
                "*",
                crate::causality::WriteKind::Despawn,
                String::new(),
            );
            Ok(Value::from_bool(true))
        }
    }

    fn option_entity(&mut self, entity: Option<u32>) -> Value {
        match entity {
            Some(entity) => {
                let mut fields = HashMap::new();
                fields.insert(
                    "value".to_string(),
                    Value::from_entity_id(&mut self.gc, entity),
                );
                Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                )
            }
            None => Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                HashMap::new(),
            ),
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

    fn bi_enter_phase(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "enter_phase() expects 1 phase name, got {}",
                args.len()
            ));
        }
        let phase = args[0]
            .as_str()
            .ok_or_else(|| "enter_phase() expects a string phase name".to_string())?
            .to_string();
        self.get_world_mut().enter_phase(&phase)?;
        let prefix = format!("$phase${phase}$");
        for (name, _, _) in &mut self.events_next {
            if let Some(event) = name.strip_prefix(&prefix) {
                *name = event.to_string();
            }
        }
        self.bi_flush_events(Vec::new())?;
        Ok(Value::NIL)
    }

    fn bi_mark_phase(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "mark_phase() expects entity and phase, got {} arguments",
                args.len()
            ));
        }
        let entity = args[0]
            .as_entity_id()
            .ok_or_else(|| "mark_phase() first argument must be an entity".to_string())?;
        let phase = args[1]
            .as_str()
            .ok_or_else(|| "mark_phase() second argument must be a string".to_string())?
            .to_string();
        self.get_world_mut().mark_lifecycle_phase(entity, &phase)?;
        Ok(Value::NIL)
    }
}
