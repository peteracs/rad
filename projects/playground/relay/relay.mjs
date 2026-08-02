// Tier-2 #2 — the WebSocket relay that takes the D4 collab demo off
// BroadcastChannel (same-process IPC) and onto a real network stack.
// Rooms are plain fan-out: every message a peer sends is forwarded,
// verbatim, to every OTHER peer in the same room. All session semantics
// (host election, state handshake, fork_delta streaming, digest receipts)
// stay in the rad VM and collab.html — the relay knows nothing.
//
//   node projects/playground/relay/relay.mjs [port]      (binds 0.0.0.0)
//
// Two-machine run: start the relay and a static server on machine A,
// open  http://<A's-LAN-IP>:8377/collab.html?relay=ws://<A's-LAN-IP>:8378
// from machine B. Same protocol, zero code changes.

import { WebSocketServer } from "ws";

const port = Number(process.argv[2] || 8378);
const wss = new WebSocketServer({ host: "0.0.0.0", port });

/** room -> Set<ws> */
const rooms = new Map();

wss.on("connection", (ws, req) => {
    const url = new URL(req.url, "http://relay");
    const room = url.searchParams.get("room") || "default";
    let peers = rooms.get(room);
    if (!peers) {
        peers = new Set();
        rooms.set(room, peers);
    }
    peers.add(ws);
    console.log(`[relay] +peer room=${room} (${peers.size} in room)`);

    ws.on("message", (data, isBinary) => {
        for (const peer of peers) {
            if (peer !== ws && peer.readyState === peer.OPEN) {
                peer.send(data, { binary: isBinary });
            }
        }
    });
    ws.on("close", () => {
        peers.delete(ws);
        console.log(`[relay] -peer room=${room} (${peers.size} in room)`);
        if (peers.size === 0) rooms.delete(room);
    });
    ws.on("error", () => {});
});

console.log(`[relay] listening on ws://0.0.0.0:${port}`);
