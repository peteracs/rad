// The 10k-edit script, Automerge side. Same MINSTD stream over 2000 keys.
// One change per edit = one keystroke per edit (Automerge's own model:
// every change is a commit).
import * as Automerge from "@automerge/automerge";

let base = Automerge.from({ cells: {} });
base = Automerge.change(base, (d) => {
    for (let i = 0; i < 2000; i++) d.cells[`cell_${i}`] = 0;
});

const t0 = Date.now();
let ours = base;
let x = 7;
for (let k = 0; k < 10000; k++) {
    x = (x * 48271) % 2147483647;
    const key = `cell_${x % 2000}`;
    const val = x % 1000000;
    ours = Automerge.change(ours, (d) => { d.cells[key] = val; });
}
const t1 = Date.now();

const changes = Automerge.getChanges(base, ours);
const deltaBytes = changes.reduce((a, c) => a + c.length, 0);
const t2 = Date.now();
let peer = Automerge.clone(base);
[peer] = Automerge.applyChanges(peer, changes);
const t3 = Date.now();

let sum = 0;
for (const v of Object.values(peer.cells)) sum += v;
console.log(`CHECK ${sum}`);
console.log(`APPLY_MS ${t1 - t0}`);
console.log(`DELTA_BYTES ${deltaBytes}`);
console.log(`INGEST_MS ${t3 - t2}`);
