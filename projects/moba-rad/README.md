# MOBA RAD

`moba-rad` is the RAD MOBA dogfood stack:

- RAD authority server owns simulation, packet grammar, validation, snapshots,
  replay, and UDP lifecycle.
- Rust edge proxy terminates WebTransport/HTTP3 and forwards opaque datagrams
  to the RAD UDP authority.
- Vite + TypeScript + Three.js browser client owns input, prediction,
  reconciliation, rendering, and visual telemetry.

The edge proxy is intentionally dumb. Do not put game rules, packet parsing, or
fallback HTTP polling in it.

## Prerequisites

Optionally build the RAD CLI once from the repo root. The npm scripts fall back
to `cargo run` automatically when the binary is missing, so this only buys you
faster startup:

```powershell
cd path\to\rad
cargo build -p rad-vm --bin rad
```

Install the browser client dependencies once:

```powershell
cd path\to\rad\projects\moba-rad\client
npm install
```

Build the browser runtime once, and rebuild it after changing `core/vm`:

```powershell
cd path\to\rad
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
wasm-pack build --target web --out-dir ..\..\projects\playground\pkg core\vm
```

## Start The Stack

Open three terminals.

### 1. RAD Authority

```powershell
cd path\to\rad\projects\moba-rad\server
npm run dev
```

Default UDP authority socket:

```text
127.0.0.1:8788
```

### 2. WebTransport Edge Proxy

```powershell
cd path\to\rad\projects\moba-rad\server
npm run proxy
```

Default WebTransport URL:

```text
https://127.0.0.1:4433/match
```

The proxy prints a `VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH=<hex>` line when it
uses its local self-signed identity. Copy that value into the client terminal
before starting Vite.

### 3. Browser Client

```powershell
cd path\to\rad\projects\moba-rad\client
$env:VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH="<sha256 hex printed by proxy>"
npm run dev
```

Open:

```text
http://127.0.0.1:5174/
```

## Playtest Controls

- Right-click the ground to move.
- Hold `Q` to show the skillshot reticle; release `Q` to cast.
- Press `F3` or `~` to open the full netcode HUD.
- A red border flash means the client applied a hard reconciliation snap.

## Useful Environment Variables

| Variable | Default |
|---|---|
| `MOBA_RAD_WEBTRANSPORT_BIND` | `127.0.0.1:4433` |
| `MOBA_RAD_AUTHORITY_ADDR` | `127.0.0.1:8788` |
| `MOBA_RAD_UDP_BIND` | `127.0.0.1:0` |
| `MOBA_RAD_CHAOS_LATENCY_MS` | `0` |
| `MOBA_RAD_CHAOS_JITTER_MS` | `0` |
| `MOBA_RAD_CHAOS_LOSS_PCT` | `0` |
| `VITE_MOBA_RAD_WEBTRANSPORT_URL` | `https://127.0.0.1:4433/match` |
| `VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH` | unset |
| `VITE_MOBA_RAD_PLAYER_ID` | `1` |
| `VITE_MOBA_RAD_NETCODE_LOG` | unset; set to `1` only for soak/chaos report logging |

For stable browser runs, provide a browser-trusted certificate instead of using
the proxy's ephemeral self-signed identity:

```powershell
$env:MOBA_RAD_CERT_PEM="H:\path\to\localhost.pem"
$env:MOBA_RAD_KEY_PEM="H:\path\to\localhost-key.pem"
npm run proxy
```

## Test And Build

Client:

```powershell
cd path\to\rad
wasm-pack build --target web --out-dir ..\..\projects\playground\pkg core\vm
cd path\to\rad\projects\moba-rad\client
npm test
npm run build
```

RAD authority smoke suites:

```powershell
cd path\to\rad\projects\moba-rad\server
npm test
```

Edge proxy:

```powershell
cd path\to\rad\projects\moba-rad\server
cargo test --manifest-path edge-proxy/Cargo.toml
```

## Troubleshooting

- If the client stays offline, restart the proxy, copy the fresh certificate
  hash, set `VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH`, and restart Vite.
- Keep the URL host family aligned with the proxy bind. The defaults use IPv4:
  `https://127.0.0.1:4433/match` and `127.0.0.1:4433`.
- WebTransport requires HTTP/3/QUIC. Chrome or Edge launched with
  `--disable-quic` will not connect.
- All three processes must be running: RAD authority, edge proxy, and Vite.

## More Docs

- [Overview](docs/overview.md)
- [Runbook](docs/runbook.md)
- [Protocol Ownership](docs/protocol.md)
- [Netcode Architecture](docs/netcode.md)
- [WebTransport Networking](docs/webtransport-networking.md)
