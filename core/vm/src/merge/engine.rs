

impl std::fmt::Display for MergeConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeConflict::Field {
                entity,
                entity_name,
                component,
                field,
                base,
                ours,
                theirs,
            } => write!(
                f,
                "{}: {}.{} changed in both forks (base {}, ours {}, theirs {})",
                entity_label(*entity, entity_name),
                component,
                field,
                base,
                ours,
                theirs
            ),
            MergeConflict::Component {
                entity,
                entity_name,
                component,
                detail,
            } => write!(
                f,
                "{}: {} {}",
                entity_label(*entity, entity_name),
                component,
                detail
            ),
            MergeConflict::Despawn {
                entity,
                entity_name,
                detail,
            } => write!(f, "{}: {}", entity_label(*entity, entity_name), detail),
            MergeConflict::Rename {
                entity,
                base,
                ours,
                theirs,
            } => write!(
                f,
                "entity id {}: renamed in both forks (base {:?}, ours {:?}, theirs {:?})",
                entity, base, ours, theirs
            ),
            MergeConflict::NameClaim { name, entities } => write!(
                f,
                "name '{}' claimed by {} entities after merge (ids {:?}) — names are identity",
                name,
                entities.len(),
                entities
            ),
            MergeConflict::ResourceField {
                resource,
                field,
                base,
                ours,
                theirs,
            } => write!(
                f,
                "resource {}: {}.{} changed in both forks (base {}, ours {}, theirs {})",
                resource, resource, field, base, ours, theirs
            ),
            MergeConflict::Resource { resource, detail } => {
                write!(f, "resource {}: {}", resource, detail)
            }
            MergeConflict::Events {
                detail,
                base,
                ours,
                theirs,
            } => write!(
                f,
                "in-flight events: {} consumed or reordered events still pending in base \
                 ({} base, {} ours, {} theirs) — flush_events() before forking, or merge by hand",
                detail, base, ours, theirs
            ),
            MergeConflict::Relations { detail } => {
                write!(f, "authoritative relations: {detail}")
            }
        }
    }
}

/// Resolution tables for `merge_forks_with`.
///
/// `fields` maps `(entity-or-resource, component, field)` to the value the
/// merged world should carry; `None` in the first slot addresses a
/// resource. Resolves [`MergeConflict::Field`] / [`MergeConflict::ResourceField`].
///
/// `renames` maps an entity id to the name it should carry in the merged
/// world (`None` = unnamed). Resolves [`MergeConflict::NameClaim`]: names
/// are semantic identity, so the *machine* never picks — but a human (or a
/// policy) saying "keep both, as T-5/a and T-5/b" is a complete answer, and
/// refusing to accept it would make name claims the one conflict kind that
/// stays unresolvable forever. Renames are applied to merged outcomes
/// before the claim check, which then re-validates: if the chosen names
/// still collide (with each other, or with an untouched base entity), the
/// claim comes back as a conflict rather than silently stealing a name.
///
/// Despawn and event conflicts remain unresolvable: "did those handlers
/// run?" has no honest pick-a-side.
#[derive(Default)]
pub struct Resolutions {
    pub(crate) fields: HashMap<(Option<u32>, String, String), Value>,
    pub renames: HashMap<u32, Option<String>>,
}

pub struct MergeOutcome {
    pub world: World,
    /// (theirs id, merged id) for entities remapped due to id collisions.
    pub remapped: Vec<(u32, u32)>,
    /// Merged in-flight event queue: base's pending events, then ours'
    /// post-fork emissions, then theirs' (reference-rewritten). Never
    /// silently dropped — that was the composition hole.
    pub(crate) events: Vec<(String, Value, u64)>,
    /// Causality emit ids, parallel to `events`.
    pub emit_ids: Vec<u64>,
    /// Merged delayed (`emit … after`) timers: unchanged sides defer to
    /// the changed one; both changed differently is a conflict (timers
    /// age, so prefix logic does not apply).
    pub(crate) delayed: Vec<(i64, String, Value, u64)>,
}

#[derive(Clone)]
struct EntityState {
    name: Option<String>,
    comps: BTreeMap<String, ComponentData>,
}

/// One entity's full state in one world, or `None` when it is not alive
/// there. Materialized only for entities the CoW scan flagged as touched —
/// merge cost is proportional to the divergence, not the world size.
fn state_of(w: &World, eid: u32) -> Option<EntityState> {
    if !w.contains_entity(eid) {
        return None;
    }
    let comps = w
        .components_on_entity(eid)
        .into_iter()
        .map(|c| (c.type_name.clone(), c))
        .collect();
    Some(EntityState {
        name: w.entity_name(eid),
        comps,
    })
}

fn rewrite_state(state: &mut EntityState, remap: &HashMap<u32, u32>, alloc: &mut dyn Allocator) {
    for comp in state.comps.values_mut() {
        Value::rewrite_component_entity_ids(comp, remap, alloc);
    }
}

fn entity_label(eid: u32, name: &Option<String>) -> String {
    match name {
        Some(n) => format!("entity '{}' (id {})", n, eid),
        None => format!("entity id {}", eid),
    }
}

/// Field-level three-way merge of one component present in all three states.
/// `subject` is `Some((eid, name))` for an entity component, `None` for a
/// resource. A both-sides divergence consults `resolutions` before becoming
/// a conflict — that lookup is what makes merge policies programmable.
fn merge_component(
    subject: Option<(u32, &Option<String>)>,
    base: &ComponentData,
    ours: &ComponentData,
    theirs: &ComponentData,
    conflicts: &mut Vec<MergeConflict>,
    resolutions: &Resolutions,
) -> ComponentData {
    if base.layout != ours.layout || base.layout != theirs.layout {
        // Cannot happen for forks of one program run; refuse rather than guess.
        let detail = "has different field layouts across forks".to_string();
        conflicts.push(match subject {
            Some((eid, name)) => MergeConflict::Component {
                entity: eid,
                entity_name: name.clone(),
                component: ours.type_name.clone(),
                detail,
            },
            None => MergeConflict::Resource {
                resource: ours.type_name.clone(),
                detail,
            },
        });
        return ours.clone();
    }
    let res_key = subject.map(|(eid, _)| eid);
    let mut merged = ours.clone();
    for (i, field) in ours.layout.iter().enumerate() {
        let b = &base.values[i];
        let o = &ours.values[i];
        let t = &theirs.values[i];
        match (o != b, t != b) {
            (_, false) => {}                        // theirs untouched: ours stands
            (false, true) => merged.values[i] = *t, // only theirs changed
            (true, true) => {
                if o != t {
                    if let Some(v) =
                        resolutions
                            .fields
                            .get(&(res_key, ours.type_name.clone(), field.clone()))
                    {
                        merged.values[i] = *v;
                        continue;
                    }
                    conflicts.push(match subject {
                        Some((eid, name)) => MergeConflict::Field {
                            entity: eid,
                            entity_name: name.clone(),
                            component: ours.type_name.clone(),
                            field: field.clone(),
                            base: *b,
                            ours: *o,
                            theirs: *t,
                        },
                        None => MergeConflict::ResourceField {
                            resource: ours.type_name.clone(),
                            field: field.clone(),
                            base: *b,
                            ours: *o,
                            theirs: *t,
                        },
                    });
                }
            }
        }
    }
    merged
}

/// Component-set merge for an entity alive in base, ours, and theirs.
fn merge_entity_components(
    eid: u32,
    ename: &Option<String>,
    base: &EntityState,
    ours: &EntityState,
    theirs: &EntityState,
    conflicts: &mut Vec<MergeConflict>,
    resolutions: &Resolutions,
) -> BTreeMap<String, ComponentData> {
    let mut names: BTreeSet<&String> = BTreeSet::new();
    names.extend(base.comps.keys());
    names.extend(ours.comps.keys());
    names.extend(theirs.comps.keys());

    let component_conflict = |cname: &str, detail: &str| MergeConflict::Component {
        entity: eid,
        entity_name: ename.clone(),
        component: cname.to_string(),
        detail: detail.to_string(),
    };

    let mut merged = BTreeMap::new();
    for cname in names {
        let b = base.comps.get(cname);
        let o = ours.comps.get(cname);
        let t = theirs.comps.get(cname);
        match (b, o, t) {
            (Some(b), Some(o), Some(t)) => {
                merged.insert(
                    cname.clone(),
                    merge_component(Some((eid, ename)), b, o, t, conflicts, resolutions),
                );
            }
            // Removed in one fork; the other must not have touched it.
            (Some(b), None, Some(t)) => {
                if t == b {
                    // clean removal by ours
                } else {
                    conflicts.push(component_conflict(
                        cname,
                        "removed in ours but modified in theirs",
                    ));
                }
            }
            (Some(b), Some(o), None) => {
                if o == b {
                    // clean removal by theirs
                } else {
                    conflicts.push(component_conflict(
                        cname,
                        "removed in theirs but modified in ours",
                    ));
                }
            }
            (Some(_), None, None) => {} // removed in both
            // Added post-fork.
            (None, Some(o), None) => {
                merged.insert(cname.clone(), o.clone());
            }
            (None, None, Some(t)) => {
                merged.insert(cname.clone(), t.clone());
            }
            (None, Some(o), Some(t)) => {
                if o == t {
                    merged.insert(cname.clone(), o.clone());
                } else {
                    conflicts.push(component_conflict(
                        cname,
                        "added in both forks with different values (ours vs theirs)",
                    ));
                }
            }
            (None, None, None) => unreachable!(),
        }
    }
    merged
}

/// Three-way name merge for a base entity.
fn merge_name(
    eid: u32,
    base: &Option<String>,
    ours: &Option<String>,
    theirs: &Option<String>,
    conflicts: &mut Vec<MergeConflict>,
) -> Option<String> {
    match (ours != base, theirs != base) {
        (_, false) => ours.clone(),
        (false, true) => theirs.clone(),
        (true, true) => {
            if ours == theirs {
                ours.clone()
            } else {
                conflicts.push(MergeConflict::Rename {
                    entity: eid,
                    base: base.clone().unwrap_or_default(),
                    ours: ours.clone().unwrap_or_default(),
                    theirs: theirs.clone().unwrap_or_default(),
                });
                ours.clone()
            }
        }
    }
}

pub(crate) fn merge_worlds(
    base_snap: &WorldSnapshot,
    ours_snap: &WorldSnapshot,
    theirs_snap: &WorldSnapshot,
    alloc: &mut dyn Allocator,
    resolutions: &Resolutions,
) -> Result<MergeOutcome, Vec<MergeConflict>> {
    let base_relations = base_snap
        .relation_state()
        .operational_checkpoint_bytes();
    let ours_relations = ours_snap
        .relation_state()
        .operational_checkpoint_bytes();
    let theirs_relations = theirs_snap
        .relation_state()
        .operational_checkpoint_bytes();
    if base_relations != ours_relations || base_relations != theirs_relations {
        return Err(vec![MergeConflict::Relations {
            detail: "branch relation states differ; assertion-aware merge is not implemented"
                .into(),
        }]);
    }

    // Restores are CoW-cheap; the worlds are read-only views here.
    let mut wb = World::new();
    wb.restore(base_snap.clone());
    let mut wo = World::new();
    wo.restore(ours_snap.clone());
    let mut wt = World::new();
    wt.restore(theirs_snap.clone());

    // ---- 0. CoW scan: which entities did either fork actually touch?
    // Shared-lineage forks answer by Arc comparison in O(divergence); forks
    // that crossed a process boundary (rebuilt worlds) fall back to a full
    // scan. Everything downstream operates on this set only.
    let touched: BTreeSet<u32> = match (
        WorldSnapshot::touched_entities(base_snap, ours_snap),
        WorldSnapshot::touched_entities(base_snap, theirs_snap),
    ) {
        (Some(a), Some(b)) => a.into_iter().chain(b).collect(),
        _ => {
            let mut all: BTreeSet<u32> = wb.all_entity_ids().into_iter().collect();
            all.extend(wo.all_entity_ids());
            all.extend(wt.all_entity_ids());
            all
        }
    };

    // ---- 1. Id remap plan: spawns colliding on a handle are different
    // logical entities; theirs moves to fresh ids. (Spawns are always in
    // the touched set: a spawn changes its archetype's row list.)
    let mut fresh = wb
        .max_live_entity_id()
        .into_iter()
        .chain(wo.max_live_entity_id())
        .chain(wt.max_live_entity_id())
        .max()
        .map_or(0, |m| m + 1);
    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut remapped: Vec<(u32, u32)> = Vec::new();
    for &eid in &touched {
        let is_spawn = !wb.contains_entity(eid) && wt.contains_entity(eid);
        if is_spawn && wo.contains_entity(eid) {
            remap.insert(eid, fresh);
            remapped.push((eid, fresh));
            fresh += 1;
        }
    }
    let reverse_remap: HashMap<u32, u32> = remap.iter().map(|(&o, &f)| (f, o)).collect();

    // ---- 2/3. Per-entity three-way merge, over touched ids only — an
    // entity neither fork touched is byte-identical to base everywhere and
    // needs no decision. Theirs' states are rewritten into merged id-space
    // before comparison; an untouched entity cannot reference a post-fork
    // spawn (referencing one is a write), so sparse rewriting is sound.
    let mut conflicts: Vec<MergeConflict> = Vec::new();
    let mut merge_ids: BTreeSet<u32> = touched;
    merge_ids.extend(remap.values().copied());

    // eid -> Some(state) = alive in merged world, None = despawned.
    let mut merged_entities: BTreeMap<u32, Option<EntityState>> = BTreeMap::new();
    for &eid in &merge_ids {
        let b = state_of(&wb, eid);
        let o = state_of(&wo, eid);
        let t = {
            // A remapped id's original handle no longer belongs to theirs;
            // its state reappears under the fresh id.
            let raw = if let Some(&orig) = reverse_remap.get(&eid) {
                state_of(&wt, orig)
            } else if remap.contains_key(&eid) {
                None
            } else {
                state_of(&wt, eid)
            };
            raw.map(|mut st| {
                if !remap.is_empty() {
                    rewrite_state(&mut st, &remap, alloc);
                }
                st
            })
        };
        match (b.as_ref(), o.as_ref(), t.as_ref()) {
            // Alive everywhere: field-level merge.
            (Some(b), Some(o), Some(t)) => {
                let name = merge_name(eid, &b.name, &o.name, &t.name, &mut conflicts);
                let comps =
                    merge_entity_components(eid, &b.name, b, o, t, &mut conflicts, resolutions);
                merged_entities.insert(eid, Some(EntityState { name, comps }));
            }
            // Despawned in one fork: the other must not have touched it.
            (Some(b), None, Some(t)) => {
                if t.comps == b.comps && t.name == b.name {
                    merged_entities.insert(eid, None); // clean despawn by ours
                } else {
                    conflicts.push(MergeConflict::Despawn {
                        entity: eid,
                        entity_name: b.name.clone(),
                        detail: "despawned in ours but modified in theirs".to_string(),
                    });
                }
            }
            (Some(b), Some(o), None) => {
                if o.comps == b.comps && o.name == b.name {
                    merged_entities.insert(eid, None); // clean despawn by theirs
                } else {
                    conflicts.push(MergeConflict::Despawn {
                        entity: eid,
                        entity_name: b.name.clone(),
                        detail: "despawned in theirs but modified in ours".to_string(),
                    });
                }
            }
            (Some(_), None, None) => {
                merged_entities.insert(eid, None); // despawned in both
            }
            // Post-fork spawns. Collisions were remapped, so at most one
            // fork owns any spawn id here.
            (None, Some(_), None) => {
                merged_entities.insert(eid, o);
            }
            (None, None, Some(_)) => {
                merged_entities.insert(eid, t);
            }
            (None, Some(_), Some(_)) => unreachable!("spawn collisions are remapped"),
            (None, None, None) => {} // e.g. original handle of a remapped spawn
        }
    }

    // ---- 3b. Rename resolutions: a human's (or a policy's) answer to a
    // name claim — "keep both, as T-5/a and T-5/b". Applied to merged
    // outcomes *before* the claim check below, which then re-validates:
    // a rename can resolve a claim, but if the chosen names still collide
    // (with each other, or with an untouched base owner) the claim comes
    // back as a conflict instead of silently stealing a name.
    if !resolutions.renames.is_empty() {
        for (&eid, new_name) in &resolutions.renames {
            let normalized = new_name.clone().filter(|n| !n.is_empty());
            match merged_entities.get_mut(&eid) {
                Some(Some(st)) => st.name = normalized,
                Some(None) => {} // renaming a despawned entity claims nothing
                None => {
                    // An entity neither fork touched (e.g. the untouched
                    // base owner in a claim): materialize it so the rename
                    // lands in the apply step.
                    if let Some(mut st) = state_of(&wb, eid) {
                        st.name = normalized;
                        merged_entities.insert(eid, Some(st));
                    }
                    // Unknown id: nothing it could claim; whatever conflict
                    // prompted the rename simply persists below.
                }
            }
        }
        // The human decided these names; both-forks-renamed conflicts on
        // the same entities are answered by the same decision.
        conflicts.retain(|c| match c {
            MergeConflict::Rename { entity, .. } => !resolutions.renames.contains_key(entity),
            _ => true,
        });
    }

    // ---- 4. Names are semantic identity: one owner per name after merge.
    // Untouched entities keep their base names, so a new collision always
    // involves at least one merged outcome — check those against each other
    // and against surviving untouched base owners.
    {
        let mut owners: BTreeMap<&String, Vec<u32>> = BTreeMap::new();
        for (&eid, st) in &merged_entities {
            if let Some(n) = st.as_ref().and_then(|s| s.name.as_ref()) {
                owners.entry(n).or_default().push(eid);
            }
        }
        for (name, ids) in owners {
            if ids.len() > 1 {
                conflicts.push(MergeConflict::NameClaim {
                    name: name.clone(),
                    entities: ids,
                });
            } else if let Some(&base_owner) = base_snap.name_to_id.get(name) {
                // The base owner still claims `name` unless the merge (or a
                // rename resolution, which may have materialized an entity
                // the forks never touched) decided its name for it.
                if base_owner != ids[0]
                    && !merge_ids.contains(&base_owner)
                    && !merged_entities.contains_key(&base_owner)
                {
                    conflicts.push(MergeConflict::NameClaim {
                        name: name.clone(),
                        entities: vec![base_owner.min(ids[0]), base_owner.max(ids[0])],
                    });
                }
            }
        }
    }

    // ---- 5. Resources: same field-level three-way rules. (Few per
    // program; a full pass costs nothing.)
    let mut merged_resources: BTreeMap<String, ComponentData> = BTreeMap::new();
    {
        let mut rnames: BTreeSet<String> = BTreeSet::new();
        rnames.extend(wb.resource_names());
        rnames.extend(wo.resource_names());
        rnames.extend(wt.resource_names());
        for rname in &rnames {
            let b = wb.get_resource(rname);
            let o = wo.get_resource(rname);
            let t = wt.get_resource(rname).map(|mut d| {
                if !remap.is_empty() {
                    Value::rewrite_component_entity_ids(&mut d, &remap, alloc);
                }
                d
            });
            match (b.as_ref(), o.as_ref(), t.as_ref()) {
                (Some(b), Some(o), Some(t)) => {
                    merged_resources.insert(
                        rname.clone(),
                        merge_component(None, b, o, t, &mut conflicts, resolutions),
                    );
                }
                (_, Some(o), None) => {
                    merged_resources.insert(rname.clone(), o.clone());
                }
                (_, None, Some(t)) => {
                    merged_resources.insert(rname.clone(), t.clone());
                }
                (None, Some(o), Some(t)) => {
                    if o == t {
                        merged_resources.insert(rname.clone(), o.clone());
                    } else {
                        conflicts.push(MergeConflict::Resource {
                            resource: rname.clone(),
                            detail: "initialized in both forks with different values".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    // ---- 5b. In-flight events. Emission is append-only within a fork, so
    // base's queue must be a prefix of each fork's queue; anything else
    // means a fork flushed (consumed) events the other still carries, and
    // there is no honest automatic answer to "did those handlers run?".
    let mut merged_events: Vec<(String, Value, u64)> = Vec::new();
    let mut merged_emit_ids: Vec<u64> = Vec::new();
    {
        fn is_prefix(prefix: &[(String, Value, u64)], full: &[(String, Value, u64)]) -> bool {
            full.len() >= prefix.len()
                && prefix
                    .iter()
                    .zip(full)
                    .all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2 == b.2)
        }
        let base_ev = base_snap.events.as_ref();
        let ours_ev = ours_snap.events.as_ref();
        let theirs_ev = theirs_snap.events.as_ref();
        let ours_ok = is_prefix(base_ev, ours_ev);
        let theirs_ok = is_prefix(base_ev, theirs_ev);
        if !ours_ok || !theirs_ok {
            conflicts.push(MergeConflict::Events {
                detail: match (ours_ok, theirs_ok) {
                    (false, false) => "both forks",
                    (false, true) => "ours",
                    _ => "theirs",
                }
                .to_string(),
                base: base_ev.len(),
                ours: ours_ev.len(),
                theirs: theirs_ev.len(),
            });
        } else {
            merged_events.extend(base_ev.iter().cloned());
            merged_emit_ids.extend(base_snap.emit_ids.iter().copied());
            merged_events.extend(ours_ev[base_ev.len()..].iter().cloned());
            merged_emit_ids.extend(ours_snap.emit_ids.iter().skip(base_ev.len()).copied());
            // Theirs' post-fork emissions may reference remapped spawns.
            for (name, payload, tid) in &theirs_ev[base_ev.len()..] {
                let rewritten = payload.rewrite_entity_ids(&remap, alloc);
                merged_events.push((name.clone(), rewritten, *tid));
            }
            merged_emit_ids.extend(theirs_snap.emit_ids.iter().skip(base_ev.len()).copied());
        }
    }

    // ---- 5c. Delayed timers. They age (ticks_left decrements), so the
    // append-only prefix rule cannot apply; the honest rule is two-sided:
    // a side equal to base defers to the other, equal sides agree, and
    // both-changed-differently has no automatic answer.
    let mut merged_delayed: Vec<(i64, String, Value, u64)> = Vec::new();
    {
        let base_d = base_snap.delayed.as_ref();
        let ours_d = ours_snap.delayed.as_ref();
        let theirs_d = theirs_snap.delayed.as_ref();
        let eq = |a: &[(i64, String, Value, u64)], b: &[(i64, String, Value, u64)]| {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| x.0 == y.0 && x.1 == y.1 && x.2 == y.2 && x.3 == y.3)
        };
        if eq(ours_d, base_d) {
            // theirs' payloads may reference remapped spawns
            for (left, name, payload, emit_id) in theirs_d {
                let rewritten = payload.rewrite_entity_ids(&remap, alloc);
                merged_delayed.push((*left, name.clone(), rewritten, *emit_id));
            }
        } else if eq(theirs_d, base_d) || eq(ours_d, theirs_d) {
            merged_delayed.extend(ours_d.iter().cloned());
        } else {
            conflicts.push(MergeConflict::Events {
                detail: "delayed timers diverged in both forks".to_string(),
                base: base_d.len(),
                ours: ours_d.len(),
                theirs: theirs_d.len(),
            });
        }
    }

    if !conflicts.is_empty() {
        return Err(conflicts);
    }

    // ---- 6. Canonical apply, in deterministic order, through World's own
    // operations so every invariant (archetypes, indices, free ids, name
    // maps) is maintained by the engine rather than by merge code. Starts
    // from base (CoW) and touches only merged outcomes — O(divergence).
    let mut work = World::new();
    work.restore(base_snap.clone());

    // Despawns first: their ids may be re-claimed by nothing, but their
    // names must be free before renames/spawns.
    for (&eid, st) in &merged_entities {
        if st.is_none() && !work.destroy_entity(eid) {
            return Err(vec![MergeConflict::Relations {
                detail: format!(
                    "entity {eid} cannot be deleted while an authoritative relation restricts it"
                ),
            }]);
        }
    }

    for (&eid, st) in &merged_entities {
        let Some(st) = st else { continue };
        let base_state = state_of(&wb, eid);
        if base_state.is_none() {
            if let Err(error) = work.insert_entity_with_id(eid, st.name.as_deref()) {
                return Err(vec![MergeConflict::Relations {
                    detail: format!("cannot install merged entity {eid}: {error}"),
                }]);
            }
        } else if base_state.as_ref().map(|b| &b.name) != Some(&st.name) {
            work.set_entity_name(eid, st.name.as_deref());
        }
        let base_comps = base_state.as_ref().map(|b| &b.comps);
        // Removals, then writes, both in sorted component order.
        if let Some(bc) = base_comps {
            for cname in bc.keys() {
                if !st.comps.contains_key(cname) {
                    work.remove_component(eid, cname);
                }
            }
        }
        for (cname, data) in &st.comps {
            let unchanged = base_comps
                .and_then(|bc| bc.get(cname))
                .is_some_and(|b| b == data);
            if !unchanged {
                work.add_component(eid, data.clone());
            }
        }
    }

    for (rname, data) in &merged_resources {
        let unchanged = wb.get_resource(rname).is_some_and(|b| &b == data);
        if !unchanged {
            work.set_resource(rname, data.clone());
        }
    }

    Ok(MergeOutcome {
        world: work,
        remapped,
        events: merged_events,
        emit_ids: merged_emit_ids,
        delayed: merged_delayed,
    })
}
