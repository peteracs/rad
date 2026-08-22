# ForgeLink

ForgeLink is an industrial telemetry gateway and the native-fidelity acceptance project. Device and session IDs retain distinct nominal identities, scalar widths and overflow are exact, enum and bitflag tags survive runtime and replay, and the packed `TelemetryFrame` has a deterministic 16-byte ABI on every host.

```bash
rad projects/dogfood/forgelink/main.rad --record forgelink.radr
rad replay forgelink.radr
rad test projects/dogfood/forgelink/tests
```

`main.rad` verifies every field offset, decodes a captured little-endian frame, stores it through an owned atomic boundary, re-encodes each scalar canonically, and checks 5,000 signed/unsigned corpus values. `bench.rad` performs 200,000 scalar round trips.

Failure fixtures cover nominal ID substitution, integer overflow, wrong endianness, and overlapping bit masks. Each fails where the representation assumption is introduced.

Score: learnability 2, ownership clarity 2, error quality 2, observability 2, determinism 2, performance 2, testability 2, refactor safety 1, host safety 2, production realism 2 — **19/20**.
