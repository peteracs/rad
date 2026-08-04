

// ---------------------------------------------------------------------------
// sandpaper: literals default-fill
// ---------------------------------------------------------------------------

/// Fields whose declarations carry defaults may be omitted from literals —
/// the constructor fills them from the declaration. (Bare type annotations
/// like `x: float` have no default and stay required.)
#[test]
fn component_literals_default_fill_omitted_fields() {
    let vm = run(r#"
        component Incident { title: "", priority: 0, age: 0, status: "open" }
        component Escort { guard: entity | nil = nil }
        resource Board { open: 0, label: "ops" }

        let i = spawn("inc", Incident { title: "disk full" })
        set(i, Escort {})
        set_resource(Board, Board { open: 3 })

        let v = get(i, Incident) |> unwrap
        let e = get(i, Escort) |> unwrap
        let b = get_resource(Board) |> unwrap
        print(f"{v.title}|{v.priority}|{v.status}")
        print(e.guard == nil)
        print(f"{b.open}|{b.label}")
    "#);
    assert_eq!(vm.print_buffer, vec!["disk full|0|open", "true", "3|ops"]);
}

// ---------------------------------------------------------------------------
// causality retention window (long-running processes)
// ---------------------------------------------------------------------------

/// The ledger is a window, not an archive: old records evict, emit ids stay
/// stable, the commit seam survives, and why() discloses the truncation
/// instead of presenting a partial ledger as the whole truth.
#[test]
fn causality_retention_evicts_honestly() {
    use crate::causality::{CausalityLedger, Cause, WriteKind};
    let mut ledger = CausalityLedger::default();
    ledger.set_retention_cap(3);

    for i in 0..10 {
        ledger.record_write(
            0,
            Some(i),
            Some(format!("e{}", i)),
            "Pos",
            format!("{{ x: {} }}", i),
            WriteKind::Set,
            Cause::Main,
        );
        let id = ledger.record_emit(0, "Tick", format!("Tick {{ n: {} }}", i), Cause::Main);
        assert_eq!(
            id,
            i as u64 + 1,
            "emit ids must stay monotonic across eviction"
        );
    }

    // Recent records still explain normally.
    let recent = ledger.explain_entity(9, "Pos", u64::MAX);
    assert!(recent.contains("Pos of e9"), "got: {}", recent);

    // Evicted records say so instead of guessing.
    let evicted = ledger.explain_entity(0, "Pos", u64::MAX);
    assert!(evicted.contains("no recorded write"), "got: {}", evicted);
    assert!(
        evicted.contains("evicted by the retention window"),
        "got: {}",
        evicted
    );

    // Commit watermarks are absolute: a commit recorded now still orders
    // correctly against surviving writes after future evictions.
    ledger.record_commit(0);
    let seam = ledger.explain_entity(9, "Pos", u64::MAX);
    assert!(seam.contains("note: commit()"), "got: {}", seam);
}

// ---------------------------------------------------------------------------
// blast-radius × merge
// ---------------------------------------------------------------------------

/// diff() composes with merge: the merged fork's distance from base is
/// exactly the union of both branches' edits — and assert_only_changed
/// fences a merge the same way it fences any other mutation.
#[test]
fn merge_result_is_diffable_and_fenceable() {
    let vm = run(r#"
        component Gold { amount: 0 }
        component Pos { x: 0 }
        let bank = spawn("bank", Gold { amount: 100 })
        let rock = spawn("rock", Pos { x: 1 })
        let base = fork()

        set(bank, Gold { amount: 150 })
        let ours = fork()

        commit(base)
        set(rock, Pos { x: 5 })
        let theirs = fork()

        commit(base)
        let merged = merge_forks(base, ours, theirs) |> unwrap
        let d = diff(base, merged)
        print(d["Gold"])
        print(d["Pos"])
        commit(merged)
        let g = get(bank, Gold) |> unwrap
        let p = get(rock, Pos) |> unwrap
        print(g.amount)
        print(p.x)
    "#);
    assert_eq!(vm.print_buffer, vec!["1", "1", "150", "5"]);
}

// ---------------------------------------------------------------------------
// conflicts as data × programmable resolution
// ---------------------------------------------------------------------------

/// A merge conflict is a value: user code destructures it and gets the
/// entity handle, component, field, and all three diverging values — then
/// uses the entity handle *as an entity* (looks up another component on it).
/// No string parsing anywhere.
#[test]
fn conflicts_destructure_into_usable_values() {
    let vm = run(r#"
        component Ticket { status: "open" }
        component Meta { team: "" }
        let t = spawn("T-1", Ticket { status: "open" }, Meta { team: "infra" })
        let base = fork()

        set(t, Ticket { status: "closed" })
        let ours = fork()

        commit(base)
        set(t, Ticket { status: "escalated" })
        let theirs = fork()

        commit(base)
        match merge_forks(base, ours, theirs) {
            Ok(_) => { print("merged?!") }
            Err(conflicts) => {
                for c in conflicts {
                    match c {
                        FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                            let m = get(ent, Meta) |> unwrap
                            print(f"{name} ({m.team}): {comp}.{field} ours={ours} theirs={theirs}")
                        }
                        _ => { print("unexpected kind") }
                    }
                }
            }
        }
    "#);
    assert_eq!(
        vm.print_buffer,
        vec!["T-1 (infra): Ticket.status ours=closed theirs=escalated"]
    );
}

/// The acceptance test of the round-table: a sync policy written **in rad**.
/// merge_forks reports conflicts; user code decides per field (status
/// precedence: closed beats escalated beats open); merge_forks_with applies
/// the decisions and merges clean. Unresolved fields still conflict.
#[test]
fn merge_forks_with_applies_a_policy_written_in_rad() {
    let vm = run(r#"
        component Ticket { status: "open", assignee: "" }
        let t = spawn("T-1", Ticket { status: "open", assignee: "" })
        let base = fork()

        update(t, Ticket) { status = "closed", assignee = "mei" }
        let ours = fork()

        commit(base)
        update(t, Ticket) { status = "escalated", assignee = "raj" }
        let theirs = fork()

        commit(base)

        // The policy, in rad: status merges by precedence; assignee
        // conflicts defer to theirs (the pusher wins assignment).
        fn rank(s: str) -> int {
            if s == "closed" { return 3 }
            if s == "escalated" { return 2 }
            return 1
        }

        match merge_forks(base, ours, theirs) {
            Ok(m) => { commit(m) }
            Err(conflicts) => {
                let mut decisions = []
                for c in conflicts {
                    match c {
                        FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                            if field == "status" {
                                let mut pick = ours
                                if rank(theirs) > rank(ours) { pick = theirs }
                                decisions = push(decisions, (c, pick))
                            }
                            if field == "assignee" {
                                decisions = push(decisions, (c, theirs))
                            }
                        }
                        _ => {}
                    }
                }
                let m = merge_forks_with(base, ours, theirs, decisions) |> unwrap
                commit(m)
            }
        }

        let tk = get(t, Ticket) |> unwrap
        print(f"{tk.status} {tk.assignee}")
    "#);
    assert_eq!(vm.print_buffer, vec!["closed raj"]);
}

/// Resolutions only cover what they name: an unresolved field conflict
/// still refuses, structural conflicts cannot be resolved at all.
#[test]
fn merge_forks_with_leaves_unnamed_conflicts_and_rejects_structural() {
    let vm = run(r#"
        component Gold { amount: 0 }
        let hero = spawn("hero", Gold { amount: 10 })
        let base = fork()
        set(hero, Gold { amount: 1 })
        let ours = fork()
        commit(base)
        set(hero, Gold { amount: 2 })
        let theirs = fork()
        commit(base)

        // Empty decisions: same conflict comes back.
        match merge_forks_with(base, ours, theirs, []) {
            Ok(_) => { print("merged?!") }
            Err(conflicts) => { print(f"still {len(conflicts)} conflict") }
        }
    "#);
    assert_eq!(vm.print_buffer, vec!["still 1 conflict"]);
}

// ---------------------------------------------------------------------------
// causality × wire: provenance rides the fork payload
// ---------------------------------------------------------------------------

/// The cross-machine why(): machine B creates an entity and leaves an event
/// in flight; machine A ingests the fork, merges, commits, flushes. Asking A
/// "why does this value exist?" must answer with B's history — spawn origin,
/// the handler chain through B's emit — labeled with the wire origin instead
/// of pretending the value has no past. This is the query that failed in the
/// first syncdesk smoke run.
#[test]
fn why_answers_across_the_machine_seam() {
    let shared = r#"
        component Ticket { status: "open" }
        component Notes { n: 0 }
        event NoteAdded { who }
        on NoteAdded(e) {
            let cur = get(e.who, Notes) |> unwrap
            set(e.who, Notes { n: cur.n + 1 })
        }
    "#;

    // Machine B: spawn a ticket, emit (but don't flush) a note event.
    let b_src = format!(
        r#"{shared}
        let t = spawn("T-9", Ticket {{ status: "open" }}, Notes {{ n: 0 }})
        emit NoteAdded {{ who: t }}
        print(fork_to_bytes(fork()))
    "#
    );
    let vm_b = run(&b_src);
    let bytes = vm_b.print_buffer[0].clone();

    // Machine A: empty world, ingest, commit, flush — then ask why.
    let a_src = format!(
        r#"{shared}
        let theirs = fork_from_bytes({bytes:?}) |> unwrap
        commit(theirs)
        flush_events()
        let t = get_entity("T-9")
        print(why(t, Ticket))
        print(why(t, Notes))
    "#
    );
    let vm_a = run(&a_src);

    // The spawn that A never performed answers with B's record + origin.
    let ticket = &vm_a.print_buffer[0];
    assert!(
        ticket.contains("Ticket of T-9") && ticket.contains("spawned"),
        "got: {}",
        ticket
    );
    assert!(ticket.contains("[via wire "), "got: {}", ticket);
    assert!(ticket.contains("<- by top-level code"), "got: {}", ticket);

    // The handler write happened on A, but its cause — the emit — happened
    // on B: the chain crosses the seam and says so.
    let notes = &vm_a.print_buffer[1];
    assert!(notes.contains("Notes of T-9 = { n: 1 }"), "got: {}", notes);
    assert!(
        notes.contains("<- by `on NoteAdded` handler"),
        "got: {}",
        notes
    );
    assert!(
        notes.contains("NoteAdded") && notes.contains("[via wire "),
        "emit record must carry the wire origin, got: {}",
        notes
    );
    assert!(notes.contains("<- by top-level code"), "got: {}", notes);
}

/// Provenance follows the merge path too: a value merged in from a wire
/// fork explains itself with the sender's record after commit(merged).
#[test]
fn why_explains_values_that_arrived_by_merge() {
    let shared = r#"
        component Gold { amount: 0 }
        fn seed() {
            let _hero = spawn("hero", Gold { amount: 10 })
        }
    "#;

    // Machine B: same base (deterministic), then diverge.
    let b_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let hero = get_entity("hero")
        set(hero, Gold {{ amount: 999 }})
        print(fork_to_bytes(fork()))
    "#
    );
    let vm_b = run(&b_src);
    let theirs_bytes = vm_b.print_buffer[0].clone();

    let a_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let ours = fork()
        let theirs = fork_from_bytes({theirs_bytes:?}) |> unwrap
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        let hero = get_entity("hero")
        let g = get(hero, Gold) |> unwrap
        print(g.amount)
        print(why(hero, Gold))
    "#
    );
    let vm_a = run(&a_src);
    assert_eq!(vm_a.print_buffer[0], "999");
    let out = &vm_a.print_buffer[1];
    assert!(
        out.contains("Gold of hero = { amount: 999 }"),
        "got: {}",
        out
    );
    assert!(out.contains("[via wire "), "got: {}", out);
}

// ---------------------------------------------------------------------------
// delta sync: fork_delta / fork_apply
// ---------------------------------------------------------------------------

/// Strip the wire header and the provenance section, leaving the state body.
/// State must be reproducible bit-for-bit; provenance legitimately differs
/// (origin labels name the seams a payload crossed).
fn wire_state_part(payload: &str) -> &str {
    let body = payload
        .splitn(3, ' ')
        .nth(2)
        .expect("wire payload has header + body");
    &body[..body.find(",\"prov\":").unwrap_or(body.len())]
}

/// fork_apply(base, fork_delta(base, f)) must reconstruct f exactly: entity
/// edits, spawns, despawns, renames, resources, the allocator, and the
/// in-flight queue. Verified by comparing the canonical full encodings of
/// the original and the reconstruction (state part — the reconstruction
/// honestly carries a wire origin in its provenance).
#[test]
fn delta_roundtrip_reconstructs_the_fork() {
    let vm = run(r#"
        component Gold { amount: 0 }
        component Tag { label: "" }
        resource Log { s: "" }
        event Ping { amount }
        on Ping(p) {
            let bank = get_entity("bank")
            let g = get(bank, Gold) |> unwrap
            set(bank, Gold { amount: g.amount + p.amount })
        }

        let _bank = spawn("bank", Gold { amount: 100 })
        let _hero = spawn("hero", Tag { label: "idle" })
        let _mook = spawn("mook", Tag { label: "doomed" })
        let base = fork()

        // Divergence: edit, spawn, despawn, resource write, in-flight event.
        let bank = get_entity("bank")
        set(bank, Gold { amount: 150 })
        let _scout = spawn("scout", Tag { label: "new" })
        let mook = get_entity("mook")
        despawn(mook)
        set_resource(Log, Log { s: "diverged" })
        emit Ping { amount: 7 }
        let f = fork()

        let d = fork_delta(base, f)
        let g = fork_apply(base, d) |> unwrap
        print(fork_to_bytes(f))
        print(fork_to_bytes(g))

        // And the reconstruction *behaves* like the original: commit, flush,
        // allocator parity (next spawn claims the same id).
        commit(g)
        flush_events()
        let b2 = get(get_entity("bank"), Gold) |> unwrap
        print(b2.amount)
        let probe = spawn("probe", Tag { label: "p" })
        print(probe)
    "#);
    assert_eq!(
        wire_state_part(&vm.print_buffer[0]),
        wire_state_part(&vm.print_buffer[1]),
        "reconstruction must be state-identical to the original fork"
    );
    assert_eq!(
        vm.print_buffer[2], "157",
        "in-flight event must survive the delta"
    );

    // Reference: same program without the wire detour claims the same id.
    let vm_ref = run(r#"
        component Gold { amount: 0 }
        component Tag { label: "" }
        resource Log { s: "" }
        event Ping { amount }
        on Ping(p) {
            let bank = get_entity("bank")
            let g = get(bank, Gold) |> unwrap
            set(bank, Gold { amount: g.amount + p.amount })
        }
        let _bank = spawn("bank", Gold { amount: 100 })
        let _hero = spawn("hero", Tag { label: "idle" })
        let _mook = spawn("mook", Tag { label: "doomed" })
        let bank = get_entity("bank")
        set(bank, Gold { amount: 150 })
        let _scout = spawn("scout", Tag { label: "new" })
        let mook = get_entity("mook")
        despawn(mook)
        set_resource(Log, Log { s: "diverged" })
        emit Ping { amount: 7 }
        flush_events()
        let probe = spawn("probe", Tag { label: "p" })
        print(probe)
    "#);
    assert_eq!(
        vm.print_buffer[3], vm_ref.print_buffer[0],
        "allocator state must survive the delta"
    );
}

/// The point of delta sync: cost proportional to the divergence, not the
/// world — for state *and* provenance. A 3-entity edit in a 300-entity world
/// must not ship the other 297 entities or their history.
#[test]
fn delta_is_small_and_so_is_its_provenance() {
    let vm = run(r#"
        component Item { label: "", n: 0 }
        let mut i = 0
        while i < 300 {
            let _e = spawn(Item { label: "filler_" + str(i), n: i })
            i = i + 1
        }
        let keep = spawn("keeper", Item { label: "keeper", n: 0 })
        let base = fork()

        set(keep, Item { label: "touched_value", n: 1 })
        let f = fork()

        print(len(fork_to_bytes(f)))
        print(len(fork_delta(base, f)))
        print(fork_delta(base, f))
    "#);
    let full: usize = vm.print_buffer[0].parse().unwrap();
    let delta: usize = vm.print_buffer[1].parse().unwrap();
    assert!(
        delta * 10 < full,
        "delta ({} bytes) must be at least 10x smaller than full ({} bytes)",
        delta,
        full
    );
    let payload = &vm.print_buffer[2];
    assert!(payload.contains("touched_value"), "touched row must ship");
    assert!(
        !payload.contains("filler_7"),
        "untouched rows (and their provenance) must not ship"
    );
}

/// `world_digest()` is the convergence receipt for distributed sync: it
/// hashes world STATE only. Event/provenance history must not move it,
/// in-flight (unflushed) events must not move it, and an actual state
/// change must.
#[test]
fn world_digest_tracks_state_not_history() {
    let vm = run(r#"
        component Counter { n: 0 }
        event Bump { who: entity }
        on Bump(e) { update(e.who, Counter) { n = 1 } }

        let a = spawn("a", Counter { n: 0 })
        let d0 = world_digest()

        // in-flight events are not state
        emit Bump { who: a }
        let d_inflight = world_digest()
        print(str(d_inflight == d0))

        // a real state change moves the digest
        flush_events()
        let d1 = world_digest()
        print(str(d1 != d0))

        // same state reached by a different history digests identically
        update(a, Counter) { n = 0 }
        let d2 = world_digest()
        print(str(d2 == d0))
    "#);
    assert_eq!(vm.print_buffer, vec!["true", "true", "true"]);
}

/// Entity-level patch granularity: editing one field of one component on an
/// entity the base already holds must not re-ship the entity's other
/// components (or the untouched fields of the edited one). Component
/// removal travels as a name, attachment falls back to a full upsert row.
#[test]
fn delta_ships_entity_field_patches_not_whole_rows() {
    let vm = run(r#"
        component Hp { current: 0, max: 0 }
        component Bio { story: "" }
        component Mark { tag: "" }
        component Aura { glow: "" }
        // long enough that provenance previews (96-char cap) cannot leak the
        // tail sentinel — only an actual whole-row ship would carry it
        let saga = "born under a wandering star, exiled twice, knighted once, and sworn to a quiet oath that ends in THE_FINAL_SECRET_WORD"
        let h = spawn("hero", Hp { current: 100, max: 100 },
                      Bio { story: saga },
                      Mark { tag: "doomed" })
        let base = fork()

        update(h, Hp) { current = 93 }
        remove(h, Mark)
        let d = fork_delta(base, fork())
        match fork_apply(base, d) {
            Ok(f) => {
                commit(f)
                let hp = get(h, Hp) |> unwrap
                print(hp.current)
                print(hp.max)
                print((get(h, Bio) |> unwrap).story == saga)
                print(has(h, Mark))
            }
            Err(m) => { print("apply failed: " + m) }
        }
        print(d)

        // attaching a component the base never saw cannot be a patch: the
        // entity falls back to a full upsert row (saga rides along)
        set(h, Aura { glow: "golden" })
        print(fork_delta(base, fork()))
    "#);
    assert_eq!(vm.print_buffer[0], "93", "patched field must apply");
    assert_eq!(vm.print_buffer[1], "100", "untouched field must survive");
    assert_eq!(
        vm.print_buffer[2], "true",
        "untouched component must survive byte-for-byte"
    );
    assert_eq!(vm.print_buffer[3], "false", "removal must apply");
    let payload = &vm.print_buffer[4];
    assert!(
        payload.contains("\"ent_patch\":[[") && payload.contains("\"upserts\":[]"),
        "edit of a known entity must travel as a patch, not an upsert: {}",
        payload
    );
    assert!(
        !payload.contains("THE_FINAL_SECRET_WORD"),
        "untouched component content must not ship: {}",
        payload
    );
    let with_attach = &vm.print_buffer[5];
    assert!(
        with_attach.contains("\"upserts\":[[") && with_attach.contains("THE_FINAL_SECRET_WORD"),
        "attaching a new component must fall back to a full upsert row: {}",
        with_attach
    );
}

/// Per-field resource patches: a growing journal must cost O(changed fields),
/// not O(resource). Ticking `round` on a resource that also carries a fat
/// log string must not re-ship the log.
#[test]
fn delta_ships_resource_field_patches_not_whole_rows() {
    let vm = run(r#"
        resource Journal { log: "", round: 0 }
        let mut big = ""
        let mut i = 0
        while i < 200 {
            big = big + "entry " + str(i) + "!"
            i = i + 1
        }
        set_resource(Journal, Journal { log: big, round: 1 })
        let base = fork()
        let j = get_resource(Journal) |> unwrap
        set_resource(Journal, Journal { log: j.log, round: 2 })
        let d = fork_delta(base, fork())
        match fork_apply(base, d) {
            Ok(f) => {
                commit(f)
                let j2 = get_resource(Journal) |> unwrap
                print(j2.round)
                print(len(j2.log))
            }
            Err(m) => { print("apply failed: " + m) }
        }
        print(len(big))
        print(len(d))
        print(d)
    "#);
    assert_eq!(vm.print_buffer[0], "2", "patched field must apply");
    assert_eq!(
        vm.print_buffer[1], vm.print_buffer[2],
        "untouched log field must survive the patch byte-for-byte"
    );
    let log_len: usize = vm.print_buffer[1].parse().unwrap();
    let delta_len: usize = vm.print_buffer[3].parse().unwrap();
    assert!(
        delta_len < log_len / 2,
        "delta ({} B) must not carry the {} B log",
        delta_len,
        log_len
    );
    let payload = &vm.print_buffer[4];
    assert!(payload.contains("\"res_patch\""), "got: {}", payload);
    // (the provenance section carries a ~100-char bounded *preview* of the
    // touched resource, so probe for content beyond the preview cutoff)
    assert!(
        !payload.contains("entry 150!"),
        "unchanged log content must not ship: {}",
        payload
    );
}

/// Corruption and wrong-base application are errors, not fabricated worlds.
#[test]
fn delta_apply_rejects_corruption_and_wrong_base() {
    let vm = run(r#"
        component Gold { amount: 0 }
        let _bank = spawn("bank", Gold { amount: 100 })
        let base = fork()
        let _extra = spawn("extra", Gold { amount: 1 })
        let other = fork()
        commit(base)

        let bank = get_entity("bank")
        set(bank, Gold { amount: 150 })
        let f = fork()
        let d = fork_delta(base, f)

        match fork_apply(base, d + " ") {
            Ok(_) => { print("accepted corrupt") }
            Err(e) => { print(e) }
        }
        match fork_apply(other, d) {
            Ok(_) => { print("accepted wrong base") }
            Err(e) => { print(e) }
        }
        match fork_apply(base, d) {
            Ok(_) => { print("ok") }
            Err(e) => { print(e) }
        }
    "#);
    assert!(
        vm.print_buffer[0].contains("integrity digest mismatch"),
        "got: {}",
        vm.print_buffer[0]
    );
    assert!(
        vm.print_buffer[1].contains("different base"),
        "got: {}",
        vm.print_buffer[1]
    );
    assert_eq!(vm.print_buffer[2], "ok");
}

/// Cross-machine why() over the delta path: the receiver applies a delta,
/// commits, and the answer chains through records that traveled inside it —
/// labeled with the wire origin, including the emit behind a handler write.
#[test]
fn delta_provenance_answers_why_across_the_seam() {
    let shared = r#"
        component Ticket { status: "open" }
        component Notes { n: 0 }
        event NoteAdded { who }
        on NoteAdded(e) {
            let cur = get(e.who, Notes) |> unwrap
            set(e.who, Notes { n: cur.n + 1 })
        }
        fn seed() {
            let _t = spawn("T-9", Ticket { status: "open" }, Notes { n: 0 })
        }
    "#;

    // Machine B: same deterministic base, then diverge + emit, ship a delta.
    let b_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let t = get_entity("T-9")
        set(t, Ticket {{ status: "escalated" }})
        emit NoteAdded {{ who: t }}
        print(fork_delta(base, fork()))
    "#
    );
    let vm_b = run(&b_src);
    let delta = vm_b.print_buffer[0].clone();

    // Machine A: apply against its own copy of the base, commit, flush, ask.
    let a_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let theirs = fork_apply(base, {delta:?}) |> unwrap
        commit(theirs)
        flush_events()
        let t = get_entity("T-9")
        print(why(t, Ticket))
        print(why(t, Notes))
    "#
    );
    let vm_a = run(&a_src);

    let ticket = &vm_a.print_buffer[0];
    assert!(
        ticket.contains("status: \"escalated\"") || ticket.contains("escalated"),
        "got: {}",
        ticket
    );
    assert!(ticket.contains("[via wire "), "got: {}", ticket);

    let notes = &vm_a.print_buffer[1];
    assert!(notes.contains("Notes of T-9 = { n: 1 }"), "got: {}", notes);
    assert!(
        notes.contains("<- by `on NoteAdded` handler"),
        "got: {}",
        notes
    );
    assert!(
        notes.contains("NoteAdded") && notes.contains("[via wire "),
        "the emit that traveled inside the delta must carry its origin, got: {}",
        notes
    );
}

/// Schema drift across the delta path: the sender runs v1, the receiver
/// declares v2 with a `migrate` block. The receiver's base arrived via the
/// full codec (migrated on ingest); the delta's rows are still v1-shaped and
/// must migrate on apply, exactly like the full codec.
#[test]
fn delta_runs_migrations_on_schema_drift() {
    // Machine B (v1): make base, ship it, then diverge and ship the delta.
    let vm_b = run(r#"
        component Hero { hp: 0 }
        let _h = spawn("hero", Hero { hp: 42 })
        let base = fork()
        let h = get_entity("hero")
        set(h, Hero { hp: 99 })
        print(fork_to_bytes(base))
        print(fork_delta(base, fork()))
    "#);
    let base_bytes = vm_b.print_buffer[0].clone();
    let delta = vm_b.print_buffer[1].clone();

    // Machine A (v2): ingest the base (migrates), apply the delta (the
    // shipped v1 row migrates too), commit, observe v2 shape + v1 value.
    let a_src = format!(
        r#"
        component Hero {{ hp: 0, shield: 0 }}
        migrate Hero(old) {{
            return Hero {{ hp: old["hp"], shield: old["hp"] / 2 }}
        }}
        let base = fork_from_bytes({base_bytes:?}) |> unwrap
        let theirs = fork_apply(base, {delta:?}) |> unwrap
        commit(theirs)
        let h = get_entity("hero")
        let hero = get(h, Hero) |> unwrap
        print(hero.hp)
        print(hero.shield)
    "#
    );
    let vm_a = run(&a_src);
    assert_eq!(vm_a.print_buffer, vec!["99", "49"]);
}

/// The reconstruction shares lineage with the receiver's base (CoW restore +
/// surgical apply), so the O(divergence) merge fast path — and merge
/// semantics generally — work on wire-delivered forks without a full scan.
#[test]
fn merge_after_delta_apply_matches_in_process_merge() {
    let shared = r#"
        component Gold { amount: 0 }
        component Tag { label: "" }
        fn seed() {
            let _bank = spawn("bank", Gold { amount: 100 })
            let _hero = spawn("hero", Tag { label: "idle" })
        }
    "#;

    // Machine B: diverge from the shared base, ship a delta.
    let b_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let hero = get_entity("hero")
        set(hero, Tag {{ label: "questing" }})
        let _scout = spawn("scout", Tag {{ label: "new" }})
        print(fork_delta(base, fork()))
    "#
    );
    let vm_b = run(&b_src);
    let delta = vm_b.print_buffer[0].clone();

    // Machine A: own divergence + B's delta, three-way merge.
    let a_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let bank = get_entity("bank")
        set(bank, Gold {{ amount: 150 }})
        let ours = fork()
        commit(base)
        let theirs = fork_apply(base, {delta:?}) |> unwrap
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        print(fork_to_bytes(fork()))
    "#
    );
    let vm_a = run(&a_src);

    // Reference: the whole dance in one process.
    let ref_src = format!(
        r#"{shared}
        seed()
        let base = fork()
        let bank = get_entity("bank")
        set(bank, Gold {{ amount: 150 }})
        let ours = fork()
        commit(base)
        let hero = get_entity("hero")
        set(hero, Tag {{ label: "questing" }})
        let _scout = spawn("scout", Tag {{ label: "new" }})
        let theirs = fork()
        commit(base)
        let merged = merge_forks(base, ours, theirs) |> unwrap
        commit(merged)
        print(fork_to_bytes(fork()))
    "#
    );
    let vm_ref = run(&ref_src);

    assert_eq!(
        wire_state_part(&vm_a.print_buffer[0]),
        wire_state_part(&vm_ref.print_buffer[0]),
        "merge over a delta-delivered fork must equal the in-process merge"
    );
}
