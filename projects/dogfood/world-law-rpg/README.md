# World-Law RPG dogfood

This headless scenario is the RFC-0001/0002/0003 vertical acceptance slice.
It is compiled and executed by the VM test suite; it is not illustrative
pseudocode.

The authoritative gameplay surface is expressed through laws, intents,
resolvers, relation facts, derivations, and candidate constraints:

- actor capacity and armor configuration;
- inventory ownership and item weight;
- unique-key trading;
- aggregate encumbrance;
- movement and rooted-state rejection;
- combat, shielding, and health bounds;
- spell casting, silence, and mana bounds;
- threat declaration, incoming-damage aggregation, and danger derivation;
- a capability-restricted bot that may write only `BotIntent` on a fork;
- assertion/proof provenance and `why_fact()`;
- failed-settlement observational replay;
- relation-aware save/load identity;
- entity despawn with canonical relation cascades.

`main.rad` owns causal behavior and components. `relations.rad` owns the
authoritative and derived relation schema. The Rust harness installs the
sealed relation manifest, drives public commands, and verifies the complete
world after every accepted or rejected candidate.
