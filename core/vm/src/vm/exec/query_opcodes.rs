// Set-valued world reads: archetype queries and the filter/project stages
// that narrow them.

impl VM {

    pub(crate) fn exec_ecs_query(&mut self) -> Result<(), String> {
        let with_count = self.read_byte()? as usize;
        let without_count = self.read_byte()? as usize;

        let mut with_types = Vec::with_capacity(with_count);
        for _ in 0..with_count {
            let v = self.pop()?;
            let s = v
                .as_str()
                .ok_or("EcsQuery: expected string component type")?
                .to_string();
            with_types.push(s);
        }
        with_types.reverse();

        let mut without_types = Vec::with_capacity(without_count);
        for _ in 0..without_count {
            let v = self.pop()?;
            let s = v
                .as_str()
                .ok_or("EcsQuery: expected string component type")?
                .to_string();
            without_types.push(s);
        }
        without_types.reverse();

        // A `query { A } without { B }` reveals which entities have A and lack
        // B, so both sides are reads and both honor the read ACL. No-op unless
        // the grant carries an explicit `"read"` allowlist.
        if self.sandbox_caps.is_some() {
            for ctype in with_types.iter().chain(without_types.iter()) {
                self.sandbox_check_read(ctype)?;
            }
        }

        let eids = self.get_world().query(&with_types, &without_types);
        let list = eids
            .into_iter()
            .map(|eid| Value::from_entity_id(&mut self.gc, eid))
            .collect();
        self.push_list_vec(list);
        Ok(())
    }

    pub(crate) fn exec_query_filter(&mut self) -> Result<(), String> {
        let comp_count = self.read_byte()? as usize;
        let filter_val = self.pop()?;
        let (filter_chunk_id, captures_arc) = if let Some(cv) = filter_val.as_closure() {
            (cv.chunk_id, Some(Arc::new(cv.captures.clone())))
        } else if let Some(fv) = filter_val.as_fn() {
            (fv.chunk_id, None)
        } else {
            return Err("QueryFilter: expected closure or function".to_string());
        };
        let mut comp_types = Vec::with_capacity(comp_count);
        for _ in 0..comp_count {
            let v = self.pop()?;
            let s = v
                .as_str()
                .ok_or("QueryFilter: expected string component type")?
                .to_string();
            comp_types.push(s);
        }
        comp_types.reverse();
        let entity_list = self.pop()?;
        let entities = entity_list
            .into_rad_list()
            .ok_or("QueryFilter: expected entity list")?
            .into_vec();

        let mut result = Vec::new();
        for entity_val in entities.into_iter() {
            let eid = entity_val
                .as_entity_id()
                .ok_or("QueryFilter: expected entity id")?;

            let saved_depth = self.frames.len();
            let stack_base = self.stack.len();

            self.push(entity_val);
            for ctype in &comp_types {
                if let Some(comp) = self.get_world().get_component(eid, ctype) {
                    let __v = Value::from_component_data(&mut self.gc, comp);
                    self.push(__v);
                } else {
                    self.push(Value::NIL);
                }
            }

            let frame_id = self.allocate_frame_id();
            self.frames.push(CallFrame {
                frame_id,
                chunk_id: filter_chunk_id,
                ip: 0,
                stack_base,
                captures: captures_arc.clone(),
                system_writeback: None,
            });
            self.run_frames(saved_depth)
                .map_err(|error| error.to_string())?;

            let keep = self.pop()?.is_truthy();
            self.stack.truncate(stack_base);

            if keep {
                result.push(entity_val);
            }
        }
        self.push_list_vec(result);
        Ok(())
    }

    pub(crate) fn exec_query_project(&mut self) -> Result<(), String> {
        let select_count = self.read_byte()? as usize;
        let mut select_types = Vec::with_capacity(select_count);
        for _ in 0..select_count {
            let v = self.pop()?;
            let s = v
                .as_str()
                .ok_or("QueryProject: expected string component type")?
                .to_string();
            select_types.push(s);
        }
        select_types.reverse();
        let entity_list = self.pop()?;
        let entities = entity_list
            .into_rad_list()
            .ok_or("QueryProject: expected entity list")?
            .into_vec();

        let mut result = Vec::new();
        for entity_val in entities {
            let eid = entity_val
                .as_entity_id()
                .ok_or("QueryProject: expected entity id")?;

            if select_types.len() == 1 {
                if let Some(comp) = self.get_world().get_component(eid, &select_types[0]) {
                    result.push(Value::from_component_data(&mut self.gc, comp));
                } else {
                    result.push(Value::NIL);
                }
            } else {
                let mut fields = Vec::with_capacity(select_types.len());
                for ctype in &select_types {
                    if let Some(comp) = self.get_world().get_component(eid, ctype) {
                        fields.push(Value::from_component_data(&mut self.gc, comp));
                    } else {
                        fields.push(Value::NIL);
                    }
                }
                result.push(Value::tuple(&mut self.gc, fields));
            }
        }
        self.push_list_vec(result);
        Ok(())
    }
}
