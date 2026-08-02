# Build a collaborative task board in rad

A from-zero tutorial. You will build a tiny shared task board — `add`,
`close`, `assign`, `list` — and then do four things to it that no mainstream
stack does:

1. Ask the runtime **who caused a value** (`why`) and get a human's name back.
2. **Record** a session with a bug, **replay** it bit-identically, then ask
   *"what would my fix have done in that exact session?"* — a counterfactual,
   not a re-run.
3. Let two offline users diverge and **merge their timelines**, with conflicts
   delivered as data and the resolution policy written by you, in code.
4. Run that divergence across **two separate processes** and prove convergence
   with matching cryptographic digests on both sides.

No rad knowledge is assumed. Every command below shows its expected output;
if you see something else, that's a bug — please report it.

**The contract:** you type along in your own file. Each stage has a
checkpoint file in this folder (`01_model.rad`, `02_provenance.rad`, …) that
is the complete working state at the end of the stage — diff against it if
you get stuck, or run the checkpoints directly if you only want the receipts.

---

## Stage 0 — setup (5 minutes)

You need a [Rust toolchain](https://rustup.rs/). Then:

```bash
git clone https://github.com/peteracs/rad.git
cd rad
cargo build -p rad-vm --release
```

The binary lands at `target/release/rad`. Sanity check:

```bash
./target/release/rad --version
# rad 0.5.0
```

All commands below run from the repo root and work in bash and PowerShell
alike.

---

## Stage 1 — the model (`01_model.rad`)

Create `projects/tutorial/task-board/tasks.rad`. A rad program is built from three primitives:
**components** (pure data, attached to entities), **events** (the messages
that request changes), and **handlers** (the only logic that performs them).

Start with the data:

```
component Task {
    title: str = "",
    indexed status: str = "open",   // indexed: list views are a hash probe
    assignee: str = ""
}

resource Board { next_id: int = 1 }
```

A `component` is a record attached to entities; a `resource` is a global
singleton. The `indexed` keyword maintains a runtime hash index on that
field — you'll use it for the `list` view.

Now the events and their handlers:

```
event Opened   { task: entity, title: str, by: str }
event Closed   { task: entity, by: str }
event Assigned { task: entity, to: str, by: str }

on Closed(e)   { update(e.task, Task) { status = "closed" } }
on Assigned(e) { update(e.task, Task) { assignee = e.to } }
```

Every event carries `by: str` — the human who asked. This looks like
ceremony in stage 1. It is the whole point of stage 2: route all mutations
through events that name a person, and the runtime can answer "who did
this?" forever after, with zero logging code.

Opening a task mints a name (`T-1`, `T-2`, …) and spawns a named entity:

```
fn new_task(title: str, by: str) -> entity {
    let b = get_resource(Board) |> unwrap
    let id = f"T-{b.next_id}"
    set_resource(Board, Board { next_id: b.next_id + 1 })
    let t = spawn(id, Task { title: title })
    emit Opened { task: t, title: title, by: by }
    flush_events()
    print(f"opened {id}: {title}")
    return t
}
```

The `list` view is one indexed probe — `lookup_all(Task, "status", "open")`
returns matching entity ids without scanning the world:

```
fn list_tasks() -> nil {
    let open = lookup_all(Task, "status", "open")
    if len(open) == 0 {
        print("(no open tasks)")
        return nil
    }
    for t in open {
        let tk = get(t, Task) |> unwrap
        let mut who = tk.assignee
        if who == "" { who = "-" }
        print(f"[ ] {name_of(t)}  {tk.title}  ({who})")
    }
}
```

Finish with a stdin command loop (see `01_model.rad` for the full `handle`
function — it parses `add` / `close` / `assign` / `list` / `quit` and emits
the matching event; `get_entity("T-1")` finds an entity by name). Then run
it:

```bash
./target/release/rad projects/tutorial/task-board/01_model.rad
```

Type:

```text
add fix login page
add update docs
assign T-1 bob
list
close T-1
list
quit
```

Expected:

```text
opened T-1: fix login page
opened T-2: update docs
assigned T-1 to bob
[ ] T-1  fix login page  (bob)
[ ] T-2  update docs  (-)
closed T-1
[ ] T-2  update docs  (-)
```

That's the whole app. ~100 lines, no framework, no database. Everything
after this stage is the part you can't do elsewhere.

---

## Stage 2 — ask why (`02_provenance.rad`)

Add one command to `handle`:

```
if cmd == "why" {
    let t = get_entity(parts[1])
    if t == nil {
        print(f"no task {parts[1]}")
    } else {
        print(why(t, Task))
    }
}
```

Run it, and this time interrogate the board:

```text
add fix login page
assign T-1 bob
close T-1
why T-1
quit
```

Expected:

```text
opened T-1: fix login page
assigned T-1 to bob
closed T-1
Task of T-1 = { title: "fix login page", status: "closed", assignee: "bob" }   (set in frame 3)
  <- by `on Closed` handler
  <- Closed { task: T-1, by: "alice" } emitted in frame 2
  <- by top-level code
```

Read that chain bottom-up: top-level code emitted `Closed { by: "alice" }`,
the `on Closed` handler handled *that exact event instance*, and its write
produced the current value. The VM keeps a causality ledger of every write
and emission on the main timeline — `why()` is a query over it, not a log
you remembered to write. The chain is causal, not correlated: handlers link
to the specific emit record they were handling, across as many event hops
as it takes.

---

## Stage 3 — record the bug, replay the fix (`03_replay/`)

`03_replay/buggy.rad` is your stage-2 program with one planted bug:

```
// THE BUG: assigns the task to whoever did the assigning (e.by), not the
// person it was assigned to (e.to).
on Assigned(e) { update(e.task, Task) { assignee = e.by } }
```

Record a session against it — `--record` captures every io interaction
(stdin included) into a tape:

```bash
cat projects/tutorial/task-board/03_replay/session.txt | ./target/release/rad projects/tutorial/task-board/03_replay/buggy.rad --record projects/tutorial/task-board/03_replay/trace.radr
```

The output shows the bug biting — everything is assigned to alice:

```text
[ ] T-1  fix login page  (alice)
[ ] T-3  rotate api keys  (alice)
```

And notice the session's `why T-1` output — the ledger shows the handler
*received* `to: "bob"` and wrote `"alice"` anyway. The provenance chain
contains the bug:

```text
Task of T-1 = { title: "fix login page", status: "open", assignee: "alice" }   (set in frame 4)
  <- by `on Assigned` handler
  <- Assigned { task: T-1, to: "bob", by: "alice" } emitted in frame 3
```

The tape is self-contained (it embeds the source). Replay it with no stdin,
no files, no network:

```bash
./target/release/rad replay projects/tutorial/task-board/03_replay/trace.radr
```

Same output, byte for byte, ending with:

```text
Replay: 6 frame(s), 1 io record(s) consumed, 0 leftover
Replay verified: world digest matches the recorded run
```

Now the counterfactual. `03_replay/fixed.rad` changes the one buggy line
(`e.by` → `e.to`). Don't re-run the program — ask what the fix *would have
done in the recorded session*:

```bash
./target/release/rad replay projects/tutorial/task-board/03_replay/trace.radr --with projects/tutorial/task-board/03_replay/fixed.rad
```

The fixed code runs against the original session's recorded inputs, and the
tooling diffs the two final worlds:

```text
[ ] T-1  fix login page  (bob)
[ ] T-3  rotate api keys  (carol)
...
=== Retroactive replay: projects/tutorial/task-board/03_replay/fixed.rad against the recorded session ===
Recorded io: 1 consumed, 0 repeated reads, 0 unused
The edit's blast radius (original vs edited final world):
  {Task: 2}
```

`{Task: 2}` is the deliverable: *this fix changes exactly the two
mis-assigned tasks and touches nothing else.* That answer came from the
tapes, not from a re-run against today's world — a debugger for the
question every postmortem actually asks.

---

## Stage 4 — two timelines, one board (`04_merge.rad`)

`fork()` snapshots the full program state — entities, names, resources, the
id allocator, even in-flight events — as a copy-on-write value. `commit()`
adopts a fork as the live state. That's enough to script an offline
divergence in one process:

```
let base = fork()              // the board both teammates pulled

// alice's afternoon
close("T-1", "alice")
assign("T-2", "bob", "alice")
new_task("rotate api keys", "alice")     // mints T-3
let ours = fork()

commit(base)                   // rewind; bob's afternoon
assign("T-2", "carol", "bob")            // collides with alice's assign
new_task("upgrade rust toolchain", "bob")  // also mints T-3
let theirs = fork()

commit(base)
match merge_forks(base, ours, theirs) { ... }
```

Run the checkpoint:

```bash
./target/release/rad projects/tutorial/task-board/04_merge.rad
```

```text
merge refused: 2 conflict(s)

  T-2: Task.assignee  base= ours=bob theirs=carol
  name T-3 claimed by 2 entities

the merged board:
[x] T-1  fix login page  (-)
[ ] T-2  update docs  (carol)
[ ] T-3/a  rotate api keys  (-)
[ ] T-3/b  upgrade rust toolchain  (-)
```

Things to notice, in increasing order of "you can't do this elsewhere":

- **The merge is field-granular.** Alice closed T-1 and bob never touched
  it — not a conflict. Both sessions bumped `Board.next_id` to the same
  value — convergent, not a conflict. Only genuine divergence surfaces.
- **Conflicts are data, not prose.** `merge_forks` returns
  `Err(list<Conflict>)`, a sum type. Your resolution policy is a `match`:

```
FieldConflict { ent: _e, name, comp, field, base, ours, theirs } => {
    decisions << (c, theirs)          // your rule, in your language
}
NameConflict { name, entities } => {
    decisions << (c, [f"{name}/a", f"{name}/b"])   // keep both
}
```

- **Names are identity, and the machine never picks.** Both sessions minted
  `T-3` for different tasks. A CRDT would converge mechanically; rad makes
  you answer, and *"keep both, as T-3/a and T-3/b"* is an answer it
  re-validates — a rename that still collides comes back as a fresh
  conflict instead of silently stealing a name.
- **Provenance survives the merge.** The final `why(T-2)` names bob's
  emit — and appends an honest note that a `commit()` adopted a fork after
  that write, because fork-internal writes are not in the main ledger. The
  runtime tells you exactly what it knows and where its knowledge seams are.

---

## Stage 5 — across machines (`05_sync.rad`)

Same divergence, but alice and bob are now **separate processes** that
exchange state through files. `fork_to_bytes()` serializes a fork — world,
names, allocator, in-flight events, *and the provenance ledger* — and
`fork_from_bytes()` ingests one (rejecting corruption by blake3 digest).
Swap the files for `tcp_write`/`tcp_read` and nothing else changes.

Run the exchange — five process invocations, each a separate "machine
session":

```bash
./target/release/rad projects/tutorial/task-board/05_sync.rad -- init
./target/release/rad projects/tutorial/task-board/05_sync.rad -- edit alice
./target/release/rad projects/tutorial/task-board/05_sync.rad -- edit bob
./target/release/rad projects/tutorial/task-board/05_sync.rad -- merge alice bob    # alice's machine
./target/release/rad projects/tutorial/task-board/05_sync.rad -- merge bob alice    # bob's machine
```

Both merges end with the receipt:

```text
world digest: 04e65f383da82c607a7e00a61de5d88b1d98993086f99007da4e33daa1ef3ccd
```

(Your hash will differ from this printout only if your boards diverged —
the two merge runs must always match *each other*.)

Two things had to be true for those digests to match:

- **The policy is perspective-independent.** Alice merges with
  `ours=alice, theirs=bob`; bob merges with the roles flipped. A
  "theirs wins" rule would converge the two machines to *different*
  boards. The checkpoint's policy picks the sorted-last candidate —
  arbitrary, but symmetric. This is the distributed-systems lesson the
  receipt enforces: try changing `resolve` to pick `theirs` and watch the
  digests split.
- **Provenance crossed the wire.** Alice's machine explains a value it
  never computed:

```text
Task of T-2 = { ... assignee: "carol" }   (set in frame 1)   [via wire 181f3fd3, remote frame]
  <- by `on Assigned` handler
  <- Assigned { task: T-2, to: "carol", by: "bob" } emitted in frame 0 [via wire 181f3fd3]
```

  `[via wire 181f3fd3]` is the payload digest the receiver *verified*, not
  a claim the sender made. Frame numbers inside foreign records follow the
  sender's clock, and the label says so. (On bob's machine the ledger's
  newest T-2 record is alice's assign-to-bob write while the surviving
  value is carol — the `commit() adopted a fork` note below the chain
  discloses exactly that seam. The runtime never pretends one timeline
  exists when two did.)

---

## Where you are now

In five files you have used: ECS components and events, indexed queries,
causal provenance (`why`), deterministic record/replay, counterfactual
replay with blast-radius diffs (`replay --with`), world forking, three-way
field-granular merge with programmable conflict resolution, the wire codec,
cross-machine provenance, and digest-certified convergence. The same
primitives compose further:

- `fork_delta` / `fork_apply` — after the first transfer, sync divergence
  only (KBs, not MBs). See the [builtins reference](../reference/builtins.md).
- `save_world` / `load_world` + `migrate` — persistence with schema
  evolution; v1 saves load into v2 programs through your migration blocks.
- `simulate` / `simulate_par` — speculative futures of the live world,
  statically fenced from io.
- Bigger versions of this exact app, dogfooded into existence:
  [`projects/dogfood/radtrack/`](../../../projects/dogfood/radtrack/) (offline-first issue tracker
  with a sync server), [`projects/dogfood/syncdesk/`](../../../projects/dogfood/syncdesk/) (TCP
  server + offline clients), [`projects/dogfood/radsheet/`](../../../projects/dogfood/radsheet/)
  (collaborative spreadsheet with per-cell `why()` and a history scrubber).

If anything in this tutorial surprised you, broke, or read as a claim the
receipt didn't back — that's a finding. Open an issue; outsider eyes are
the one thing this project can't dogfood.
