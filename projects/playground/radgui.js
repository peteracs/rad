// RADGUI — the one generic renderer. It knows NOTHING about any app:
// it draws whatever entities carry a `Widget` component, and pushes DOM
// interactions back into the rad session as the events declared in
// projects/dogfood/radgui/lib_gui.rad. Keyed reconciliation by entity name.

export class RadGui {
    constructor(mountEl, statusEl) {
        this.mount = mountEl;
        this.statusEl = statusEl || null;
        this.worker = new Worker("./radgui_worker.js", { type: "module" });
        this.nodes = new Map();      // entity name -> DOM element
        this.queue = [];             // pending events
        this.inflight = false;
        this.reqId = 0;
        this.tickEnabled = false;
        this.lastTick = 0;
        this.onframe = null;         // hook(parsedSnapshot) for embedders
        this.pendingTarget = new Map(); // reqId -> {resolve, reject}
        this.undoEnabled = false;
        this.previewEnabled = false;
        // incremental world mirror, fed by render deltas (id-keyed)
        this.world = { entities: new Map(), resources: {} };
        // collab (UiConfig.collab=1): tabs (BroadcastChannel) or machines
        // (?relay=ws://...) sync via deterministic id-based election with
        // heartbeat failover — edits-to-host, deltas-to-all
        this.collab = {
            enabled: false, chan: null, isHost: false, decided: false,
            id: Math.random().toString(36).slice(2) + Date.now().toString(36),
            lastPing: 0, hbTimer: null, watchTimer: null, deferTo: null,
        };
        // persist (UiConfig.persist=1): world saved to localStorage,
        // version-stamped against the app source
        this.persist = { enabled: false, restored: false, saveTimer: null, broken: false };
        this.pendingDragMove = null;
        this.awaitingHistoryOp = false;
        this.source = null;
        this.appPath = null;
        this.worker.onmessage = (ev) => this.#onWorkerMessage(ev.data);
        document.addEventListener("keydown", (ev) => {
            // Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y): the framework-level
            // undo/redo — the whole app state walks the checkpoint line
            // with zero app code. Replicas forward the request to the
            // host, which rewinds the SHARED timeline and resyncs all.
            const mod = ev.ctrlKey || ev.metaKey;
            const k = ev.key.toLowerCase();
            if (this.undoEnabled && mod && (k === "z" || k === "y")) {
                ev.preventDefault();
                const op = (k === "y" || ev.shiftKey) ? "redo" : "undo";
                if (this.collab.enabled && !this.collab.isHost) {
                    if (this.collab.decided) this.collab.chan.post({ kind: "history", op });
                    return;
                }
                this.#historyOp(op);
                return;
            }
            // text inputs own their keys; global keys go to the app
            if (ev.target && (ev.target.tagName === "INPUT" || ev.target.tagName === "TEXTAREA")) return;
            this.send("Key", { code: ev.key === " " ? "Space" : ev.key });
        });
    }

    #historyOp(op) {
        this.awaitingHistoryOp = this.collab.enabled && this.collab.isHost;
        this.worker.postMessage({ type: op, reqId: ++this.reqId });
        this.inflight = true;
    }

    async start(appPath) {
        this.world = { entities: new Map(), resources: {} };
        this.nodes.clear();
        this.appPath = appPath;
        const cleanAppPath = appPath.replace(/^\/+/, "").replace(/^dogfood\//, "projects/dogfood/");
        const [lib, app] = await Promise.all([
            fetch("../../projects/dogfood/radgui/lib_gui.rad").then(r => r.text()),
            fetch("../../" + cleanAppPath).then(r => {
                if (!r.ok) throw new Error(`cannot load ${appPath}`);
                return r.text();
            }),
        ]);
        // the page is the module loader
        this.source = lib + "\n" + app.replace(/^use "lib_gui.rad"\s*$/m, "");
        this.#status("compiling…");
        this.worker.postMessage({ type: "start", source: this.source, reqId: ++this.reqId });
        this.inflight = true;
    }

    /** Serialize the whole session (fork_to_bytes wire string). */
    state() {
        return new Promise((resolve, reject) => {
            const reqId = ++this.reqId;
            this.pendingTarget.set(reqId, { resolve, reject });
            this.worker.postMessage({ type: "state", reqId });
        });
    }

    /** Queue an event for the rad session; pumps are serialized. */
    send(name, fields) {
        this.queue.push({ name, fields });
        this.#flush();
    }

    #flush() {
        if (this.queue.length === 0) return;
        // collab replica: events belong to the host's timeline, not ours
        if (this.collab.enabled && this.collab.decided && !this.collab.isHost) {
            const events = this.queue.splice(0, this.queue.length);
            this.collab.chan.post({ kind: "edit", events });
            return;
        }
        if (this.inflight) return;
        if (this.collab.enabled && !this.collab.decided) return; // hold until role known
        const events = this.queue.splice(0, this.queue.length);
        // checkpoint only on discrete interactions — Changed (typing,
        // slider scrubs) and Tick/Hover would flood the undo ring. A drag
        // STROKE is one undo unit: checkpoint at its start only.
        const DISCRETE = new Set(["Click", "Submit", "Key"]);
        const checkpoint = this.undoEnabled && events.some(e =>
            DISCRETE.has(e.name) || (e.name === "Drag" && e.fields.phase === "start"));
        this.worker.postMessage({
            type: "events", events, checkpoint,
            wantWire: this.collab.enabled && this.collab.isHost,
            reqId: ++this.reqId,
        });
        this.inflight = true;
    }

    /** Run an operation against the worker's TARGET runtime (RADSCOPE). */
    target(op, args = {}) {
        return new Promise((resolve, reject) => {
            const reqId = ++this.reqId;
            this.pendingTarget.set(reqId, { resolve, reject });
            this.worker.postMessage({ type: "target", op, reqId, ...args });
        });
    }

    /** why() against the live session (inspect mode). */
    why(entityName, component) {
        return new Promise((resolve, reject) => {
            const reqId = ++this.reqId;
            this.pendingTarget.set(reqId, { resolve, reject });
            this.worker.postMessage({ type: "why", entity: entityName, component, reqId });
        });
    }

    /** Simulate an event in a fork; resolves to the speculative snapshot. */
    preview(name, fields) {
        return new Promise((resolve, reject) => {
            const reqId = ++this.reqId;
            this.pendingTarget.set(reqId, { resolve, reject });
            this.worker.postMessage({ type: "preview", name, fields, reqId });
        });
    }

    #onWorkerMessage(msg) {
        const isAsync = msg.type === "target" || msg.type === "why"
            || msg.type === "preview" || msg.type === "state";
        if (isAsync || (msg.type === "error" && this.pendingTarget.has(msg.reqId))) {
            const p = this.pendingTarget.get(msg.reqId);
            if (p) {
                this.pendingTarget.delete(msg.reqId);
                if (msg.type === "error") p.reject(new Error(msg.error));
                else p.resolve(msg.result);
            }
            return;
        }
        // collab host: ship the wire divergence to every replica
        if (msg.wire && this.collab.enabled && this.collab.isHost) {
            this.collab.chan.post({ kind: "delta", wire: msg.wire });
        }
        this.inflight = false;
        if (msg.type === "error") {
            this.#status("");
            console.error("[radgui]", msg.error);
            this.#renderError(msg.error);
            return;
        }
        this.#status("");
        if (msg.output) {
            for (const line of msg.output.split("\n")) {
                if (line) console.log("[rad]", line);
            }
        }
        let delta;
        try {
            delta = JSON.parse(msg.delta);
        } catch (e) {
            console.error("[radgui] delta parse failed", e, String(msg.delta).slice(0, 400));
            return;
        }
        this.#applyDelta(delta);
        this.render();
        if (this.onframe) this.onframe(this.snapView(), msg.output || "");
        this.#maybeTick();
        this.#scheduleSave();
        // undo/redo rewound the host's shared timeline: replicas can't
        // follow a rewind by delta, so they get a full state resync
        if (this.awaitingHistoryOp) {
            this.awaitingHistoryOp = false;
            if (this.collab.enabled && this.collab.isHost) {
                this.state().then(({ state }) =>
                    this.collab.chan.post({ kind: "sync", state })
                ).catch(() => {});
            }
        }
        // a drag move that arrived while we were busy goes out now
        if (this.pendingDragMove && this.queue.length === 0) {
            const m = this.pendingDragMove;
            this.pendingDragMove = null;
            this.send("Drag", m);
        }
        this.#flush(); // events queued while we were busy
    }

    #applyDelta(delta) {
        for (const ent of delta.upsert || []) {
            this.world.entities.set(ent.id, ent);
        }
        for (const id of delta.remove || []) {
            this.world.entities.delete(id);
        }
        for (const [name, fields] of Object.entries(delta.resources || {})) {
            this.world.resources[name] = fields;
        }
    }

    /** Full-snapshot-shaped view of the incremental mirror (for bridges). */
    snapView() {
        return { entities: [...this.world.entities.values()], resources: this.world.resources };
    }

    #maybeTick() {
        const cfg = this.world.resources.UiConfig;
        const want = cfg && cfg.tick === 1;
        this.undoEnabled = !!(cfg && cfg.undo === 1);
        this.previewEnabled = !!(cfg && cfg.preview === 1);
        if (cfg && cfg.collab === 1 && !this.collab.enabled) this.#initCollab();
        if (cfg && cfg.persist === 1 && !this.persist.enabled) this.#initPersist();
        if (cfg && cfg.title) {
            document.title = cfg.title;
            const h = document.getElementById("radgui-title");
            if (h) h.textContent = cfg.title;
        }
        if (want && !this.tickEnabled) {
            this.tickEnabled = true;
            this.lastTick = performance.now();
            requestAnimationFrame(() => this.#tickLoop());
        }
        if (!want) this.tickEnabled = false;
    }

    // ---- collab: deterministic election + heartbeat failover ------------
    // Transport: BroadcastChannel for tabs; ?relay=ws://host:port for
    // machines (the same dumb fan-out relay the D4 demo used). Election:
    // claim-with-lowest-id-wins — two tabs claiming simultaneously resolve
    // deterministically instead of split-braining. The host heartbeats;
    // replicas that lose the pulse re-elect FROM THEIR CURRENT WORLD (all
    // replicas hold identical state, so any of them can take over and the
    // delta stream continues seamlessly).
    #makeChannel(name) {
        const relay = new URLSearchParams(location.search).get("relay");
        if (relay) {
            const ws = new WebSocket(`${relay}?room=${encodeURIComponent(name)}`);
            const pending = [];
            ws.onopen = () => { for (const m of pending.splice(0)) ws.send(m); };
            const out = {
                post: (o) => {
                    const s = JSON.stringify(o);
                    if (ws.readyState === WebSocket.OPEN) ws.send(s);
                    else pending.push(s);
                },
                handler: null,
            };
            ws.onmessage = async (ev) => {
                const text = typeof ev.data === "string" ? ev.data : await ev.data.text();
                if (out.handler) out.handler(JSON.parse(text));
            };
            return out;
        }
        const bc = new BroadcastChannel(name);
        const out = { post: (o) => bc.postMessage(o), handler: null };
        bc.onmessage = (ev) => { if (out.handler) out.handler(ev.data); };
        return out;
    }

    #becomeHost() {
        const c = this.collab;
        c.decided = true;
        c.isHost = true;
        this.#status("");
        if (this.persist.enabled) this.#restorePersisted();
        c.hbTimer = setInterval(() => c.chan.post({ kind: "ping", id: c.id }), 1500);
        this.#flush();
    }

    #becomeReplica() {
        const c = this.collab;
        c.decided = true;
        c.isHost = false;
        c.lastPing = performance.now();
        this.persist.restored = true; // the host's live state wins
        this.#status("synced as replica — edits go to the host");
        // lose the pulse -> re-elect from our own (identical) world
        c.watchTimer = setInterval(() => {
            if (performance.now() - c.lastPing > 5000) {
                clearInterval(c.watchTimer);
                c.decided = false;
                this.#status("host lost — re-electing…");
                this.#elect();
            }
        }, 1000);
    }

    #elect() {
        const c = this.collab;
        c.deferTo = null;
        c.chan.post({ kind: "hello", id: c.id });
        setTimeout(() => {
            if (c.decided) return;
            c.chan.post({ kind: "claim", id: c.id });
            setTimeout(() => {
                if (c.decided) return;
                // lowest id wins a contested claim
                if (c.deferTo && c.deferTo < c.id) {
                    c.chan.post({ kind: "hello", id: c.id }); // ask the winner
                    setTimeout(() => { if (!c.decided) this.#becomeHost(); }, 600);
                } else {
                    this.#becomeHost();
                }
            }, 300);
        }, 350);
    }

    #initCollab() {
        this.collab.enabled = true;
        const c = this.collab;
        c.chan = this.#makeChannel("radgui:" + this.appPath);
        c.chan.handler = async (m) => {
            if (m.kind === "hello" && c.isHost) {
                const { state } = await this.state();
                c.chan.post({ kind: "host-here", state, id: c.id });
            } else if (m.kind === "host-here" && !c.decided) {
                this.worker.postMessage({ type: "load", state: m.state, reqId: ++this.reqId });
                this.inflight = true;
                this.#becomeReplica();
            } else if (m.kind === "claim") {
                if (c.isHost) {
                    // a claimant raced our heartbeat: reassert
                    const { state } = await this.state();
                    c.chan.post({ kind: "host-here", state, id: c.id });
                } else if (!c.decided) {
                    c.deferTo = c.deferTo && c.deferTo < m.id ? c.deferTo : m.id;
                }
            } else if (m.kind === "ping") {
                c.lastPing = performance.now();
                if (c.isHost && m.id < c.id) {
                    // two hosts (race or rejoin): deterministic loser demotes
                    clearInterval(c.hbTimer);
                    c.decided = false;
                    this.#elect();
                }
            } else if (m.kind === "edit" && c.isHost) {
                for (const e of m.events) this.queue.push(e);
                this.#flush();
            } else if (m.kind === "history" && c.isHost) {
                this.#historyOp(m.op === "redo" ? "redo" : "undo");
            } else if (m.kind === "delta" && c.decided && !c.isHost) {
                this.worker.postMessage({ type: "apply", wire: m.wire, reqId: ++this.reqId });
                this.inflight = true;
            } else if (m.kind === "sync" && c.decided && !c.isHost) {
                this.worker.postMessage({ type: "load", state: m.state, reqId: ++this.reqId });
                this.inflight = true;
            }
        };
        this.#elect();
    }

    // ---- persist: the world survives refresh (fork_to_bytes wire) -------
    // Version-stamped against the app source: stale saves are discarded
    // loudly, not loaded into code that no longer matches them. Saves are
    // debounced per frame (300 ms), not on a blind interval.
    #persistKey() { return "radgui-state:" + this.appPath; }

    #sourceHash() {
        let h = 5381;
        const s = this.source || "";
        for (let i = 0; i < s.length; i++) h = ((h * 33) ^ s.charCodeAt(i)) >>> 0;
        return h.toString(36);
    }

    #initPersist() {
        this.persist.enabled = true;
        // restore now, or — under collab — once the election decides
        // (replicas adopt the host's live state; nobody saves until the
        // restore had its chance, so a fresh session can't clobber storage)
        if (!this.collab.enabled) this.#restorePersisted();
    }

    #scheduleSave() {
        if (!this.persist.enabled || !this.persist.restored || this.persist.broken) return;
        if (this.collab.enabled && !this.collab.isHost) return;
        clearTimeout(this.persist.saveTimer);
        this.persist.saveTimer = setTimeout(async () => {
            try {
                const { state } = await this.state();
                localStorage.setItem(this.#persistKey(),
                    JSON.stringify({ v: this.#sourceHash(), state }));
            } catch (e) {
                // quota blown (or storage denied): say so ONCE, stop trying
                this.persist.broken = true;
                this.#renderError(`state too large for localStorage — persistence disabled (${e.name})`);
            }
        }, 300);
    }

    #restorePersisted() {
        if (this.persist.restored) return;
        this.persist.restored = true;
        const raw = localStorage.getItem(this.#persistKey());
        if (!raw) return;
        try {
            const { v, state } = JSON.parse(raw);
            if (v !== this.#sourceHash()) {
                localStorage.removeItem(this.#persistKey());
                this.#renderError("saved state was from an older version of this app — discarded");
                return;
            }
            this.worker.postMessage({ type: "load", state, reqId: ++this.reqId });
            this.inflight = true;
        } catch {
            localStorage.removeItem(this.#persistKey()); // pre-stamp format
        }
    }

    #tickLoop() {
        if (!this.tickEnabled) return;
        const now = performance.now();
        const dt = (now - this.lastTick) / 1000;
        this.lastTick = now;
        // one Tick per frame, but never stack pumps
        if (!this.inflight && this.queue.length === 0) {
            this.send("Tick", { dt });
        }
        requestAnimationFrame(() => this.#tickLoop());
    }

    // ---- rendering --------------------------------------------------------

    render() {
        const widgets = new Map(); // name -> {kind,parent,order,comps}
        for (const ent of this.world.entities.values()) {
            const byType = {};
            for (const c of ent.components) byType[c.type] = c.fields;
            if (!byType.Widget) continue;
            const name = ent.name || `eid-${ent.id}`;
            widgets.set(name, { ...byType.Widget, comps: byType, name });
        }

        // group children by parent, ordered
        const children = new Map();
        for (const w of widgets.values()) {
            const list = children.get(w.parent) || [];
            list.push(w);
            children.set(w.parent, list);
        }
        for (const list of children.values()) list.sort((a, b) => a.order - b.order);

        // drop DOM nodes whose entities vanished
        for (const [name, el] of this.nodes) {
            if (!widgets.has(name)) {
                el.remove();
                this.nodes.delete(name);
            }
        }

        this.#reconcile(this.mount, children.get("") || [], children);
    }

    #reconcile(container, list, children) {
        let prev = null;
        for (const w of list) {
            let el = this.nodes.get(w.name);
            if (!el || el.dataset.kind !== w.kind) {
                if (el) el.remove();
                el = this.#create(w);
                this.nodes.set(w.name, el);
            }
            this.#patch(el, w);
            // position: after prev sibling (keyed reorder)
            if (el.parentElement !== container || el.previousElementSibling !== prev) {
                if (prev) prev.after(el);
                else container.prepend(el);
            }
            prev = el;
            this.#reconcile(el, children.get(w.name) || [], children);
        }
    }

    #create(w) {
        let el;
        switch (w.kind) {
            case "button": el = document.createElement("button"); break;
            case "pre": el = document.createElement("pre"); break;
            case "image": {
                el = document.createElement("img");
                break;
            }
            case "checkbox": {
                el = document.createElement("input");
                el.type = "checkbox";
                el.addEventListener("change", () =>
                    this.send("Changed", { target: { entity: w.name }, value: el.checked ? "1" : "0" }));
                break;
            }
            case "select": {
                el = document.createElement("select");
                el.addEventListener("change", () =>
                    this.send("Changed", { target: { entity: w.name }, value: el.value }));
                break;
            }
            case "textarea": {
                el = document.createElement("textarea");
                el.addEventListener("input", () =>
                    this.send("Changed", { target: { entity: w.name }, value: el.value }));
                el.addEventListener("keydown", (ev) => {
                    if (ev.key === "Enter" && (ev.ctrlKey || ev.metaKey))
                        this.send("Submit", { target: { entity: w.name }, value: el.value });
                });
                break;
            }
            case "input": {
                el = document.createElement("input");
                el.type = "text";
                el.addEventListener("input", () =>
                    this.send("Changed", { target: { entity: w.name }, value: el.value }));
                el.addEventListener("keydown", (ev) => {
                    if (ev.key === "Enter")
                        this.send("Submit", { target: { entity: w.name }, value: el.value });
                });
                break;
            }
            case "slider": {
                el = document.createElement("input");
                el.type = "range";
                el.addEventListener("input", () =>
                    this.send("Changed", { target: { entity: w.name }, value: el.value }));
                break;
            }
            default: el = document.createElement("div");
        }
        el.dataset.kind = w.kind;
        el.dataset.entity = w.name;
        el.classList.add("rg", `rg-${w.kind}`);
        if (w.kind === "button") {
            el.addEventListener("click", (ev) => {
                // inspect mode: alt-click any widget => why() overlay
                if (ev.altKey) {
                    ev.preventDefault();
                    ev.stopPropagation();
                    this.#inspect(w.name);
                    return;
                }
                this.send("Click", { target: { entity: w.name } });
            });
            // speculative hover: simulate the click in a fork, ghost the diff
            el.addEventListener("mouseenter", async () => {
                if (!this.previewEnabled || this.inflight) return;
                try {
                    const r = await this.preview("Click", { target: { entity: w.name } });
                    this.#ghostDiff(JSON.parse(r.json), w.name);
                } catch (e) { /* speculation is best-effort */ }
            });
            el.addEventListener("mouseleave", () => this.#clearGhosts());
        } else {
            el.addEventListener("click", (ev) => {
                if (ev.altKey) {
                    ev.preventDefault();
                    ev.stopPropagation();
                    this.#inspect(w.name);
                }
            });
        }
        return el;
    }

    /** Paint "this is what clicking would do" onto changed widgets. */
    #ghostDiff(specSnap, hoveredName) {
        this.#clearGhosts();
        const liveText = new Map();
        for (const ent of this.world.entities.values()) {
            const t = ent.components.find(c => c.type === "Text");
            if (t && ent.name) liveText.set(ent.name, String(t.fields.value));
        }
        for (const ent of specSnap.entities) {
            const t = ent.components.find(c => c.type === "Text");
            if (!t || !ent.name) continue;
            const now = liveText.get(ent.name);
            const would = String(t.fields.value);
            if (now !== undefined && now !== would) {
                const el = this.nodes.get(ent.name);
                if (el) {
                    const ghost = document.createElement("span");
                    ghost.className = "rg-ghost";
                    ghost.textContent = ` → ${would}`;
                    el.appendChild(ghost);
                    el.classList.add("rg-ghosted");
                }
            }
        }
        // structural consequences: spawned/despawned entities as a badge
        const liveIds = new Set(this.world.entities.keys());
        const specIds = new Set(specSnap.entities.map(e => e.id));
        const born = [...specIds].filter(id => !liveIds.has(id)).length;
        const dead = [...liveIds].filter(id => !specIds.has(id)).length;
        if ((born || dead) && hoveredName) {
            const el = this.nodes.get(hoveredName);
            if (el) {
                const parts = [];
                if (born) parts.push(`+${born}`);
                if (dead) parts.push(`-${dead}`);
                const ghost = document.createElement("span");
                ghost.className = "rg-ghost";
                ghost.textContent = ` ${parts.join("/")} widgets`;
                el.appendChild(ghost);
                el.classList.add("rg-ghosted");
            }
        }
    }

    #clearGhosts() {
        for (const g of [...document.querySelectorAll(".rg-ghost")]) g.remove();
        for (const el of [...document.querySelectorAll(".rg-ghosted")]) el.classList.remove("rg-ghosted");
    }

    /** Inspect overlay: the causal chain for every component on a widget. */
    async #inspect(name) {
        const old = document.getElementById("rg-inspect");
        if (old) old.remove();
        const ent = [...this.world.entities.values()].find(e => e.name === name);
        if (!ent) return;
        const parts = [];
        for (const c of ent.components) {
            try {
                const r = await this.why(name, c.type);
                parts.push(r.text);
            } catch (e) { /* unledgered component */ }
        }
        const box = document.createElement("div");
        box.id = "rg-inspect";
        box.innerHTML = `<div class="rg-inspect-head">why is <b>${name}</b> the way it is?</div>` +
            `<pre>${parts.join("\n\n") || "(no ledger entries — state set by top-level code)"}</pre>` +
            `<div class="rg-inspect-hint">alt-click anywhere to re-inspect · click to dismiss</div>`;
        box.addEventListener("click", () => box.remove());
        document.body.appendChild(box);
    }

    #patch(el, w) {
        // opt-in interaction markers (idempotent wiring)
        if (w.comps.Hoverable && !el.dataset.hoverWired) {
            el.dataset.hoverWired = "1";
            el.addEventListener("mouseenter", () =>
                this.send("Hover", { target: { entity: w.name }, entered: 1 }));
            el.addEventListener("mouseleave", () =>
                this.send("Hover", { target: { entity: w.name }, entered: 0 }));
        }
        if (w.comps.Draggable && !el.dataset.dragWired) {
            el.dataset.dragWired = "1";
            const panelPos = (ev) => {
                const host = el.closest(".rg-panel") || el;
                const r = host.getBoundingClientRect();
                return { x: Math.round(ev.clientX - r.left), y: Math.round(ev.clientY - r.top) };
            };
            let dragging = false;
            el.addEventListener("pointerdown", (ev) => {
                dragging = true;
                try { el.setPointerCapture(ev.pointerId); } catch { /* synthetic events */ }
                const p = panelPos(ev);
                this.send("Drag", { target: { entity: w.name }, x: p.x, y: p.y, phase: "start" });
            });
            el.addEventListener("pointermove", (ev) => {
                if (!dragging) return;
                const p = panelPos(ev);
                const fields = { target: { entity: w.name }, x: p.x, y: p.y, phase: "move" };
                if (!this.inflight && this.queue.length === 0) {
                    this.send("Drag", fields);
                } else {
                    // never dropped: the latest move ships right after the
                    // in-flight pump returns (apps interpolate the gap)
                    this.pendingDragMove = fields;
                }
            });
            el.addEventListener("pointerup", (ev) => {
                dragging = false;
                this.pendingDragMove = null;
                const p = panelPos(ev);
                this.send("Drag", { target: { entity: w.name }, x: p.x, y: p.y, phase: "end" });
            });
        }
        if (w.comps.Focusable && !el.dataset.focusWired) {
            el.dataset.focusWired = "1";
            el.addEventListener("focus", () =>
                this.send("Focus", { target: { entity: w.name }, focused: 1 }));
            el.addEventListener("blur", () =>
                this.send("Focus", { target: { entity: w.name }, focused: 0 }));
        }
        if (w.comps.Scrollable && !el.dataset.scrollWired) {
            el.dataset.scrollWired = "1";
            let raf = 0;
            el.addEventListener("scroll", () => {
                if (raf) return;
                raf = requestAnimationFrame(() => {
                    raf = 0;
                    this.send("Scroll", { target: { entity: w.name }, top: Math.round(el.scrollTop) });
                });
            });
        }
        // programmatic focus: app bumped FocusNow.seq -> focus once
        if (w.comps.FocusNow) {
            const seq = String(w.comps.FocusNow.seq);
            if (el.dataset.focusSeq !== seq) {
                el.dataset.focusSeq = seq;
                el.focus();
            }
        }
        // visual-only hover colors, PURE CSS (:hover + custom properties):
        // no JS save/restore, so a world update mid-hover can never get
        // stomped by a stale unhover
        const sNow = w.comps.Style || {};
        el.classList.toggle("rg-hovbg", !!sNow.hover_bg);
        el.classList.toggle("rg-hovfg", !!sNow.hover_fg);
        if (sNow.hover_bg) el.style.setProperty("--rg-hbg", sNow.hover_bg);
        else el.style.removeProperty("--rg-hbg");
        if (sNow.hover_fg) el.style.setProperty("--rg-hfg", sNow.hover_fg);
        else el.style.removeProperty("--rg-hfg");

        // ARIA: native semantics where possible, labels everywhere else
        if (w.kind === "image") {
            const src = w.comps.Src ? String(w.comps.Src.url) : "";
            if (el.src !== src) el.src = src;
            el.alt = w.comps.Text ? String(w.comps.Text.value) : w.name;
        }
        if (w.kind === "input" || w.kind === "textarea") {
            const ph = w.comps.Input && w.comps.Input.placeholder;
            if (ph) el.setAttribute("aria-label", ph);
        }
        if (w.kind === "select" && w.comps.Options) {
            const items = String(w.comps.Options.items);
            if (el.dataset.options !== items) {
                el.dataset.options = items;
                el.innerHTML = "";
                for (const o of items.split("|")) {
                    const opt = document.createElement("option");
                    opt.value = o;
                    opt.textContent = o;
                    el.appendChild(opt);
                }
            }
        }
        if (w.kind === "checkbox") {
            const inp = w.comps.Input || {};
            const want = String(inp.value) === "1";
            if (el.dataset.lastWorldChecked !== String(want)) {
                el.checked = want;
                el.dataset.lastWorldChecked = String(want);
            }
        }

        const text = w.comps.Text ? String(w.comps.Text.value) : null;
        if (w.kind === "input" || w.kind === "slider" || w.kind === "textarea" || w.kind === "select") {
            const inp = w.comps.Input || {};
            // Only write the DOM value when the WORLD's value changed —
            // comparing against the DOM would clobber in-flight typing the
            // app hasn't echoed into the Input component (and most apps
            // shouldn't have to).
            const worldVal = String(inp.value ?? "");
            if (el.dataset.lastWorldValue !== worldVal) {
                el.value = worldVal;
                el.dataset.lastWorldValue = worldVal;
            }
            if (w.kind === "input" && inp.placeholder) el.placeholder = inp.placeholder;
            if (w.kind === "slider") {
                if (el.min !== String(inp.min)) el.min = inp.min;
                if (el.max !== String(inp.max)) el.max = inp.max;
                if (el.step !== String(inp.step || 1)) el.step = inp.step || 1;
            }
        } else if (w.kind !== "image" && text !== null && el.textContent !== text) {
            el.textContent = text;
        }

        const s = w.comps.Style || {};
        const css = el.style;
        css.display = w.comps.Hidden ? "none" : "";
        if (w.kind === "col" || w.kind === "row") {
            css.display = w.comps.Hidden ? "none" : "flex";
            css.flexDirection = w.kind === "col" ? "column" : "row";
        }
        css.color = s.fg || "";
        css.background = s.bg || "";
        css.fontSize = s.size ? `${s.size}px` : "";
        css.padding = s.pad ? `${s.pad}px` : "";
        css.gap = s.gap ? `${s.gap}px` : "";
        css.width = s.width ? `${s.width}px` : "";
        css.height = s.height ? `${s.height}px` : "";
        // an explicit size is a promise: don't let a flex parent shrink it
        // (the 240,000px virtual-list spacer found this one)
        css.flexShrink = (s.width || s.height) ? "0" : "";
        css.flexGrow = s.grow ? String(s.grow) : "";
        css.fontFamily = s.mono ? "ui-monospace, Consolas, monospace" : "";
        css.overflow = s.scroll ? "auto" : "";
        css.border = s.border || "";
        css.alignItems = s.align === "center" ? "center" : (s.align || "");
        if (s.title) el.title = s.title;
        // scoped, not `all`: tweening padding/layout on every world write
        // is how you get a UI that feels like it's underwater
        css.transition = s.anim
            ? ["background-color", "color", "left", "top", "width", "height", "opacity"]
                .map(p => `${p} ${s.anim}ms ease`).join(", ")
            : "";

        // absolute layer — applied after Style so Bounds wins on size
        if (w.kind === "panel") {
            css.position = "relative";
        }
        const b = w.comps.Bounds;
        if (b) {
            css.position = "absolute";
            css.left = `${b.x}px`;
            css.top = `${b.y}px`;
            if (b.w) css.width = `${b.w}px`;
            if (b.h) css.height = `${b.h}px`;
            if (b.z) css.zIndex = String(b.z);
        }
    }

    #status(msg) {
        if (this.statusEl) this.statusEl.textContent = msg;
    }

    #renderError(err) {
        // session compile errors get the full dump; runtime handler errors
        // get a toast — the app keeps running, the user keeps clicking
        if (this.world.entities.size === 0) {
            const pre = document.createElement("pre");
            pre.className = "rg rg-error";
            pre.textContent = err;
            this.mount.appendChild(pre);
            return;
        }
        // one toast, reused: chatty errors update it instead of stacking
        let toast = document.getElementById("rg-toast");
        if (!toast) {
            toast = document.createElement("div");
            toast.id = "rg-toast";
            toast.className = "rg-toast";
            document.body.appendChild(toast);
        }
        toast.textContent = `handler error: ${String(err).slice(0, 200)}`;
        toast.classList.remove("rg-toast-out");
        clearTimeout(this._toastTimer);
        this._toastTimer = setTimeout(() => {
            toast.classList.add("rg-toast-out");
            setTimeout(() => toast.remove(), 700);
        }, 3500);
    }
}
