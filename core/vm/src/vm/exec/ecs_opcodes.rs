// Opcodes that read and write authoritative world state on one entity:
// component and field access, spawning, and resource initialisation.

impl VM {
    pub(crate) fn exec_ecs_get(&mut self) -> Result<(), String> {
        let type_idx = self.read_u16()? as usize;
        let ctype = helpers::constant_string(self.current_chunk(), type_idx)?;
        let ent = self.pop()?;
        let eid = helpers::entity_id(&ent)?;
        let comp = self
            .world
            .get_component(eid, &ctype)
            .ok_or_else(|| format!("Missing component `{}` on entity {}", ctype, eid))?;
        let __v = Value::from_component_data(&mut self.gc, comp);
        self.push(__v);
        Ok(())
    }

    pub(crate) fn exec_ecs_read_field(&mut self) -> Result<(), String> {
        let component_idx = self.read_u16()? as usize;
        let field_idx = self.read_u16()? as usize;
        // Borrowed from the copied constants below, not freshly allocated: this
        // opcode runs once per field per entity per frame.
        let component_const = helpers::constant_string_value(self.current_chunk(), component_idx)?;
        let field_const = helpers::constant_string_value(self.current_chunk(), field_idx)?;
        let component = component_const.as_str().expect("validated string constant");
        let field = field_const.as_str().expect("validated string constant");
        let entity = helpers::entity_id(&self.pop()?)?;
        self.sandbox_check_read(component)?;
        let value = self
            .world
            .component_field_value(entity, component, field)
            .ok_or_else(|| {
                format!(
                    "read_field() cannot read '{component}.{field}' on entity {entity}: component or field is absent"
                )
            })?;
        let value = if value.object_identity().is_some() {
            value.deep_copy(&mut self.gc)
        } else {
            value
        };
        self.push(value);
        Ok(())
    }

    pub(crate) fn exec_ecs_write_field(&mut self) -> Result<(), String> {
        let component_idx = self.read_u16()? as usize;
        let field_idx = self.read_u16()? as usize;
        let component_const = helpers::constant_string_value(self.current_chunk(), component_idx)?;
        let field_const = helpers::constant_string_value(self.current_chunk(), field_idx)?;
        let component = component_const.as_str().expect("validated string constant");
        let field = field_const.as_str().expect("validated string constant");
        let value = self.pop()?;
        let entity = helpers::entity_id(&self.pop()?)?;
        self.write_component_field_value(entity, component, field, value)?;
        self.push(Value::NIL);
        Ok(())
    }

    fn write_component_field_value(
        &mut self,
        entity: u32,
        component: &str,
        field: &str,
        value: Value,
    ) -> Result<(), String> {
        self.sandbox_check_write(component)?;
        self.transaction_check_write(component)?;
        self.transaction_journal_component(entity, component);
        let persisted = value.deep_copy(&mut crate::value::PersistentStore);
        if self.is_worker {
            let buffered = value.deep_copy(&mut crate::value::PersistentStore);
            if !self
                .world
                .set_component_field_owned(entity, component, field, persisted)
            {
                unsafe { buffered.release_persistent() };
                return Err(format!(
                    "write_field() failed to update '{component}.{field}' on entity {entity}"
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
                "write_field() failed to update '{component}.{field}' on entity {entity}"
            ));
        }
        self.record_causal_field_write(entity, component, field, value);
        Ok(())
    }

    pub(crate) fn exec_logical_load(&mut self) -> Result<(), String> {
        self.exec_ecs_get()
    }

    pub(crate) fn exec_ecs_set(&mut self) -> Result<(), String> {
        let comp_val = self.pop()?;
        let ent = self.pop()?;
        let eid = helpers::entity_id(&ent)?;
        let type_name = comp_val.type_name().to_string();
        let mut data = comp_val
            .into_component()
            .ok_or_else(|| format!("EcsSet expected component, got {}", type_name))?;
        self.sandbox_check_write(&data.type_name)?;
        self.transaction_check_write(&data.type_name)?;
        // EcsSet is emitted only as the end-of-iteration writeback of a
        // `mut` query loop. The body may despawn the entity it is visiting
        // (the guide's TTL/particle cleanup idiom) or remove the bound
        // component; the writeback then has nothing to write to, by design —
        // the same rule the system writeback path applies on Op::Return.
        if !self.system_component_writeback_target_exists(eid, &data.type_name) {
            return Ok(());
        }
        self.transaction_journal_component(eid, &data.type_name);
        Value::persist_component_data(&mut data);
        if self.is_worker {
            self.command_buffer
                .push(EcsCommand::SetComponent(eid, data.clone()));
            if self.transaction.is_some() {
                let cname = data.type_name.clone();
                let summary = Self::component_summary(&data);
                if !self.get_world_mut().add_component_owned(eid, data) {
                    return Err(format!(
                        "Cannot set component on non-existent entity {}",
                        eid
                    ));
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
            // Data is already persisted above; the owned sink transfers
            // ownership instead of deep-copying a second time.
            if !self.get_world_mut().add_component_owned(eid, data) {
                return Err(format!(
                    "Cannot set component on non-existent entity {}",
                    eid
                ));
            }
            self.record_causal_write(Some(eid), &cname, crate::causality::WriteKind::Set, summary);
        }
        Ok(())
    }

    pub(crate) fn exec_logical_store(&mut self) -> Result<(), String> {
        self.exec_ecs_set()
    }

    pub(crate) fn exec_materialize_aos(&mut self) -> Result<(), String> {
        Ok(())
    }

    pub(crate) fn exec_ecs_has(&mut self) -> Result<(), String> {
        let type_idx = self.read_u16()? as usize;
        let ctype = helpers::constant_string(self.current_chunk(), type_idx)?;
        let ent = self.pop()?;
        let eid = helpers::entity_id(&ent)?;
        self.push(Value::from_bool(
            self.get_world().has_component(eid, &ctype),
        ));
        Ok(())
    }

    pub(crate) fn exec_ecs_spawn(&mut self) -> Result<(), String> {
        let n = self.read_byte()? as usize;
        let mut comps = Vec::with_capacity(n);
        for _ in 0..n {
            let v = self.pop()?;
            let type_name = v.type_name().to_string();
            if let Some(state) = v.as_state() {
                comps.push(ComponentData {
                    type_name: state.machine.clone(),
                    layout: std::sync::Arc::new(vec!["state".to_string()]),
                    values: vec![Value::from_string(&mut self.gc, state.state.clone())],
                });
            } else {
                let mut data = v.into_component().ok_or_else(|| {
                    format!("EcsSpawn expected component or state, got {}", type_name)
                })?;
                Value::persist_component_data(&mut data);
                comps.push(data);
            }
        }
        let name_source = self.read_byte()?;
        let dynamic_name: Option<String> = if name_source == 1 {
            let _placeholder = self.read_u16()?;
            let name_val = self.pop()?;
            match name_val.as_str() {
                Some(s) if !s.is_empty() => Some(s.to_string()),
                _ => None,
            }
        } else {
            let name_idx = self.read_u16()? as usize;
            let name_str = helpers::constant_string(self.current_chunk(), name_idx)?;
            if name_str.is_empty() {
                None
            } else {
                Some(name_str.to_string())
            }
        };
        let name_opt = dynamic_name.as_deref();
        if self.sandbox_caps.is_some() {
            for c in &comps {
                self.sandbox_check_write(&c.type_name)?;
            }
        }
        self.transaction_check_write("$entities")?;
        self.transaction_check_write("$entity_identity")?;
        if name_opt.is_some() {
            self.transaction_check_write("$entity_names")?;
        }
        for component in &comps {
            self.transaction_check_write(&component.type_name)?;
        }
        let spawn_checkpoint = self
            .transaction
            .is_some()
            .then(|| self.get_world().transaction_spawn_checkpoint(name_opt));
        let eid = self
            .get_world_mut()
            .spawn_entity(name_opt)
            .map_err(|error| error.to_string())?;
        if let Some(checkpoint) = spawn_checkpoint {
            self.transaction_record_spawn(eid, checkpoint);
        }
        if self.is_worker {
            let mut comps_clone = Vec::with_capacity(comps.len());
            for c in comps.iter().rev() {
                comps_clone.push(c.clone());
            }
            self.command_buffer.push(EcsCommand::SpawnEntity(
                name_opt.map(|s| s.to_string()),
                comps_clone,
                eid,
            ));
            if self.transaction.is_some() {
                for component in comps.into_iter().rev() {
                    let cname = component.type_name.clone();
                    let summary = Self::component_summary(&component);
                    let _ = self.get_world_mut().add_component(eid, component);
                    self.record_causal_write(
                        Some(eid),
                        &cname,
                        crate::causality::WriteKind::Spawn,
                        summary,
                    );
                }
            }
        } else {
            for c in comps.into_iter().rev() {
                let cname = c.type_name.clone();
                let summary = Self::component_summary(&c);
                let _ = self.get_world_mut().add_component(eid, c);
                self.record_causal_write(
                    Some(eid),
                    &cname,
                    crate::causality::WriteKind::Spawn,
                    summary,
                );
            }
        }
        let __v = Value::from_entity_id(&mut self.gc, eid);
        self.push(__v);
        Ok(())
    }

    pub(crate) fn exec_init_resource(&mut self) -> Result<(), String> {
        let type_idx = self.read_u16()? as usize;
        let field_count = self.read_u16()? as usize;
        let type_name = helpers::constant_string(self.current_chunk(), type_idx)?.to_string();
        let layout = self
            .component_layouts
            .get(&type_name)
            .cloned()
            .unwrap_or_else(|| std::sync::Arc::new(Vec::new()));
        let mut values = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            values.push(self.pop()?);
        }
        values.reverse();
        let data = ComponentData {
            type_name: type_name.clone(),
            layout,
            values,
        };
        self.get_world_mut().init_resource(&type_name, data);
        Ok(())
    }
}
