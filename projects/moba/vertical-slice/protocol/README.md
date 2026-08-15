# MOBA wire protocol

`contract.json` is the only hand-edited authority for the match wire layout,
numeric identities, packet sizes, and cross-endpoint code tables.

Run:

```text
node protocol/generate.mjs
```

to refresh the checked-in RAD and TypeScript bindings. CI runs the generator
with `--check`; endpoint code must not duplicate contract constants manually.
The edge proxy deliberately does not import this contract because it forwards
opaque bytes without interpreting them.
