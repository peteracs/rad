impl VM {

    /// `try_load_world(json) -> Result<int, str>` — the fallible sibling of
    /// `load_world()`. `load_world()` aborts on bad input, which made it the
    /// only deserialization entry point in the language that could not be
    /// handled in Rad — every other boundary (`fork_from_bytes`,
    /// `fork_apply`, `merge_forks`, `sandbox_run`, `json_parse`) returns a
    /// value (dogfood feature seq 69). This returns `Ok(entities_loaded)` or
    /// `Err(message)`, so an app can fall back to a prior backup when today's
    /// save is corrupt. `load_world` builds a replacement world and swaps it
    /// in only on success, so a failed load leaves the live world untouched —
    /// the property the fallback pattern depends on.
    fn bi_try_load_world(&mut self, args: Vec<Value>) -> Result<Value, String> {
        match self.bi_load_world(args) {
            Ok(v) => Ok(self.make_result(true, v)),
            Err(e) => {
                let msg = Value::from_string(&mut self.gc, e);
                Ok(self.make_result(false, msg))
            }
        }
    }

    /// Build the replacement world used by `load_world`.
    ///
    /// Saves carry entities and non-transient resources, but the program's
    /// schema-level runtime declarations live outside the file. Seed the fresh
    /// world with the current indexed-field declarations and declared resources
    /// so resource shape checks and transient resources remain available while
    /// loaded rows replace the entity set.
    fn load_world_replacement_target(&self) -> crate::world::World {
        let mut target = crate::world::World::new();
        target.set_indexed_fields_arc(Arc::clone(&self.indexed_decl));
        for rname in self.world.resource_names() {
            if let Some(data) = self.world.get_resource(&rname) {
                target.set_resource(&rname, data);
            }
        }
        target
    }

    /// Structural conformance of a deserialized value to a declared field
    /// type. Strict on scalars (a str is never an int, an int is never a
    /// bool, nil only satisfies nil-able types) with one deliberate
    /// exception: a float-declared field accepts an int — the checker
    /// allows that lossless direction at construction time, so well-formed
    /// worlds legitimately hold ints there. Types the checker cannot fully
    /// describe (Any, type variables, fn/task values that the wire codec
    /// refuses to encode anyway) validate permissively — the boundary must
    /// never wrongly reject a save the program legally produced.
    fn loaded_value_conforms(v: &Value, ty: &crate::types::Ty) -> bool {
        use crate::types::Ty;
        match ty {
            Ty::Int => v.as_int().is_some(),
            Ty::Float => v.as_float().is_some() || v.as_int().is_some(),
            Ty::Str => v.as_str().is_some(),
            Ty::Bool => v.as_bool().is_some(),
            Ty::Nil => v.is_nil(),
            Ty::EntityId => v.as_entity_id().is_some(),
            Ty::List(elem) => v
                .as_list()
                .is_some_and(|items| items.iter().all(|it| Self::loaded_value_conforms(it, elem))),
            Ty::Tuple(elems) => v.as_tuple().is_some_and(|items| {
                items.len() == elems.len()
                    && items
                        .iter()
                        .zip(elems)
                        .all(|(it, t)| Self::loaded_value_conforms(it, t))
            }),
            Ty::Map(kty, vty) => v.as_map().is_some_and(|m| {
                m.keys().all(|k| {
                    Self::loaded_map_key_conforms(k, kty) && Self::loaded_value_conforms(&m[k], vty)
                })
            }),
            Ty::Component(name) | Ty::Struct(name) => {
                v.as_component().is_some_and(|c| c.type_name == *name)
            }
            Ty::SumType(name) => v.as_sum_type().is_some_and(|st| st.type_name == *name),
            Ty::Union(alts) => alts.iter().any(|t| Self::loaded_value_conforms(v, t)),
            // Generic application: check the head name when the value
            // carries one; parameter checking would need instantiation.
            Ty::App(name, _) => {
                if let Some(st) = v.as_sum_type() {
                    st.type_name == *name
                } else if let Some(c) = v.as_component() {
                    c.type_name == *name
                } else {
                    true
                }
            }
            _ => true,
        }
    }

    fn loaded_map_key_conforms(k: &MapKey, ty: &crate::types::Ty) -> bool {
        use crate::types::Ty;
        match ty {
            Ty::Str => matches!(k, MapKey::Str(_)),
            Ty::Int => matches!(k, MapKey::Int(_)),
            Ty::Bool => matches!(k, MapKey::Bool(_)),
            Ty::EntityId => matches!(k, MapKey::Entity(_)),
            Ty::Tuple(elems) => match k {
                MapKey::Tuple(items) => {
                    items.len() == elems.len()
                        && items
                            .iter()
                            .zip(elems)
                            .all(|(i, t)| Self::loaded_map_key_conforms(i, t))
                }
                _ => false,
            },
            Ty::Union(alts) => alts.iter().any(|t| Self::loaded_map_key_conforms(k, t)),
            _ => true,
        }
    }

    /// Entity names are unique identity: `spawn` refuses to record an empty
    /// name and the name maps hold one id per name, so a well-formed save or
    /// fork payload can never carry `""` or a duplicate. Loading such a
    /// payload used to strip the name from one entity silently (the loser
    /// became unreachable via `get_entity`) — data loss with a success
    /// return. Refuse it, naming the collision.
    fn validate_loaded_entity_name(
        ctx: &str,
        name: Option<&str>,
        seen: &mut std::collections::HashSet<String>,
    ) -> Result<(), String> {
        let Some(n) = name else { return Ok(()) };
        if n.is_empty() {
            return Err(format!(
                "{}: payload contains an entity named \"\" — a live world cannot hold an \
                 empty name (unnamed entities are stored as null); the payload is corrupt",
                ctx
            ));
        }
        if !seen.insert(n.to_string()) {
            return Err(format!(
                "{}: payload contains two entities named '{}' — entity names are unique \
                 identity, and loading both would silently strip the name from one of them",
                ctx, n
            ));
        }
        Ok(())
    }

    /// Enforce declared field types on a row crossing the deserialization
    /// boundary (`load_world`, fork bytes, a delta, or a `migrate` block's
    /// return value). Shape drift already fails loudly; this closes the
    /// remaining hole where the field SET matches but a value's TYPE does
    /// not — which would otherwise plant wrong-typed data in fields the
    /// static checker trusts. Rows of types the checker never described
    /// (checker-less compiles) validate permissively.
    /// Sandbox write-shape enforcement (capability model, list item #1).
    ///
    /// The component-write ACL (`sandbox_check_write`) only gates the type
    /// *name*. That let a guest declare its own version of a granted
    /// component and write it with the wrong field types (poisoning a
    /// statically-typed host field), a different field name (positional
    /// aliasing — guest fields land in host columns by position), or an
    /// extra/short field set (silently dropped). None of the documented
    /// host-side defenses (peek/diff/assert_only_changed) can see any of it.
    ///
    /// This binds a granted write to the HOST's declared schema: the guest
    /// (whose own `component_field_types` was overwritten with the host's in
    /// `run_sandbox_guest`) must write the exact declared field set, with
    /// each value conforming to the declared type. Host-unknown components
    /// (no declared schema) are left alone — the host chose to grant that
    /// name, and there is no schema to bind against. No-op outside a sandbox.
    pub(crate) fn sandbox_check_write_shape(
        &self,
        data: &crate::value::ComponentData,
    ) -> Result<(), String> {
        if self.sandbox_caps.is_none() {
            return Ok(());
        }
        let Some(decl) = self.component_field_types.get(&data.type_name) else {
            return Ok(());
        };
        let declared: std::collections::HashSet<&str> =
            decl.iter().map(|(n, _)| n.as_str()).collect();
        let written: std::collections::HashSet<&str> =
            data.layout.iter().map(|s| s.as_str()).collect();
        if declared != written {
            let mut d: Vec<&str> = declared.into_iter().collect();
            d.sort_unstable();
            let mut w: Vec<&str> = written.into_iter().collect();
            w.sort_unstable();
            return Err(format!(
                "sandbox: write to component '{}' uses fields {:?}, but the host declares {:?} — \
                 a granted component must be written with the host's exact schema \
                 (guest field aliasing or field drift is rejected at the boundary)",
                data.type_name, w, d
            ));
        }
        for (field, value) in data.layout.iter().zip(&data.values) {
            let Some((_, ty)) = decl.iter().find(|(n, _)| n == field) else {
                continue;
            };
            if !Self::loaded_value_conforms(value, ty) {
                let shown = format!("{}", value);
                return Err(format!(
                    "sandbox: write to '{}.{}' has value {} ({}), but the host declares {} — \
                     refusing to plant wrong-typed data in a statically-typed field through a \
                     capability grant",
                    data.type_name,
                    field,
                    crate::radpack::preview(&shown, 48),
                    value.type_name(),
                    ty,
                ));
            }
        }
        Ok(())
    }

    fn validate_loaded_row(
        &self,
        ctx: &str,
        data: &crate::value::ComponentData,
    ) -> Result<(), String> {
        let Some(decl) = self.component_field_types.get(&data.type_name) else {
            return Ok(());
        };
        for (field, value) in data.layout.iter().zip(&data.values) {
            let Some((_, ty)) = decl.iter().find(|(n, _)| n == field) else {
                continue;
            };
            if !Self::loaded_value_conforms(value, ty) {
                let shown = format!("{}", value);
                return Err(format!(
                    "{}: type drift in '{}.{}': declared {}, loaded value is {} ({}) — \
                     refusing to plant wrong-typed data in a statically-typed field",
                    ctx,
                    data.type_name,
                    field,
                    ty,
                    value.type_name(),
                    crate::radpack::preview(&shown, 48),
                ));
            }
        }
        Ok(())
    }

    /// Decode the one current `RADWORLD3` body shape. Each component type gets
    /// one realization plan and each entity takes one archetype hop.
    fn load_world_body(&mut self, body_text: &str) -> Result<Value, String> {
        let body: serde_json::Value = serde_json::from_str(body_text)
            .map_err(|e| format!("load_world(): invalid JSON: {}", e))?;
        let allocator =
            Self::decode_validated_transport_entity_allocator(&body, "load_world()")?;

        let mut schema: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        // Optional third element per entry: the type's declared schema
        // version at save time (`component X v2`, dogfood seq 69) — handed
        // to `migrate X(old, from_version)`. Absent = 0.
        let mut schema_versions: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for entry in body["schema"].as_array().into_iter().flatten() {
            let pair = entry
                .as_array()
                .filter(|a| a.len() == 2 || a.len() == 3)
                .ok_or("load_world(): malformed schema entry")?;
            let tname = pair[0]
                .as_str()
                .ok_or("load_world(): malformed schema entry")?;
            let fields: Vec<String> = pair[1]
                .as_array()
                .ok_or("load_world(): malformed schema entry")?
                .iter()
                .filter_map(|f| f.as_str().map(String::from))
                .collect();
            if let Some(v) = pair.get(2).and_then(|v| v.as_i64()) {
                schema_versions.insert(tname.to_string(), v);
            }
            schema.insert(tname.to_string(), fields);
        }

        // Plan per type: aligned field sets decode positionally; drift runs
        // the `migrate` block per row.
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
        fn make_plan(stored: &[String], declared: std::sync::Arc<Vec<String>>) -> Plan {
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
        }
        let mut plans: std::collections::HashMap<String, Plan> = std::collections::HashMap::new();

        let mut target = self.load_world_replacement_target();
        let mut writes: Vec<(Option<u32>, String, crate::causality::WriteKind, String)> =
            Vec::new();
        let mut loaded = 0i64;
        let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for ent in body["entities"].as_array().into_iter().flatten() {
            let parts = ent
                .as_array()
                .filter(|a| a.len() == 3)
                .ok_or("load_world(): malformed entity entry")?;
            let saved_entity = parts[0]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or("load_world(): saved entity ID out of range")?;
            let name = parts[1].as_str();
            Self::validate_loaded_entity_name("load_world()", name, &mut seen_names)?;
            let comps_json = parts[2]
                .as_array()
                .ok_or("load_world(): malformed entity components")?;

            let mut comps: Vec<crate::value::ComponentData> = Vec::with_capacity(comps_json.len());
            for centry in comps_json {
                let cpair = centry
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("load_world(): malformed component entry")?;
                let cname = cpair[0]
                    .as_str()
                    .ok_or("load_world(): malformed component entry")?;
                let vals = cpair[1]
                    .as_array()
                    .ok_or("load_world(): malformed component values")?;

                if !plans.contains_key(cname) {
                    let stored = schema
                        .get(cname)
                        .ok_or_else(|| format!("load_world(): no schema entry for '{}'", cname))?;
                    let declared = self.component_layouts.get(cname).cloned().ok_or_else(|| {
                        format!(
                            "load_world(): save contains component '{}' which is not \
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
                                "load_world(): '{}' row has {} values, schema says {}",
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
                        self.validate_loaded_row("load_world()", &data)?;
                        data
                    }
                    Plan::Migrate { stored, declared } => {
                        let (stored, declared) = (stored.clone(), declared.clone());
                        let from_version = *schema_versions.get(cname).unwrap_or(&0);
                        self.migrate_wire_row(cname, &stored, declared, vals, from_version)?
                    }
                };
                comps.push(data);
            }

            if !target.insert_entity_with_components(saved_entity, name, comps.clone()) {
                return Err(format!("load_world(): duplicate entity ID {saved_entity}"));
            }
            let eid = saved_entity;
            loaded += 1;
            for data in &comps {
                writes.push((
                    Some(eid),
                    data.type_name.clone(),
                    crate::causality::WriteKind::Spawn,
                    Self::component_summary(data),
                ));
            }
        }

        for rentry in body["resources"].as_array().into_iter().flatten() {
            let rpair = rentry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("load_world(): malformed resource entry")?;
            let rname = rpair[0]
                .as_str()
                .ok_or("load_world(): malformed resource entry")?;
            let vals = rpair[1]
                .as_array()
                .ok_or("load_world(): malformed resource values")?;
            let stored = schema
                .get(rname)
                .ok_or_else(|| format!("load_world(): no schema entry for '{}'", rname))?;
            let declared = self
                .world
                .get_resource(rname)
                .map(|d| d.layout)
                .ok_or_else(|| {
                    format!(
                        "load_world(): save contains resource '{}' which is not declared \
                         in this program",
                        rname
                    )
                })?;
            let data = match make_plan(stored, declared) {
                Plan::Direct { declared, map } => {
                    if vals.len() != map.len() {
                        return Err(format!(
                            "load_world(): resource '{}' has {} values, schema says {}",
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
                    self.validate_loaded_row("load_world()", &data)?;
                    data
                }
                Plan::Migrate { stored, declared } => {
                    let from_version = *schema_versions.get(rname).unwrap_or(&0);
                    self.migrate_wire_row(rname, &stored, declared, vals, from_version)?
                }
            };
            let summary = Self::component_summary(&data);
            target.set_resource(rname, data);
            writes.push((
                None,
                rname.to_string(),
                crate::causality::WriteKind::Resource,
                summary,
            ));
        }

        self.restore_authoritative_world_transport_with_allocator(
            &mut target,
            &body,
            "load_world()",
            allocator,
        )?;
        self.world = target;
        for (entity, component, kind, summary) in writes {
            self.record_causal_write(entity, &component, kind, summary);
        }
        Ok(Value::from_int(&mut self.gc, loaded))
    }

    /// Run the declared `migrate` block for one row whose stored shape drifted
    /// from the declaration. Shared by world and fork decoding. `from_version` is the
    /// save's declared schema version for this type, bound to the optional
    /// second migrate parameter (dogfood seq 69 IDEA 03).
    fn migrate_loaded(
        &mut self,
        tname: &str,
        stored_pairs: Vec<(String, Value)>,
        declared: &std::sync::Arc<Vec<String>>,
        from_version: i64,
    ) -> Result<crate::value::ComponentData, String> {
        let Some(entry) = self.migrations.get(tname).copied() else {
            let stored_names: Vec<&String> = stored_pairs.iter().map(|(f, _)| f).collect();
            let added: Vec<&str> = declared
                .iter()
                .filter(|f| !stored_names.contains(f))
                .map(|f| f.as_str())
                .collect();
            let removed: Vec<&str> = stored_names
                .iter()
                .filter(|f| !declared.contains(f))
                .map(|f| f.as_str())
                .collect();
            return Err(format!(
                "load_world(): schema of '{}' changed (added: [{}], removed: [{}]) and no \
                 migration is declared — add `migrate {}(old) {{ return {} {{ ... }} }}`",
                tname,
                added.join(", "),
                removed.join(", "),
                tname,
                tname
            ));
        };

        let mut old_map = MapStorage::new();
        for (f, v) in stored_pairs {
            old_map.insert(MapKey::Str(f), v);
        }
        let old_value = Value::map(&mut self.gc, old_map);

        let result = self.run_migration_chunk(entry, old_value, from_version)?;
        let comp = result.as_component().ok_or_else(|| {
            format!(
                "migrate {}(old) must `return {} {{ ... }}`, got {}",
                tname,
                tname,
                result.type_name()
            )
        })?;
        if comp.type_name != tname {
            return Err(format!(
                "migrate {}(old) returned component '{}' — it must return '{}'",
                tname, comp.type_name, tname
            ));
        }
        // `old` binds persisted fields as map<str, any>, so the static
        // checker cannot see a wrong-typed migration result (the classic
        // mistake: grabbing the wrong old key). Enforce the declared field
        // types here, before the row becomes durable state.
        let ctx = format!("migrate {}(old)", tname);
        self.validate_loaded_row(&ctx, comp)?;
        Ok(comp.clone())
    }

    /// Invoke a compiled `migrate` chunk with the old-fields map (and the
    /// save's schema version, when the block declared a second parameter),
    /// returning the body's `return` value.
    fn run_migration_chunk(
        &mut self,
        entry: crate::vm::MigrationEntry,
        old_value: Value,
        from_version: i64,
    ) -> Result<Value, String> {
        let saved_depth = self.frames.len();
        let stack_base = self.stack.len();
        for _ in 0..entry.param_slot {
            self.push(Value::NIL);
        }
        self.push(old_value);
        if let Some(vslot) = entry.version_slot {
            // Pad any gap (defensive; in practice vslot == param_slot + 1),
            // then bind `from_version`.
            for _ in (entry.param_slot + 1)..vslot {
                self.push(Value::NIL);
            }
            let v = Value::from_int(&mut self.gc, from_version);
            self.push(v);
        }
        let frame_id = self.allocate_frame_id();
        self.frames.push(crate::vm::CallFrame {
            frame_id,
            chunk_id: entry.chunk_id,
            ip: 0,
            stack_base,
            captures: None,
            system_writeback: None,
        });
        // Migrations run mid-decode: the caller (fork_apply, fork_from_bytes,
        // load_world) holds already-decoded heap values in Rust locals the
        // collector cannot see. Auto-GC stays off for the duration.
        self.gc_pause += 1;
        let run = self
            .run_frames(saved_depth)
            .map_err(|error| error.to_string());
        self.gc_pause -= 1;
        run?;
        let result = self.pop()?;
        self.stack.truncate(stack_base);
        Ok(result)
    }

    /// `why(entity, Component) -> str` — causality query (#4): walks the
    /// provenance ledger from the last write to the component back through
    /// the handler→event→emitter chain.
    fn bi_why(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!("why() expects 2 arguments, got {}", args.len()));
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("why() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "why")?;
        // Provenance reveals a component's value history — a read, and a
        // richer one than `get`.
        self.sandbox_check_read(&ctype)?;
        let explanation = self.ledger.explain_entity(eid, &ctype, u64::MAX);
        Ok(Value::from_string(&mut self.gc, explanation))
    }

    /// `why_resource(Resource) -> str` — causality query for resources.
    fn bi_why_resource(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "why_resource() expects 1 argument, got {}",
                args.len()
            ));
        }
        let rtype = Self::expect_component_type_name(&args[0], "why_resource")?;
        self.sandbox_check_read(&rtype)?;
        let explanation = self.ledger.explain_resource(&rtype, u64::MAX);
        Ok(Value::from_string(&mut self.gc, explanation))
    }

    /// `diff(fork_a, fork_b) -> map<str, int>` — per-component changed-row
    /// counts between two forks (component/resource type name → rows, an
    /// upper bound). O(archetypes) `Arc::ptr_eq` comparisons on CoW columns,
    /// not a world scan.
    fn bi_diff(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!("diff() expects 2 arguments, got {}", args.len()));
        }
        let summary = {
            let a = args[0]
                .as_world_fork()
                .ok_or_else(|| "diff() first argument must be a world_fork".to_string())?;
            let b = args[1]
                .as_world_fork()
                .ok_or_else(|| "diff() second argument must be a world_fork".to_string())?;
            crate::world::WorldSnapshot::diff_summary(a, b)
        };
        let mut m = MapStorage::new();
        for (name, count) in summary {
            let v = Value::from_int(&mut self.gc, count as i64);
            m.insert(MapKey::Str(name), v);
        }
        Ok(Value::map(&mut self.gc, m))
    }

    /// `assert_only_changed(fork_a, fork_b, allowed)` — the negative-space
    /// assertion: errors unless every difference between the two forks is in
    /// the `allowed` component list (component type refs or name strings).
    ///
    /// This is only possible because the language owns 100% of program state:
    /// "nothing else in the universe changed" is a checkable sentence.
    fn bi_assert_only_changed(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "assert_only_changed() expects 3 arguments, got {}",
                args.len()
            ));
        }
        let allowed: std::collections::HashSet<String> = args[2]
            .as_list()
            .ok_or_else(|| {
                "assert_only_changed() third argument must be a list of component types".to_string()
            })?
            .iter()
            .map(|v| Self::expect_component_type_name(v, "assert_only_changed"))
            .collect::<Result<_, _>>()?;
        let summary = {
            let a = args[0].as_world_fork().ok_or_else(|| {
                "assert_only_changed() first argument must be a world_fork".to_string()
            })?;
            let b = args[1].as_world_fork().ok_or_else(|| {
                "assert_only_changed() second argument must be a world_fork".to_string()
            })?;
            crate::world::WorldSnapshot::diff_summary(a, b)
        };
        let unexpected: Vec<String> = summary
            .iter()
            .filter(|(name, _)| !allowed.contains(*name))
            .map(|(name, rows)| format!("{} ({} rows)", name, rows))
            .collect();
        if !unexpected.is_empty() {
            let mut allowed_sorted: Vec<&String> = allowed.iter().collect();
            allowed_sorted.sort();
            return Err(format!(
                "assert_only_changed() failed: unexpected changes to [{}] (allowed: [{}])",
                unexpected.join(", "),
                allowed_sorted
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Ok(Value::NIL)
    }

    /// `sandbox_input() -> any` — the data-only input handed to this guest by
    /// the host (`nil` when none was provided or outside a sandbox).
    fn bi_sandbox_input(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!(
                "sandbox_input() takes no arguments, got {}",
                args.len()
            ));
        }
        match self.sandbox_input_json.take() {
            Some(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|e| format!("sandbox_input(): host sent invalid JSON: {}", e))?;
                let v = json_to_value(&mut self.gc, &parsed)?;
                // Re-arm so repeated calls keep working.
                self.sandbox_input_json = Some(text);
                Ok(v)
            }
            None => Ok(Value::NIL),
        }
    }

    /// `sandbox_output(v)` — report a structured, data-only result to the
    /// host. The value is serialized to JSON immediately, so nothing from the
    /// guest heap survives past the guest VM. Last call wins.
    fn bi_sandbox_output(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "sandbox_output() expects 1 argument, got {}",
                args.len()
            ));
        }
        let j = value_to_json(&args[0], 0)
            .map_err(|e| format!("sandbox_output() value is not data-only: {}", e))?;
        self.sandbox_output_json = Some(j.to_string());
        Ok(Value::NIL)
    }

    /// `sandbox_last_output() -> any | nil` — the structured value the most
    /// recent `sandbox_run` guest reported via `sandbox_output(v)`, parsed
    /// back onto the host heap (the same data-only JSON boundary as
    /// `sandbox_input`, in reverse), or `nil` if the guest emitted none. This
    /// closes the in-language gap where `run_sandbox_guest` computed the
    /// guest's output and `sandbox_run` discarded it (dogfood feature seq 62),
    /// leaving a Rad host unable to read a plugin's typed result without
    /// forcing it to WRITE state just to communicate.
    fn bi_sandbox_last_output(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!(
                "sandbox_last_output() takes no arguments, got {}",
                args.len()
            ));
        }
        match &self.last_sandbox_output_json {
            Some(text) => {
                let parsed: serde_json::Value = serde_json::from_str(text).map_err(|e| {
                    format!(
                        "sandbox_last_output(): stored guest output is invalid JSON: {}",
                        e
                    )
                })?;
                json_to_value(&mut self.gc, &parsed)
            }
            None => Ok(Value::NIL),
        }
    }

    /// `sandbox_last_fuel() -> int` — fuel consumed by the most recent
    /// `sandbox_run` (charge points crossed: loop back-edges and calls), or 0
    /// before any run. The metering signal a plugin host bills or rate-limits
    /// on; also computed-then-discarded before seq 62.
    fn bi_sandbox_last_fuel(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!(
                "sandbox_last_fuel() takes no arguments, got {}",
                args.len()
            ));
        }
        Ok(Value::from_int(
            &mut self.gc,
            self.last_sandbox_fuel_spent as i64,
        ))
    }
}
