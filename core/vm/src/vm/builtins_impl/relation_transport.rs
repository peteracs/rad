struct TransportEntityAllocator {
    next_id: u32,
    exhausted: bool,
    free_ids: Vec<u32>,
    generations: Vec<(u32, u32)>,
}

impl TransportEntityAllocator {
    fn validate(
        self,
        live_ids: &[u32],
        operation: &str,
    ) -> Result<crate::world::ValidatedEntityAllocatorState, String> {
        crate::world::ValidatedEntityAllocatorState::try_new(
            self.next_id,
            self.exhausted,
            self.free_ids,
            self.generations,
            live_ids,
        )
        .map_err(|error| format!("{operation}: {error}"))
    }
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

    fn decode_transport_live_entity_ids(
        body: &serde_json::Value,
        operation: &str,
    ) -> Result<Vec<u32>, String> {
        body.get("entities")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| format!("{operation}: malformed or missing entity rows"))?
            .iter()
            .map(|entity| {
                let row = entity
                    .as_array()
                    .filter(|row| row.len() == 3)
                    .ok_or_else(|| format!("{operation}: malformed entity entry"))?;
                row[0]
                    .as_u64()
                    .and_then(|id| u32::try_from(id).ok())
                    .ok_or_else(|| format!("{operation}: entity ID out of range"))
            })
            .collect()
    }

    /// Prove the exact live/free/retired allocator partition before entity
    /// insertion can materialize a hostile sparse identity gap or duplicate
    /// a physical ECS row.
    fn decode_validated_transport_entity_allocator(
        body: &serde_json::Value,
        operation: &str,
    ) -> Result<crate::world::ValidatedEntityAllocatorState, String> {
        let allocator = Self::decode_transport_entity_allocator(body, operation)?;
        let live_ids = Self::decode_transport_live_entity_ids(body, operation)?;
        allocator.validate(&live_ids, operation)
    }

    fn restore_authoritative_world_transport_with_allocator(
        &self,
        world: &mut crate::world::World,
        body: &serde_json::Value,
        operation: &str,
        allocator: crate::world::ValidatedEntityAllocatorState,
    ) -> Result<(), String> {
        world.restore_validated_entity_allocator(allocator);

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
