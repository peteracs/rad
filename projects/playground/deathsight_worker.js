// DEATHSIGHT browser game — worker side.
// The whole rad VM (compiled to WebAssembly) forks universes in here so the
// page never freezes while the sight prices your moves or necromancy digs
// through rewound timelines.

import init, { RadRuntime } from "./pkg/rad_vm.js";

let ready = init().then(() => new RadRuntime());

self.onmessage = async (ev) => {
    const { source, mode, script, seed } = ev.data;
    try {
        const runtime = await ready;
        runtime.reset();
        const src = source
            .replace(/^let MODE = ".*"$/m, `let MODE = "${mode}"`)
            .replace(/^let SCRIPT = ".*"$/m, `let SCRIPT = "${script}"`)
            .replace(/^let SEED = \d+$/m, `let SEED = ${seed}`);
        const t0 = performance.now();
        const out = runtime.compile_and_run(src);
        const ms = performance.now() - t0;
        self.postMessage({ ok: true, output: out, ms });
    } catch (e) {
        self.postMessage({ ok: false, error: String(e) });
    }
};
