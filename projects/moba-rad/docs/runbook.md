# Runbook

## 0. Build the RAD wasm (prerequisite)

The browser client loads the RAD VM from `projects/playground/pkg`. This package
is git-ignored and is **not** rebuilt by `npm run dev`, so a missing or stale
build silently breaks the client: `session_start` runs an old module, the local
avatar is never seeded into the RAD world, and the champion stays frozen at spawn
while the server-fed ghost (visible under `F3`) keeps moving. Rebuild it after any
change to `core/vm` (and once on a fresh checkout):

```powershell
cd path\to\rad
wasm-pack build --target web core/vm/ --out-dir ../../projects/playground/pkg
```

`--out-dir` is resolved relative to the `core/vm` crate, so this lands the
`rad_vm.js` + `rad_vm_bg.wasm` pair in `projects/playground/pkg`. After
rebuilding, restart `npm run dev` for the client (the dev client also cache-busts
the wasm fetch, so a plain reload normally suffices).

On boot the client logs `[moba-rad] RAD world seeded local avatar
player_id=<id> ...`, read straight from the wasm VM world. The local avatar is
seeded by a `SeedLocalAvatar { player_id }` event the host emits at boot
(`radHost.create`) using this client's persistent per-tab id from
`app/matchIdentity.ts` — it is **not** baked into the RAD source. If that line
instead reports the id was not seeded, it prints the exact cause and remedy: a
stale Vite virtual module missing the `SeedLocalAvatar` handler (restart
`npm run dev`). Note the local champion is the first-allocated entity (RAD entity
**id 0**); the render bridge must treat id 0 as a real entity
(`isRenderableEntityId`) — a `<= 0` guard there will silently drop the champion
while higher-id server entities still render.

### Identity (why two tabs now sync)

Each tab persists a unique `(session_id, player_id)` in `sessionStorage`. Two
tabs therefore claim two distinct ids and the server (which keys peers by
`player_id` and rejects a second session reusing a live id) accepts both — they
see and sync with each other. Reloading a tab reuses the same stored pair, so the
server's `remember_peer` re-finds the existing peer and you reconnect to the
**same** avatar instead of spawning a fresh one. Override a tab's id explicitly
with `VITE_MOBA_RAD_PLAYER_ID`.

Then open three terminals.

## 1. RAD Authority

```powershell
cd path\to\rad\projects\moba-rad\server
npm run dev
```

Default authority socket:

```text
127.0.0.1:8788
```

If `target/debug/rad.exe` has not been built:

```powershell
cd path\to\rad\projects\moba-rad\server
npm run dev:cargo
```

## 2. WebTransport Edge Proxy

```powershell
cd path\to\rad\projects\moba-rad\server
npm run proxy
```

Default WebTransport URL (IPv4 loopback, to match the proxy's IPv4 bind):

```text
https://127.0.0.1:4433/match
```

Default forwarded authority address:

```text
127.0.0.1:8788
```

## 3. Browser Client

```powershell
cd path\to\rad\projects\moba-rad\client
npm run dev
```

Open:

```text
http://127.0.0.1:5174/
```

Playtest controls:

- Right-click the ground to move.
- Hold `Q` to show the skillshot reticle; release `Q` to cast.
- The top-right status pill shows only `Ping` and `Loss` for normal play.
- Press `F3` or `~` to open the full netcode HUD and show raw server-position
  ghost overlays.
- A red border flash means the client had to apply a hard reconciliation snap.

## Certificate Setup

**The default workflow needs zero certificate steps.** Just start the proxy and
the client:

```powershell
npm run proxy   # mints + persists a self-signed cert on first boot
npm run dev     # Vite reads that same cert and pins its hash automatically
```

How it works:

- On first boot the proxy generates an ECDSA P-256 self-signed identity (SANs
  `localhost`, `127.0.0.1`, `::1`) and **persists it** to
  `server/edge-proxy/.dev-certs/localhost.crt` + `localhost.key` (gitignored).
- Every later boot **loads the same files**, so the SHA-256 fingerprint is
  stable across restarts — it no longer rotates on every boot.
- The Vite dev server reads that exact `.crt` file, hashes its DER the same way
  Chromium's `serverCertificateHashes` path does, and injects
  `VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH` for you. **No copy-paste, ever.**
- The proxy still prints `[WebTransport] Certificate SHA-256 fingerprint: …` for
  reference, and logs whether it `loaded persisted` or `generated` the cert.

**14-day auto-rotation.** W3C/Chromium reject browser-pinned self-signed certs
older than two weeks. The proxy mints certs for 14 days and **rotates one day
early**: on startup, if `localhost.crt` is ≥13 days old it deletes the pair and
regenerates. After a rotation just restart `npm run dev` so Vite re-reads the
new hash. (The fingerprint only changes on rotation, not on normal restarts.)

Optional overrides:

- `MOBA_RAD_CERT_DIR` — change where the persisted cert pair lives. Vite honors
  the same variable so the hash stays in sync.
- `MOBA_RAD_CERT_PEM` + `MOBA_RAD_KEY_PEM` — bypass persistence entirely and load
  a specific (e.g. browser-trusted) cert. Set both or neither.
- `VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH` — an explicit env value always wins over
  the auto-derived hash (use this in CI or with a browser-trusted cert).

```powershell
# Browser-trusted cert (no hash pinning needed if the CA is trusted):
$env:MOBA_RAD_CERT_PEM="H:\path\to\localhost.pem"
$env:MOBA_RAD_KEY_PEM="H:\path\to\localhost-key.pem"
npm run proxy
```

To verify a PEM manually, use a text fingerprint command — do **not** pipe DER
bytes through PowerShell, which can reinterpret binary pipe data and produce a
different digest (Chromium then rejects the handshake as `certificate unknown`):

```powershell
openssl x509 -in .\localhost.pem -noout -fingerprint -sha256
```

For browser-pinned self-signed WebTransport certificates the key MUST be ECDSA
P-256 and the validity period MUST be under 14 days. RSA certificates can look
valid in normal TLS tools but are rejected by Chromium's
`serverCertificateHashes` path with `CERTIFICATE_VERIFY_FAILED` /
`certificate unknown`.

Optional overrides:

| Variable | Default |
|---|---|
| `MOBA_RAD_WEBTRANSPORT_BIND` | `127.0.0.1:4433` |
| `MOBA_RAD_AUTHORITY_ADDR` | `127.0.0.1:8788` |
| `MOBA_RAD_UDP_BIND` | `127.0.0.1:0` |
| `MOBA_RAD_CERT_DIR` | `server/edge-proxy/.dev-certs` (persisted cert pair) |
| `MOBA_RAD_CERT_PEM` / `MOBA_RAD_KEY_PEM` | unset (bypass persistence, load explicit cert) |
| `MOBA_RAD_CHAOS_LATENCY_MS` | `0` (chaos off) |
| `MOBA_RAD_CHAOS_JITTER_MS` | `0` (chaos off) |
| `MOBA_RAD_CHAOS_LOSS_PCT` | `0` (chaos off) |
| `VITE_MOBA_RAD_WEBTRANSPORT_URL` | `https://127.0.0.1:4433/match` |
| `VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH` | unset |
| `VITE_MOBA_RAD_PLAYER_ID` | `1` |
| `VITE_MOBA_RAD_NETCODE_LOG` | unset; set to `1` only for soak/chaos report logging |

Authority timing and lifecycle defaults live in the RAD `ServerConfig`
resource, including `tick_hz = 128`, `udp_receive_budget = 64`,
`udp_idle_timeout_ms = 1`, and `peer_timeout_ms = 10000`.

## Troubleshooting: client shows "offline" / connection hangs

WebTransport runs over HTTP/3 (QUIC), and browsers surface almost nothing about
a failed handshake for security reasons — the connection simply hangs and the
status pill drops to `offline`. Check these in order; the first is the cause
~90% of the time:

1. **Cert hash mismatch (`CERTIFICATE_VERIFY_FAILED` / `certificate unknown`).**
   The proxy now persists its self-signed cert to `server/edge-proxy/.dev-certs`
   and Vite auto-injects the matching hash, so this should be rare. It happens
   when the client's pinned hash drifts from the proxy's live cert:
   - **You restarted the proxy after a 13-day rotation** but did not restart
     `npm run dev`. Vite reads the cert hash once at startup — restart it so it
     picks up the rotated cert.
   - **A stale `VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH` is set in your shell/env.**
     An explicit env value overrides the auto-derived hash. Unset it (or update
     it to the proxy's printed fingerprint) and restart `npm run dev`.
   - **The proxy and Vite disagree on `MOBA_RAD_CERT_DIR`.** Set the same value
     for both, or leave both unset to use the default location.

   The proxy prints its current fingerprint on every boot
   (`[WebTransport] Certificate SHA-256 fingerprint: …`); the Vite dev server
   logs the hash it pinned (`[moba-rad] pinned WebTransport cert …`). These two
   must match. W3C caps browser-accepted self-signed certs at **14 days**; the
   proxy auto-rotates at 13 days, or use `MOBA_RAD_CERT_PEM`/`MOBA_RAD_KEY_PEM`
   for a browser-trusted cert that never rotates.
2. **`localhost` resolves to IPv6 but the proxy binds IPv4** —
   `net::ERR_QUIC_PROTOCOL_ERROR.QUIC_NETWORK_IDLE_TIMEOUT` with
   `num_undecryptable_packets: 0`. On Windows `localhost` resolves to `::1`
   first, but the proxy binds IPv4 `127.0.0.1:4433`, so the browser fires QUIC at
   `[::1]:4433` where nothing listens and the handshake idles out after ~4s (this
   is the 4s "syncing" stall before the pill flips to `offline`). The client now
   defaults to `https://127.0.0.1:4433/match` to avoid this. If you override
   `VITE_MOBA_RAD_WEBTRANSPORT_URL`, keep its host family equal to the proxy's
   `MOBA_RAD_WEBTRANSPORT_BIND` (both IPv4 *or* both IPv6).
3. **QUIC is disabled in the browser.** WebTransport requires HTTP/3/QUIC.
   Chrome or Edge processes launched with `--disable-quic` will never reach the
   proxy, and the proxy log will not contain `accepted WebTransport request`.

   ```powershell
   Get-CimInstance Win32_Process |
     Where-Object { $_.CommandLine -match 'chrome|msedge' } |
     Select-String -- '--disable-quic'
   ```

   Open the client in a browser profile/window that does not use that flag.
4. **A process is not running.** Three processes are required: the Vite client,
   the edge proxy (`npm run proxy`), and the RAD authority (`npm run dev`). With
   the proxy or authority down, the browser connects to nothing and times out the
   same way as case 2 (UDP has no connection-refused for QUIC). Confirm the proxy
   log shows `moba-rad edge proxy ready` and an `accepted WebTransport request`
   line when the browser connects.
5. **Port disagreement.** Defaults already agree out of the box — the proxy
   forwards to `MOBA_RAD_AUTHORITY_ADDR` (`127.0.0.1:8788`) and the RAD authority
   binds `ServerConfig.host:udp_port` (`127.0.0.1:8788`); the proxy listens on
   `4433` and the client targets `https://127.0.0.1:4433/match`. If you override
   any of these, keep the proxy's authority address equal to the RAD UDP bind.

The F3 HUD's `status` line shows the last transport error verbatim: a
`QUIC_NETWORK_IDLE_TIMEOUT` / connect error points at cases 2–4 (handshake never
completed), whereas `Timed out waiting for ... state packet` means the handshake
succeeded but the authority never answered — look at the RAD authority, not the
proxy.

## Network Chaos Testing

The edge proxy is the realistic place to emulate a degraded public-internet
path: it sits on both directions of the datagram stream while the RAD client and
authority stay oblivious. Chaos is **debug-only** and bypassed entirely unless a
knob is set, so it never touches production forwarding.

Set any of these before starting the proxy to inject chaos on *both* directions:

| Variable | Effect |
|---|---|
| `MOBA_RAD_CHAOS_LATENCY_MS` | Base one-way delay added to every datagram (e.g. `120`). |
| `MOBA_RAD_CHAOS_JITTER_MS` | Symmetric delivery variance `latency +/- U[0, jitter]` (e.g. `15`). Non-zero jitter reorders datagrams, exercising the out-of-order guards. |
| `MOBA_RAD_CHAOS_LOSS_PCT` | Per-datagram drop probability as a percent (e.g. `5` or `7.5`). |

```powershell
cd path\to\rad\projects\moba-rad\server
$env:MOBA_RAD_CHAOS_LATENCY_MS="120"
$env:MOBA_RAD_CHAOS_JITTER_MS="15"
$env:MOBA_RAD_CHAOS_LOSS_PCT="5"
npm run proxy
```

The proxy logs a `WARN` line with the active settings on startup. Loss is rolled
first; survivors are delivered after a clamped, jittered delay (delay can never
go negative). A datagram with zero net delay is forwarded immediately so a
loss-only configuration preserves ordering.

What "passing the bar" looks like under `+120ms / 15ms jitter / 5% loss`:

- The local player's avatar still moves smoothly (client-side prediction hides
  the round trip).
- Remote avatars interpolate without teleporting (the historical timeline
  absorbs late/dropped snapshots).
- The ack-bit loss diagnostics raise the recommended input delay, and
  reconciliation snaps cleanly without visual scale/camera artifacts.

The pure chaos decision model (loss roll, jitter clamp, delay computation) is
unit tested:

```powershell
cd path\to\rad\projects\moba-rad\server
cargo test --manifest-path edge-proxy/Cargo.toml
```

## Tests

```powershell
cd path\to\rad\projects\moba-rad\client
npm test
```

```powershell
cd path\to\rad\projects\moba-rad\server
npm test
```

The server `npm test` runs the RAD smoke suites: `test:movement` (shared
movement source and anti-teleport), `test:netcode` (peer table, move/cast ring
validation, ack bookkeeping), `test:projectile` (lag-compensated skillshots),
`test:flood` (move/cast input-ring DDoS/overflow resilience), `test:collision`
(shared static-collision wall-sliding), `test:lag-window` (chaos RTT rewind
coverage), `test:shutdown` (authority lifecycle control without opening
sockets), and `test:replay` (deterministic applied-input replay tape logging).
Run any alone with, e.g., `npm run test:replay`.
