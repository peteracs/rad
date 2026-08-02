// D4 collab demo — worker side. The rad VM (WASM) runs a streaming session
// in here: compile once, then emit/pump/delta/apply. The page thread only
// renders and relays BroadcastChannel messages.

import init, { RadRuntime } from "./pkg/rad_vm.js";

const SOURCE = `
component Note { text: "", by: "" }
resource Board { count: 0 }
event AddNote { text: str, by: str }
event ClearNotes { by: str }

on AddNote(e) {
    let b = get_resource(Board) |> unwrap
    let n = b.count + 1
    let _ = spawn(f"note-{n}", Note { text: e.text, by: e.by })
    set_resource(Board, Board { count: n })
}

on ClearNotes(e) {
    for ent in entities(Note) { despawn(ent) }
}
`;

let runtime = null;

function reply(id, ok, payload) {
    self.postMessage({ id, ok, ...payload });
}

self.onmessage = async (ev) => {
    const { id, cmd, args } = ev.data;
    try {
        if (cmd === "start") {
            await init();
            runtime = new RadRuntime();
            runtime.session_start(SOURCE);
            reply(id, true, { digest: runtime.session_digest() });
            return;
        }
        if (!runtime) throw new Error("session not started");
        switch (cmd) {
            case "emit": {
                runtime.session_emit(args.event, JSON.stringify(args.fields));
                reply(id, true, {});
                break;
            }
            case "pump_and_delta": {
                // One frame, one delta: the host's per-flush broadcast.
                runtime.session_pump();
                const delta = runtime.session_delta();
                reply(id, true, {
                    delta,
                    digest: runtime.session_digest(),
                    snapshot: runtime.get_world_snapshot(),
                });
                break;
            }
            case "apply": {
                runtime.session_apply(args.delta);
                reply(id, true, {
                    digest: runtime.session_digest(),
                    snapshot: runtime.get_world_snapshot(),
                });
                break;
            }
            case "state": {
                reply(id, true, { state: runtime.session_state() });
                break;
            }
            case "load": {
                runtime.session_load(args.state);
                reply(id, true, {
                    digest: runtime.session_digest(),
                    snapshot: runtime.get_world_snapshot(),
                });
                break;
            }
            case "snapshot": {
                reply(id, true, {
                    digest: runtime.session_digest(),
                    snapshot: runtime.get_world_snapshot(),
                });
                break;
            }
            default:
                throw new Error(`unknown cmd ${cmd}`);
        }
    } catch (e) {
        reply(id, false, { error: String(e) });
    }
};
