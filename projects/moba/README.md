# MOBA project boundary

All MOBA-owned code lives below this directory.

```text
moba/
├── kit/             reusable RAD gameplay corpus and golden fixtures
├── simcore/         project-specific native/WASM acceleration
└── vertical-slice/  networked product dogfood
    ├── protocol/    shared hand-edited wire authority + generators
    ├── client/      browser prediction and rendering endpoint
    ├── server/      authoritative RAD simulation endpoint
    └── edge-proxy/  opaque WebTransport/UDP forwarding endpoint
```

The protocol is not owned by either endpoint. `protocol/contract.json` is the
single hand-edited wire authority; generated RAD and TypeScript bindings are
checked into their consumers and verified by both package test commands.

`simcore/` accelerates this project only. General language or VM semantics do
not belong there.
