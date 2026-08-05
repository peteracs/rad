struct TransportEntityAllocator {
    next_id: u32,
    exhausted: bool,
    free_ids: Vec<u32>,
    generations: Vec<(u32, u32)>,
}

impl VM {
    fn append_authoritative_world_transport(
        world: &crate::world::World,
        out: &mut String,
    ) -> Result<(), String> {
        use std::fmt::Write as _;
        let (next_id, exhausted, free_ids) = world.allocator_state();
        let _ = write!(
            out,
            ",\"entity_allocator\":[{},{},[",
            next_id,
            if exhausted { "true" } else { "false" }
        );
        for (index, free) in free_ids.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(out, "{free}");
        }
        out.push_str("],[");
        for (index, (slot, generation)) in world.generation_entries().iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(out, "[{slot},{generation}]");
        }
        out.push_str("]],\"relations\":");
        match world.relation_state().transport_hex() {
            Some(encoded) => crate::wire::escape_json_into(out, &encoded),
            None => out.push_str("null"),
        }
        Ok(())
    }

    fn decode_transport_entity_allocator(
        body: &serde_json::Value,
        operation: &str,
    ) -> Result<TransportEntityAllocator, String> {
        let allocator = body
            .get("entity_allocator")
            .and_then(serde_json::Value::as_array)
            .filter(|values| values.len() == 4)
            .ok_or_else(|| format!("{operation}: malformed or missing entity allocator"))?;
        let next_id = allocator[0]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| format!("{operation}: entity next_id out of range"))?;
        let exhausted = allocator[1]
            .as_bool()
            .ok_or_else(|| format!("{operation}: malformed exhaustion flag"))?;
        let free_ids = allocator[2]
            .as_array()
            .ok_or_else(|| format!("{operation}: malformed entity free list"))?
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| format!("{operation}: free entity ID out of range"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let generations = allocator[3]
            .as_array()
            .ok_or_else(|| format!("{operation}: malformed generation table"))?
            .iter()
            .map(|entry| {
                let pair = entry
                    .as_array()
                    .filter(|pair| pair.len() == 2)
                    .ok_or_else(|| format!("{operation}: malformed generation entry"))?;
                let slot = pair[0]
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| format!("{operation}: generation slot out of range"))?;
                let generation = pair[1]
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| format!("{operation}: generation out of range"))?;
                Ok((slot, generation))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(TransportEntityAllocator {
            next_id,
            exhausted,
            free_ids,
            generations,
        })
    }

    /// Validate allocator accounting before entity insertion can materialize
    /// a hostile sparse identity gap as billions of free-list entries.
    fn decode_validated_transport_entity_allocator(
        body: &serde_json::Value,
        operation: &str,
    ) -> Result<TransportEntityAllocator, String> {
        let allocator = Self::decode_transport_entity_allocator(body, operation)?;
        let entities = body["entities"].as_array();
        let live_count = entities.map_or(0, Vec::len) as u64;
        let issued_count = if allocator.exhausted {
            u64::from(u32::MAX) + 1
        } else {
            u64::from(allocator.next_id)
        };
        let accounted_count = live_count.saturating_add(allocator.free_ids.len() as u64);
        if issued_count > accounted_count {
            return Err(format!(
                "{operation}: allocator claims {issued_count} ids issued but the payload \
                 accounts for only {accounted_count} ({live_count} live + {} free)",
                allocator.free_ids.len()
            ));
        }

        for entity in entities.into_iter().flatten() {
            let Some(id) = entity
                .as_array()
                .and_then(|parts| parts.first())
                .and_then(serde_json::Value::as_u64)
            else {
                continue;
            };
            if id >= issued_count {
                return Err(format!(
                    "{operation}: entity id {id} is outside the allocator range \
                     ({issued_count} ids issued)"
                ));
            }
        }
        Ok(allocator)
    }

    fn restore_authoritative_world_transport_with_allocator(
        &self,
        world: &mut crate::world::World,
        body: &serde_json::Value,
        operation: &str,
        allocator: TransportEntityAllocator,
    ) -> Result<(), String> {
        world
            .set_id_allocator_state(
                allocator.next_id,
                allocator.exhausted,
                allocator.free_ids,
            )
            .map_err(|error| format!("{operation}: {error}"))?;
        world
            .restore_generation_entries(allocator.generations)
            .map_err(|error| format!("{operation}: {error}"))?;

        let relations = body
            .get("relations")
            .ok_or_else(|| format!("{operation}: payload omits authoritative relation state"))?;
        match (self.world.relation_state().manifest().cloned(), relations) {
            (None, serde_json::Value::Null) => {}
            (None, _) => {
                return Err(format!(
                    "{operation}: payload carries relations but this program has no relation manifest"
                ))
            }
            (Some(_), serde_json::Value::Null) => {
                return Err(format!(
                    "{operation}: payload has no relations for the installed relation manifest"
                ))
            }
            (Some(manifest), serde_json::Value::String(encoded)) => world
                .restore_relation_transport(encoded, manifest)
                .map_err(|error| format!("{operation}: {error}"))?,
            (Some(_), _) => {
                return Err(format!("{operation}: malformed authoritative relation state"))
            }
        }
        Ok(())
    }
}
