/// Canonical allocator state whose issued identity universe has been proven
/// to be an exact, disjoint live/free/retired partition.
pub(crate) struct ValidatedEntityAllocatorState {
    next_id: u32,
    fresh_ids_exhausted: bool,
    free_ids: Arc<BTreeSet<u32>>,
    generations: Arc<HashMap<u32, u32>>,
}

/// Stable, typed failures from the entity identity allocator and its storage
/// boundary. Public host APIs return these instead of panicking or collapsing
/// allocator corruption into a boolean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityAllocationError {
    IdSpaceExhausted,
    AllocatorLiveFreeOverlap(u32),
    FreshIdAlreadyLive(u32),
    ArchetypeDuplicate(u32),
    IdAlreadyLive(u32),
    GenerationExhausted(u32),
    ExplicitIdNotReusable(u32),
    ExplicitIdGapTooLarge {
        start: u32,
        requested: u32,
        limit: u32,
    },
}

impl EntityAllocationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::IdSpaceExhausted => "entity.id_space_exhausted",
            Self::AllocatorLiveFreeOverlap(_) => "entity.allocator_live_free_overlap",
            Self::FreshIdAlreadyLive(_) => "entity.allocator_fresh_id_is_live",
            Self::ArchetypeDuplicate(_) => "entity.archetype_duplicate",
            Self::IdAlreadyLive(_) => "entity.id_already_live",
            Self::GenerationExhausted(_) => "entity.generation_exhausted",
            Self::ExplicitIdNotReusable(_) => "entity.explicit_id_not_reusable",
            Self::ExplicitIdGapTooLarge { .. } => "entity.explicit_id_gap_limit",
        }
    }
}

impl std::fmt::Display for EntityAllocationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())?;
        match self {
            Self::AllocatorLiveFreeOverlap(entity)
            | Self::FreshIdAlreadyLive(entity)
            | Self::ArchetypeDuplicate(entity)
            | Self::IdAlreadyLive(entity)
            | Self::GenerationExhausted(entity)
            | Self::ExplicitIdNotReusable(entity) => write!(formatter, ": entity {entity}"),
            Self::ExplicitIdGapTooLarge {
                start,
                requested,
                limit,
            } => write!(
                formatter,
                ": requested entity {requested} from fresh cursor {start}, limit {limit}"
            ),
            Self::IdSpaceExhausted => Ok(()),
        }
    }
}

impl std::error::Error for EntityAllocationError {}

// Explicit identities exist only for internal fork merging and delta
// reconstruction. A fixed bound prevents one integer from materializing an
// attacker-sized free set; full world/fork restoration bypasses gap inference
// and installs a separately validated allocator partition instead.
const MAX_EXPLICIT_ENTITY_GAP: u32 = 65_536;

impl ValidatedEntityAllocatorState {
    pub(crate) fn try_new(
        next_id: u32,
        fresh_ids_exhausted: bool,
        free_ids: Vec<u32>,
        generations: Vec<(u32, u32)>,
        live_ids: &[u32],
    ) -> Result<Self, String> {
        if fresh_ids_exhausted && next_id != u32::MAX {
            return Err(format!(
                "id allocator: exhausted fresh identity space requires next_id {}, got {next_id}",
                u32::MAX
            ));
        }
        let issued_count = if fresh_ids_exhausted {
            u64::from(u32::MAX) + 1
        } else {
            u64::from(next_id)
        };
        let in_issued_range = |id: u32| u64::from(id) < issued_count;

        let mut previous_live = None;
        for &id in live_ids {
            if let Some(previous) = previous_live {
                if previous == id {
                    return Err(format!("id allocator: duplicate live entity ID {id}"));
                }
                if previous > id {
                    return Err("id allocator: live entity IDs are not strictly ascending".into());
                }
            }
            if !in_issued_range(id) {
                return Err(format!(
                    "id allocator: live entity ID {id} is outside the issued range"
                ));
            }
            previous_live = Some(id);
        }

        let mut previous_free = None;
        for &id in &free_ids {
            if let Some(previous) = previous_free {
                if previous == id {
                    return Err(format!("id allocator: duplicate free entity ID {id}"));
                }
                if previous > id {
                    return Err("id allocator: free entity IDs are not strictly ascending".into());
                }
            }
            if !in_issued_range(id) {
                return Err(format!(
                    "id allocator: free entity ID {id} is outside the issued range"
                ));
            }
            previous_free = Some(id);
        }

        let mut previous_generation_slot = None;
        for &(slot, generation) in &generations {
            if let Some(previous) = previous_generation_slot {
                if previous == slot {
                    return Err(format!("id allocator: duplicate generation slot {slot}"));
                }
                if previous > slot {
                    return Err("id allocator: generation slots are not strictly ascending".into());
                }
            }
            if generation == 0 {
                return Err(format!(
                    "id allocator: generation slot {slot} stores noncanonical zero"
                ));
            }
            if !in_issued_range(slot) {
                return Err(format!(
                    "id allocator: generation slot {slot} is outside the issued range"
                ));
            }
            previous_generation_slot = Some(slot);
        }

        let (mut live_cursor, mut free_cursor) = (0, 0);
        while live_cursor < live_ids.len() && free_cursor < free_ids.len() {
            match live_ids[live_cursor].cmp(&free_ids[free_cursor]) {
                std::cmp::Ordering::Less => live_cursor += 1,
                std::cmp::Ordering::Greater => free_cursor += 1,
                std::cmp::Ordering::Equal => {
                    return Err(format!(
                        "id allocator: entity ID {} is both live and free",
                        live_ids[live_cursor]
                    ));
                }
            }
        }

        let mut retired_count = 0u64;
        let (mut live_cursor, mut free_cursor) = (0, 0);
        for &(slot, generation) in &generations {
            while live_ids.get(live_cursor).is_some_and(|id| *id < slot) {
                live_cursor += 1;
            }
            while free_ids.get(free_cursor).is_some_and(|id| *id < slot) {
                free_cursor += 1;
            }
            let live = live_ids.get(live_cursor) == Some(&slot);
            let free = free_ids.get(free_cursor) == Some(&slot);
            if free && generation == u32::MAX {
                return Err(format!(
                    "id allocator: generation-exhausted slot {slot} must be retired, not free"
                ));
            }
            if live || free {
                continue;
            }
            if generation != u32::MAX {
                return Err(format!(
                    "id allocator: generation slot {slot} is neither live, free, nor retired"
                ));
            }
            retired_count += 1;
        }
        let accounted_count = u64::try_from(live_ids.len())
            .ok()
            .and_then(|count| count.checked_add(u64::try_from(free_ids.len()).ok()?))
            .and_then(|count| count.checked_add(retired_count))
            .ok_or_else(|| "id allocator: partition accounting overflow".to_string())?;
        if accounted_count != issued_count {
            return Err(format!(
                "id allocator claims {issued_count} identities issued but the canonical partition \
                 accounts for {accounted_count} ({} live + {} free + {retired_count} retired)",
                live_ids.len(),
                free_ids.len()
            ));
        }

        Ok(Self {
            next_id,
            fresh_ids_exhausted,
            free_ids: Arc::new(free_ids.into_iter().collect()),
            generations: Arc::new(generations.into_iter().collect()),
        })
    }
}

impl World {
    fn set_entity_generation(&mut self, entity: u32, generation: u32) {
        let generations = Arc::make_mut(&mut self.generations);
        if generation == 0 {
            generations.remove(&entity);
        } else {
            generations.insert(entity, generation);
        }
    }

    pub fn spawn_entity(&mut self, name: Option<&str>) -> Result<u32, EntityAllocationError> {
        let empty_archetype = self.archetype_map.get(&Vec::new()).copied();
        let has_physical_row = |world: &World, entity| {
            empty_archetype.is_some_and(|aid| {
                world.archetypes[aid as usize]
                    .entity_row
                    .contains_key(&entity)
            })
        };
        let (eid, generation) = loop {
            let reusable = self.free_ids.first().copied();
            match reusable {
                Some(reused) => {
                    if self.entity_archetype.contains_key(&reused) {
                        return Err(EntityAllocationError::AllocatorLiveFreeOverlap(reused));
                    }
                    if has_physical_row(self, reused) {
                        return Err(EntityAllocationError::ArchetypeDuplicate(reused));
                    }
                    Arc::make_mut(&mut self.free_ids).pop_first();
                    let previous = self.generations.get(&reused).copied().unwrap_or(0);
                    if let Some(generation) = previous.checked_add(1) {
                        break (reused, generation);
                    }
                    // Defense in depth for a corrupted trusted snapshot:
                    // canonical worlds retire this slot at destruction and
                    // transport rejects it in the free set.
                }
                None => {
                    if self.fresh_ids_exhausted {
                        return Err(EntityAllocationError::IdSpaceExhausted);
                    }
                    let fresh = self.next_id;
                    if self.entity_archetype.contains_key(&fresh) {
                        return Err(EntityAllocationError::FreshIdAlreadyLive(fresh));
                    }
                    if has_physical_row(self, fresh) {
                        return Err(EntityAllocationError::ArchetypeDuplicate(fresh));
                    }
                    if fresh == u32::MAX {
                        self.fresh_ids_exhausted = true;
                    } else {
                        self.next_id = fresh + 1;
                    }
                    break (fresh, 0);
                }
            }
        };
        self.set_entity_generation(eid, generation);
        let aid = self.get_or_create_archetype(Vec::new());
        self.archetypes[aid as usize].push_entity(eid, HashMap::new())?;
        Arc::make_mut(&mut self.entity_archetype).insert(eid, aid);
        if let Some(n) = name {
            if !n.is_empty() {
                if let Some(old_eid) =
                    Arc::make_mut(&mut self.name_to_id).insert(n.to_string(), eid)
                {
                    Arc::make_mut(&mut self.id_to_name).remove(&old_eid);
                }
                Arc::make_mut(&mut self.id_to_name).insert(eid, n.to_string());
            }
        }
        Ok(eid)
    }

    pub fn entity_ref(&self, eid: u32) -> Option<crate::relation_runtime::EntityRef> {
        self.entity_exists(eid)
            .then(|| crate::relation_runtime::EntityRef {
                slot: eid,
                generation: self.generations.get(&eid).copied().unwrap_or(0),
            })
    }

    pub(crate) fn generation_entries(&self) -> Vec<(u32, u32)> {
        let mut entries = self
            .generations
            .iter()
            .map(|(slot, generation)| (*slot, *generation))
            .collect::<Vec<_>>();
        entries.sort_unstable();
        entries
    }

    pub(crate) fn allocator_state(&self) -> (u32, bool, Vec<u32>) {
        let free_ids = self.free_ids.iter().copied().collect();
        (self.next_id, self.fresh_ids_exhausted, free_ids)
    }

    pub(crate) fn restore_validated_entity_allocator(
        &mut self,
        state: ValidatedEntityAllocatorState,
    ) {
        self.next_id = state.next_id;
        self.fresh_ids_exhausted = state.fresh_ids_exhausted;
        self.free_ids = state.free_ids;
        self.generations = state.generations;
    }

    /// Claim a caller-selected identity while preserving allocator state for
    /// bounded internal delta and merge reconstruction. Full world/fork
    /// decoders install a sealed allocator partition without gap inference.
    fn claim_explicit_entity_id(
        &mut self,
        eid: u32,
    ) -> Result<u32, EntityAllocationError> {
        if self.entity_archetype.contains_key(&eid) {
            return Err(EntityAllocationError::IdAlreadyLive(eid));
        }
        let generation = self
            .generations
            .get(&eid)
            .copied()
            .map_or(Some(0), |generation| generation.checked_add(1))
            .ok_or(EntityAllocationError::GenerationExhausted(eid))?;
        if self.fresh_ids_exhausted {
            if !Arc::make_mut(&mut self.free_ids).remove(&eid) {
                return Err(EntityAllocationError::ExplicitIdNotReusable(eid));
            }
        } else if eid >= self.next_id {
            let gap = eid - self.next_id;
            if gap > MAX_EXPLICIT_ENTITY_GAP {
                return Err(EntityAllocationError::ExplicitIdGapTooLarge {
                    start: self.next_id,
                    requested: eid,
                    limit: MAX_EXPLICIT_ENTITY_GAP,
                });
            }
            Arc::make_mut(&mut self.free_ids).extend(self.next_id..eid);
            if eid == u32::MAX {
                self.fresh_ids_exhausted = true;
            } else {
                self.next_id = eid + 1;
            }
        } else {
            if !Arc::make_mut(&mut self.free_ids).remove(&eid) {
                return Err(EntityAllocationError::ExplicitIdNotReusable(eid));
            }
        }
        self.set_entity_generation(eid, generation);
        Ok(generation)
    }
}
