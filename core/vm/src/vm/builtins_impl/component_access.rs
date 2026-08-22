// Reading and writing component and resource state on one entity:
// whole-component access, single-field access, and presence tests.

impl VM {
    /// Component type name borrowed from the argument instead of copied.
    ///
    /// `read_field`/`write_field` run once per field per entity per frame
    /// (~36M times in the dispatch60 production run) and the owning variant
    /// allocated a `String` on every one of them just to name a type the
    /// caller already had in hand.
    fn component_type_name<'a>(arg: &'a Value, fn_name: &str) -> Result<&'a str, String> {
        if let Some(name) = arg.as_str() {
            return Ok(name);
        }
        if let Some(component) = arg.as_component() {
            return Ok(&component.type_name);
        }
        Err(format!(
            "{}() expects component type string or component value, got {}",
            fn_name,
            arg.type_name()
        ))
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

    fn bi_read_field(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "read_field() expects (entity, Component, \"field\"), got {} arguments",
                args.len()
            ));
        }
        let entity = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("read_field() expects entity, got {}", args[0].type_name()))?;
        let component = Self::component_type_name(&args[1], "read_field")?;
        let field = args[2]
            .as_str()
            .ok_or_else(|| "read_field() field must be a string literal".to_string())?;
        self.sandbox_check_read(component)?;
        let value = self
            .world
            .component_field_value(entity, component, field)
            .ok_or_else(|| {
                format!(
                    "read_field() cannot read '{}.{}' on entity {}: component or field is absent",
                    component, field, entity
                )
            })?;
        Ok(if value.object_identity().is_some() {
            value.deep_copy(&mut self.gc)
        } else {
            value
        })
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
        self.transaction_check_write(&data.type_name)?;
        self.transaction_journal_component(eid, &data.type_name);
        // One persist: the command buffer owns deferred values (they must
        // survive worker GC); the direct path hands ownership to the world
        // via the owned sink. Persisting on both sides of either path
        // abandons a full persistent copy per write.
        let mut data = data.clone();
        Value::persist_component_data(&mut data);
        if self.is_worker {
            self.command_buffer
                .push(crate::vm::EcsCommand::SetComponent(eid, data.clone()));
            if self.transaction.is_some() {
                let cname = data.type_name.clone();
                let summary = Self::component_summary(&data);
                if !self.world.add_component_owned(eid, data) {
                    return Err(format!("set() called on non-existent entity {}", eid));
                }
                self.record_causal_write(
                    Some(eid),
                    &cname,
                    crate::causality::WriteKind::Set,
                    summary,
                );
            }
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

    fn bi_write_field(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(format!(
                "write_field() expects (entity, Component, \"field\", value), got {} arguments",
                args.len()
            ));
        }
        let entity = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("write_field() expects entity, got {}", args[0].type_name()))?;
        let component = Self::component_type_name(&args[1], "write_field")?;
        let field = args[2]
            .as_str()
            .ok_or_else(|| "write_field() field must be a string literal".to_string())?;
        self.sandbox_check_write(component)?;
        self.transaction_check_write(component)?;
        self.transaction_journal_component(entity, component);

        let persisted = args[3].deep_copy(&mut crate::value::PersistentStore);
        if self.is_worker {
            let buffered = args[3].deep_copy(&mut crate::value::PersistentStore);
            if !self
                .world
                .set_component_field_owned(entity, component, field, persisted)
            {
                unsafe { buffered.release_persistent() };
                return Err(format!(
                    "write_field() failed to update '{}.{}' on entity {}",
                    component, field, entity
                ));
            }
            self.command_buffer.push(crate::vm::EcsCommand::SetField(
                entity,
                component.to_string(),
                field.to_string(),
                buffered,
            ));
        } else if !self
            .world
            .set_component_field_owned(entity, component, field, persisted)
        {
            return Err(format!(
                "write_field() failed to update '{}.{}' on entity {}",
                component, field, entity
            ));
        }
        self.record_causal_field_write(entity, component, field, args[3]);
        Ok(Value::NIL)
    }

    fn bi_set_resource(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("set_resource() requires 2 arguments".into());
        }
        let rtype = Self::expect_component_type_name(&args[0], "set_resource")?;
        self.sandbox_check_write(&rtype)?;
        self.transaction_check_write(&rtype)?;
        self.transaction_journal_resource(&rtype);
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
            if self.transaction.is_some() {
                let written = self
                    .world
                    .get_resource(&rtype)
                    .expect("resource was just written");
                let summary = Self::component_summary(&written);
                self.record_causal_write(
                    None,
                    &rtype,
                    crate::causality::WriteKind::Resource,
                    summary,
                );
            }
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
        let ctype = Self::component_type_name(&args[1], "has")?;
        self.sandbox_check_read(ctype)?;
        Ok(Value::from_bool(self.world.has_component(eid, ctype)))
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
        self.transaction_check_write(&ctype)?;
        self.transaction_journal_component(eid, &ctype);
        if self.is_worker {
            self.command_buffer
                .push(crate::vm::EcsCommand::RemoveComponent(eid, ctype.clone()));
            if self.transaction.is_some() {
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
            } else {
                Ok(Value::from_bool(true))
            }
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
}
