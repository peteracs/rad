// RADGUI — worker side. One long-lived GUI session (compile once; every
// interaction is session_emit + session_pump; the renderer reads back the
// world snapshot — widgets are entities, the world IS the scene graph),
// plus an optional TARGET runtime: a second VM that debugger-style apps
// (RADSCOPE) run traced programs in and interrogate frame by frame.

import init, { RadRuntime } from "./pkg/rad_vm.js";

let ready = init().then(() => new RadRuntime());
let target = null; // lazy second runtime for traced target programs

self.onmessage = async (ev) => {
    const msg = ev.data;
    const runtime = await ready;
    try {
        if (msg.type === "start") {
            runtime.reset();
            const output = runtime.session_start(msg.source);
            self.postMessage({
                type: "frame",
                reqId: msg.reqId,
                output,
                delta: runtime.session_render_delta(),
            });
        } else if (msg.type === "events") {
            if (msg.checkpoint) runtime.session_checkpoint();
            for (const e of msg.events) {
                runtime.session_emit(e.name, JSON.stringify(e.fields));
            }
            const output = runtime.session_pump();
            const reply = {
                type: "frame",
                reqId: msg.reqId,
                output,
                delta: runtime.session_render_delta(),
            };
            // collab hosts broadcast the wire-format divergence per pump
            if (msg.wantWire) reply.wire = runtime.session_delta();
            self.postMessage(reply);
        } else if (msg.type === "apply") {
            // collab replica: adopt the host's wire delta, re-render
            runtime.session_apply(msg.wire);
            self.postMessage({
                type: "frame",
                reqId: msg.reqId,
                output: "",
                delta: runtime.session_render_delta(),
            });
        } else if (msg.type === "state") {
            self.postMessage({
                type: "state",
                reqId: msg.reqId,
                result: { state: runtime.session_state() },
            });
        } else if (msg.type === "load") {
            runtime.session_load(msg.state);
            self.postMessage({
                type: "frame",
                reqId: msg.reqId,
                output: "",
                delta: runtime.session_render_delta(),
            });
        } else if (msg.type === "redo") {
            const did = runtime.session_redo();
            self.postMessage({
                type: "frame",
                reqId: msg.reqId,
                output: "",
                delta: did ? runtime.session_render_delta() : '{"upsert":[],"remove":[],"resources":{}}',
            });
        } else if (msg.type === "undo") {
            const did = runtime.session_undo();
            self.postMessage({
                type: "frame",
                reqId: msg.reqId,
                output: did ? "" : "(nothing to undo)",
                delta: runtime.session_render_delta(),
            });
        } else if (msg.type === "why") {
            const text = runtime.session_why(msg.entity, msg.component);
            self.postMessage({ type: "why", reqId: msg.reqId, result: { text } });
        } else if (msg.type === "preview") {
            const json = runtime.session_preview(msg.name, JSON.stringify(msg.fields));
            self.postMessage({ type: "preview", reqId: msg.reqId, result: { json } });
        } else if (msg.type === "target") {
            if (!target) target = new RadRuntime();
            let result;
            switch (msg.op) {
                case "run_traced": {
                    let output = "";
                    let error = null;
                    try {
                        output = target.run_traced(msg.source);
                    } catch (e) {
                        error = String(e); // partial timeline stays inspectable
                    }
                    result = { output, error, frames: target.timeline_len() };
                    break;
                }
                case "run_traced_with_patch": {
                    let output = "";
                    let error = null;
                    try {
                        output = target.run_traced_with_patch(
                            msg.source, msg.frame, msg.entity, msg.component,
                            msg.field, msg.value);
                    } catch (e) {
                        error = String(e);
                    }
                    result = { output, error, frames: target.timeline_len() };
                    break;
                }
                case "world":
                    result = { json: target.timeline_world(msg.frame) };
                    break;
                case "events":
                    result = { json: target.timeline_events() };
                    break;
                case "why":
                    result = { text: target.why_at(msg.frame, msg.entity, msg.component) };
                    break;
                default:
                    throw new Error(`unknown target op ${msg.op}`);
            }
            self.postMessage({ type: "target", reqId: msg.reqId, op: msg.op, result });
        }
    } catch (err) {
        self.postMessage({ type: "error", reqId: msg.reqId, error: String(err) });
    }
};
