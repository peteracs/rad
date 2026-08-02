// The 10k-edit script, Yjs side. Same MINSTD stream over 2000 keys.
// One transaction per edit = one keystroke per edit, the same granularity
// the rad script pays (one set() per edit).
import * as Y from "yjs";

const base = new Y.Doc();
const baseMap = base.getMap("cells");
base.transact(() => {
    for (let i = 0; i < 2000; i++) baseMap.set(`cell_${i}`, 0);
});
const baseSnapshot = Y.encodeStateAsUpdate(base);
const baseSV = Y.encodeStateVector(base);

const t0 = Date.now();
let x = 7;
for (let k = 0; k < 10000; k++) {
    x = (x * 48271) % 2147483647;
    const key = `cell_${x % 2000}`;
    const val = x % 1000000;
    base.transact(() => baseMap.set(key, val));
}
const t1 = Date.now();

const delta = Y.encodeStateAsUpdate(base, baseSV);
const t2 = Date.now();
const peer = new Y.Doc();
Y.applyUpdate(peer, baseSnapshot);
Y.applyUpdate(peer, delta);
const t3 = Date.now();

let sum = 0;
peer.getMap("cells").forEach((v) => { sum += v; });
console.log(`CHECK ${sum}`);
console.log(`APPLY_MS ${t1 - t0}`);
console.log(`DELTA_BYTES ${delta.length}`);
console.log(`INGEST_MS ${t3 - t2}`);
