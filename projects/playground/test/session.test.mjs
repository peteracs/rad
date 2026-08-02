// The JS test harness the critique demanded — three rounds of demos were
// hand-verified through CDP; this locks the session API contract into CI.
//
//   node --test projects/playground/test
//
// Prereq: wasm-pack build --target nodejs --out-dir ../../projects/playground/pkg-node core/vm
//
// What's covered is exactly the layer every RADGUI demo stands on:
// lifecycle, render deltas, undo/redo, the collab replication protocol
// (host/replica/late-join/out-of-order), persistence round-trips, and
// patch & replay. The DOM renderer itself is exercised in the browser;
// everything below it is exercised here.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const { RadRuntime } = await import(
    new URL("../pkg-node/rad_vm.js", import.meta.url).href
);

const COUNTER = `
component Box { z: int = 0 }
resource Count { n: int = 0 }
pub event Bump { amount: int }
on Bump(e) {
    let c = get_resource(Count) |> unwrap
    set_resource(Count, Count { n: c.n + e.amount })
}
fn main() -> nil {
    set_resource(Count, Count { n: 0 })
    let _b = entity "box" { Box { z: 1 } }
}
`;

function freshSession(src = COUNTER) {
    const rt = new RadRuntime();
    rt.session_start(src);
    return rt;
}

function countOf(rt) {
    const state = JSON.parse(rt.session_render_delta());
    // render_delta vs a fresh base reports resources only when changed;
    // use the full snapshot path instead
    const snap = JSON.parse(rt.get_world_snapshot());
    return snap.resources.Count.n;
}

// ---------------------------------------------------------------- lifecycle

test("session: start, emit, pump, observe", () => {
    const rt = freshSession();
    rt.session_emit("Bump", JSON.stringify({ amount: 3 }));
    rt.session_pump();
    assert.equal(countOf(rt), 3);
    rt.session_emit("Bump", JSON.stringify({ amount: 4 }));
    rt.session_pump();
    assert.equal(countOf(rt), 7);
});

test("render delta: quiet pump ships nothing, one change ships one row", () => {
    const rt = freshSession();
    rt.session_render_delta(); // settle the render base
    rt.session_pump();
    const quiet = JSON.parse(rt.session_render_delta());
    assert.equal(quiet.upsert.length, 0);
    assert.deepEqual(quiet.resources, {});

    rt.session_emit("Bump", JSON.stringify({ amount: 1 }));
    rt.session_pump();
    const loud = JSON.parse(rt.session_render_delta());
    assert.equal(loud.upsert.length, 0); // only a RESOURCE changed
    assert.equal(loud.resources.Count.n, 1);
});

// --------------------------------------------------------------- undo/redo

test("undo/redo: checkpoint, rewind, walk forward, prune on new action", () => {
    const rt = freshSession();
    rt.session_checkpoint();
    rt.session_emit("Bump", JSON.stringify({ amount: 5 }));
    rt.session_pump();
    assert.equal(countOf(rt), 5);

    assert.equal(rt.session_undo(), true);
    assert.equal(countOf(rt), 0);

    assert.equal(rt.session_redo(), true);
    assert.equal(countOf(rt), 5);

    // a fresh action prunes the redo branch
    assert.equal(rt.session_undo(), true);
    rt.session_checkpoint();
    rt.session_emit("Bump", JSON.stringify({ amount: 1 }));
    rt.session_pump();
    assert.equal(rt.session_redo(), false);
    assert.equal(countOf(rt), 1);

    // empty ring answers false, not an error
    assert.equal(rt.session_undo(), true); // back to 0
    assert.equal(rt.session_undo(), false);
});

// ------------------------------------------------- collab replication core

test("collab: host edits, replica applies, digests agree", () => {
    const host = freshSession();
    const replica = freshSession();
    host.session_delta(); // settle both wire bases
    assert.equal(host.session_digest(), replica.session_digest());

    host.session_emit("Bump", JSON.stringify({ amount: 9 }));
    host.session_pump();
    const wire = host.session_delta();
    replica.session_apply(wire);

    assert.equal(countOf(replica), 9);
    assert.equal(host.session_digest(), replica.session_digest());
});

test("collab: late joiner adopts session_state and stays in the stream", () => {
    const host = freshSession();
    host.session_emit("Bump", JSON.stringify({ amount: 2 }));
    host.session_pump();
    host.session_delta(); // history the late joiner never saw

    const late = new RadRuntime();
    late.session_start(COUNTER);
    late.session_load(host.session_state());
    assert.equal(host.session_digest(), late.session_digest());

    host.session_emit("Bump", JSON.stringify({ amount: 3 }));
    host.session_pump();
    late.session_apply(host.session_delta());
    assert.equal(countOf(late), 5);
    assert.equal(host.session_digest(), late.session_digest());
});

test("collab: a wrong-lineage delta is refused, never merged wrong", () => {
    const host = freshSession();
    const replica = freshSession();
    host.session_delta();

    host.session_emit("Bump", JSON.stringify({ amount: 1 }));
    host.session_pump();
    const d1 = host.session_delta();
    host.session_emit("Bump", JSON.stringify({ amount: 1 }));
    host.session_pump();
    const d2 = host.session_delta();

    // applying d2 without d1: base fingerprint mismatch -> hard error
    assert.throws(() => replica.session_apply(d2));
    // the world is untouched by the refused delta
    assert.equal(countOf(replica), 0);
    // in-order still works
    replica.session_apply(d1);
    replica.session_apply(d2);
    assert.equal(countOf(replica), 2);
});

// ------------------------------------------------------------- persistence

test("persist: state round-trips through fork_to_bytes wire", () => {
    const a = freshSession();
    a.session_emit("Bump", JSON.stringify({ amount: 42 }));
    a.session_pump();
    const saved = a.session_state();

    const b = freshSession();
    b.session_load(saved);
    assert.equal(countOf(b), 42);
    assert.equal(a.session_digest(), b.session_digest());
});

// ---------------------------------------------------------- patch & replay

const STORY = `
component Purse { gold: int = 100 }
pub event Spend { amount: int }
on Spend(e) {
    for h in entities(Purse) {
        let p = get(h, Purse) |> unwrap
        update(h, Purse) { gold = p.gold - e.amount }
    }
}
fn main() -> nil {
    let _h = entity "hero" { Purse { gold: 100 } }
    for i in range(0, 5) {
        emit Spend { amount: 10 }
        flush_events()
    }
    let hero = get_entity("hero")
    let p = get(hero, Purse) |> unwrap
    print(f"left: {p.gold}")
}
`;

function goldAt(rt, frame) {
    const w = JSON.parse(rt.timeline_world(frame));
    const hero = w.entities.find((e) => e.name === "hero");
    return hero.components.find((c) => c.type === "Purse").fields.gold;
}

test("patch & replay: edit the past, the future recomputes", () => {
    const rt = new RadRuntime();
    const out1 = rt.run_traced(STORY);
    assert.match(out1, /left: 50/);
    const frames = rt.timeline_len();
    assert.ok(frames >= 5);
    const before = goldAt(rt, 3);

    const rt2 = new RadRuntime();
    const out2 = rt2.run_traced_with_patch(STORY, 3, "hero", "Purse", "gold", "1000");
    // frames before the patch are identical history
    assert.equal(goldAt(rt2, 2), goldAt(rt, 2));
    // the patched frame shows the edit, the future is recomputed from it
    assert.equal(goldAt(rt2, 3), 1000);
    assert.notEqual(goldAt(rt2, frames - 1), goldAt(rt, frames - 1));
    assert.match(out2, /left: 980/);
    assert.notEqual(before, 1000);
});

// --------------------------------------------------- the real lib_gui stack

test("lib_gui + counter compile and serve a session end to end", () => {
    const lib = readFileSync(join(here, "../../../projects/dogfood/radgui/lib_gui.rad"), "utf8");
    const app = readFileSync(join(here, "../../../projects/dogfood/radgui/counter.rad"), "utf8");
    const source = lib + "\n" + app.replace(/^use "lib_gui.rad"\s*$/m, "");
    const rt = new RadRuntime();
    rt.session_start(source);
    const snap = JSON.parse(rt.get_world_snapshot());
    const widgets = snap.entities.filter((e) =>
        e.components.some((c) => c.type === "Widget"));
    assert.ok(widgets.length >= 5, "counter UI spawns widget entities");
    assert.equal(snap.resources.UiConfig.undo, 1);
});
