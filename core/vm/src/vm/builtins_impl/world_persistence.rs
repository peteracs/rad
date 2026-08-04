const WORLD_CONTENT_DIGEST_DOMAIN: &[u8] = b"rad.world-content.v1";

impl VM {
    /// `save_world() -> str` — schema migration (#5), the save half.
    /// Serializes entities (names + components) and resources to JSON with
    /// the **schema embedded** (per-type field layout), using the tagged
    /// value codec for full fidelity. Pure given the world: persistence
    /// composes with io (`write_file(path, save_world())`), so record &
    /// replay needs no new machinery.
    pub(crate) fn bi_save_world(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("save_world() takes no arguments".into());
        }
        // A full world dump — the strongest bulk read there is.
        self.sandbox_check_bulk_read("save_world()")?;
        let body = self.save_world_body()?;
        // RADWORLD3 carries a blake3 integrity digest (`RADWORLD3 <digest>
        // <body>`, or the RADPACK1 envelope for large saves). There is one
        // current body shape; unsupported pre-release experiments are not a
        // compatibility surface.
        let out = crate::radpack::seal("RADWORLD3", &body);
        Ok(Value::from_string(&mut self.gc, out))
    }

    /// The canonical state-only serialization inside `RADWORLD3`.
    /// Shared by `save_world` (which envelopes it) and `world_digest`
    /// (which hashes it — keeping the digest independent of transport
    /// encoding decisions).
    fn save_world_body(&mut self) -> Result<String, String> {
        let skip = Arc::clone(&self.transient_resources);
        let versions = Arc::clone(&self.component_versions);
        Self::world_body_of(&self.world, &skip, &versions, true)
    }

    /// Canonical state-only body of ANY world — the live one for
    /// `save_world()` / `world_digest()`, or a fork's reconstruction for
    /// `world_digest(fork)` (the cross-version convergence certificate:
    /// a server digests the migrated view of a peer's world without
    /// committing it).
    /// `versions`: declared schema versions to embed per type in the schema
    /// section (`["T",["f"],2]`, dogfood seq 69). save_world passes the
    /// program's declarations; DIGEST callers pass an empty map — a version
    /// tag is load metadata, not state, so re-tagging a component must not
    /// change `world_digest()` (state identity survives a rolling upgrade).
    fn world_body_of(
        world: &crate::world::World,
        skip_resources: &std::collections::HashSet<String>,
        versions: &std::collections::HashMap<String, u32>,
        operational_relations: bool,
    ) -> Result<String, String> {
        // Direct string writer using the compact wire value codec shared with
        // `fork_to_bytes`.
        let mut schema: std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>> =
            std::collections::BTreeMap::new();
        fn write_data(
            schema: &mut std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>>,
            data: &crate::value::ComponentData,
            out: &mut String,
        ) -> Result<(), String> {
            let wire_layout = schema
                .entry(data.type_name.clone())
                .or_insert_with(|| data.layout.clone())
                .clone();
            crate::wire::escape_json_into(out, &data.type_name);
            out.push_str(",[");
            let aligned =
                std::sync::Arc::ptr_eq(&wire_layout, &data.layout) || *wire_layout == *data.layout;
            for i in 0..wire_layout.len() {
                if i > 0 {
                    out.push(',');
                }
                let v = if aligned {
                    &data.values[i]
                } else {
                    let f = &wire_layout[i];
                    let pos = data.layout.iter().position(|n| n == f).ok_or_else(|| {
                        format!(
                            "save_world: instances of '{}' disagree on field '{}'",
                            data.type_name, f
                        )
                    })?;
                    &data.values[pos]
                };
                crate::wire::encode_value_into(v, out)?;
            }
            out.push(']');
            Ok(())
        }

        let mut body = String::with_capacity(64 * 1024);
        body.push_str("{\"entities\":[");
        for (i, eid) in world.all_entity_ids().into_iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            use std::fmt::Write as _;
            let _ = write!(body, "{eid},");
            match world.entity_name(eid) {
                Some(name) => crate::wire::escape_json_into(&mut body, &name),
                None => body.push_str("null"),
            }
            body.push_str(",[");
            let mut comps = world.components_on_entity(eid);
            comps.sort_by(|a, b| a.type_name.cmp(&b.type_name));
            for (j, data) in comps.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                body.push('[');
                write_data(&mut schema, data, &mut body)?;
                body.push(']');
            }
            body.push_str("]]");
        }

        body.push_str("],\"resources\":[");
        let mut rnames = world.resource_names();
        rnames.sort();
        // transient resources are not part of the world's identity
        rnames.retain(|n| !skip_resources.contains(n));
        let mut first = true;
        for rname in rnames.iter() {
            if let Some(data) = world.get_resource(rname) {
                if !first {
                    body.push(',');
                }
                first = false;
                body.push('[');
                write_data(&mut schema, &data, &mut body)?;
                body.push(']');
            }
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
            body.push(']');
            // Optional third element: the declared schema version (only
            // nonzero versions are recorded, so undeclared programs emit
            // byte-identical saves).
            if let Some(v) = versions.get(tname.as_str()) {
                body.push(',');
                body.push_str(&v.to_string());
            }
            body.push(']');
        }
        body.push(']');
        if operational_relations {
            Self::append_authoritative_world_transport(world, &mut body)?;
        } else {
            body.push_str(",\"relation_content\":");
            crate::wire::escape_json_into(
                &mut body,
                &hex::encode(world.relation_state().semantic_content_bytes()),
            );
        }
        body.push('}');
        Ok(body)
    }

    /// blake3 of the canonical state-only serialization (`save_world` body).
    /// Excludes events, provenance, frame counters, and id free-lists — the
    /// convergence receipt for distributed sync: machines that merged to the
    /// same world print the same digest even though their fork bytes differ.
    /// Content digest of a frozen fork — same recipe as `world_digest`,
    /// usable wherever a snapshot needs a convergence fingerprint.
    pub(crate) fn fork_digest(
        snap: &std::sync::Arc<crate::world::WorldSnapshot>,
        skip_resources: &std::collections::HashSet<String>,
    ) -> Result<String, String> {
        let mut scratch = crate::world::World::new();
        scratch.restore((**snap).clone());
        let no_versions = std::collections::HashMap::new();
        let body = Self::world_body_of(&scratch, skip_resources, &no_versions, false)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(WORLD_CONTENT_DIGEST_DOMAIN);
        hasher.update(body.as_bytes());
        Ok(hasher.finalize().to_hex().to_string())
    }

    pub(crate) fn bi_world_digest(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() > 1 {
            return Err(format!(
                "world_digest() takes 0 arguments (live world) or 1 (a world_fork), got {}",
                args.len()
            ));
        }
        let body = match args.first() {
            None => {
                // No-arg form hashes the whole live world; the fork-arg form
                // hashes a fork the guest already holds, so only the former
                // is a world read. Version-free body (NOT save_world_body):
                // a version tag is load metadata, so world_digest() must
                // stay equal across peers that differ only in declared
                // versions — and equal to world_digest(fork) of the same
                // state.
                self.sandbox_check_bulk_read("world_digest()")?;
                let skip = Arc::clone(&self.transient_resources);
                let no_versions = std::collections::HashMap::new();
                Self::world_body_of(&self.world, &skip, &no_versions, false)?
            }
            Some(v) => {
                // `world_digest(fork)`: digest a fork's state without
                // committing it. Decoding and migration shape a peer's bytes
                // to this program's schema before comparison.
                let snap = v.as_world_fork().ok_or_else(|| {
                    format!(
                        "world_digest() argument must be a world_fork, got {}",
                        v.type_name()
                    )
                })?;
                let mut scratch = crate::world::World::new();
                scratch.restore((**snap).clone());
                let skip = Arc::clone(&self.transient_resources);
                let no_versions = std::collections::HashMap::new();
                Self::world_body_of(&scratch, &skip, &no_versions, false)?
            }
        };
        let mut hasher = blake3::Hasher::new();
        hasher.update(WORLD_CONTENT_DIGEST_DOMAIN);
        hasher.update(body.as_bytes());
        let digest = hasher.finalize().to_hex().to_string();
        Ok(Value::from_string(&mut self.gc, digest))
    }

    /// `schema_digest() -> str` — the PROGRAM's schema fingerprint: blake3
    /// of the declared component/resource/event layouts, sorted. Two peers
    /// with equal `schema_digest` may compare `world_digest` directly; when
    /// the fingerprints differ (a rolling migration), a raw digest mismatch
    /// means "different schema vintage", not "diverged" — certify through
    /// `world_digest(fork)` on the migrated view instead.
    pub(crate) fn bi_schema_digest(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("schema_digest() takes no arguments".into());
        }
        let digest = self.schema_digest_value();
        Ok(Value::from_string(&mut self.gc, digest))
    }

    pub(crate) fn schema_digest_value(&self) -> String {
        let mut names: Vec<&String> = self.component_layouts.keys().collect();
        names.sort();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"RADSCHEMA1");
        for name in names {
            hasher.update(b"\x1f");
            hasher.update(name.as_bytes());
            for field in self.component_layouts[name].iter() {
                hasher.update(b"\x1e");
                hasher.update(field.as_bytes());
            }
        }
        hasher.finalize().to_hex().to_string()
    }

    /// `load_world(json) -> int` — the load half of schema migration (#5).
    /// For each persisted component/resource: identical shape loads as-is
    /// (field order normalized); shape drift runs the declared
    /// `migrate X(old)` block (old fields as `map<str, any>`); drift with
    /// no migration is a loud error naming the added/removed fields.
    /// Returns the number of entities loaded.
    pub(crate) fn bi_load_world(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("load_world() requires 1 argument (the save_world() JSON)".into());
        }
        let text = args[0]
            .as_str()
            .ok_or_else(|| format!("load_world() expects str, got {}", args[0].type_name()))?
            .to_string();
        let text = crate::radpack::open(&text)
            .map_err(|e| format!("load_world(): {}", e))?
            .into_owned();
        if let Some(rest) = text.strip_prefix("RADWORLD3 ") {
            // `RADWORLD3 <blake3-of-body> <body>`: verify the integrity
            // envelope before trusting a byte of the payload. A packed save
            // is normalized to this same current representation first.
            let (claimed, body) = rest.split_once(' ').ok_or_else(|| {
                "load_world(): malformed RADWORLD3 envelope (missing digest separator)".to_string()
            })?;
            let actual = blake3::hash(body.as_bytes()).to_hex();
            if claimed != actual.as_str() {
                return Err(format!(
                    "load_world(): integrity digest mismatch (claimed {}…, computed {}…) — \
                     save corrupted or tampered",
                    crate::radpack::preview(claimed, 12),
                    &actual.as_str()[..12]
                ));
            }
            return self.load_world_body(body);
        }
        Err("load_world(): unsupported save format (expected RADWORLD3)".to_string())
    }}
