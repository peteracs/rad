// RADTACTICS browser arena — worker side.
// The whole rad VM (compiled to WebAssembly) thinks in here so the page
// never freezes while the oracle forks universes.

import init, { RadRuntime } from "./pkg/rad_vm.js";

let ready = init().then(() => new RadRuntime());

self.onmessage = async (ev) => {
    const { source, seed } = ev.data;
    try {
        const runtime = await ready;
        runtime.reset();
        const src = source.replace(/^let SEED = \d+$/m, `let SEED = ${seed}`);
        const t0 = performance.now();
        const out = runtime.compile_and_run(src);
        const ms = performance.now() - t0;
        self.postMessage({ ok: true, output: out, ms });
    } catch (e) {
        self.postMessage({ ok: false, error: String(e) });
    }
};
