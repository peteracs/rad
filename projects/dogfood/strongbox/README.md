# strongbox — a tamper-evident incident archive

A working incident ledger built to stress one question: **when Rad writes state
somewhere and reads it back, does the same thing come back?**

Everything here is run from the repo root with `target\debug\rad.exe`.

```powershell
powershell -File projects\dogfood\strongbox\run_all.ps1
```

## What it does

An on-call team files incidents, escalates them, and resolves them. The archive
must survive being written to disk, migrated across three schema generations,
shipped over the wire as fork bytes, and replayed frame by frame — and come back
identical every time.

The app is built around a gap the harnesses found. `fork_to_bytes()` wraps its
payload in a blake3 integrity digest, so a tampered fork is rejected on ingest.
`save_world()` has no such envelope: an archive on disk is a plain
`RADWORLD2 {json}` document, and `load_world()` reads any well-formed edit back
as gospel. So strongbox builds the missing layer out of the language itself —
`main.rad` appends a `world_digest()` receipt after every transition and stores
the chain in a separate file, and `verify.rad` re-derives the digest and
compares. An attacker now has to forge two artifacts consistently instead of
editing one number.

## Files

| file | what it is |
|---|---|
| `main.rad` | the app: reads `feed.txt`, drives incidents through events, appends a receipt per transition, forecasts SLA breaches in a fork, seals the archive |
| `verify.rad` | reloads an archive and checks it against the receipt chain |
| `forge.rad` | the adversary: four plausible edits to a sealed archive |
| `gen1_seed.rad` → `gen2_migrate.rad` → `gen3_migrate.rad` | three schema generations; gen3 reads *both* older formats |
| `roundtrip.rad` | 15 executable round-trip contracts, prints a failure count |
| `tamper_wire.rad` | 12 adversarial fork payloads |
| `tamper_save.rad` + `run_tamper_saves.ps1` | 15 corrupted saves, one process each |
| `run_tamper_traces.ps1` | 8 byte-level corruptions of a recorded `.radr` |
| `fixed.rad` | `main.rad` with one changed SLA constant, for `replay --with` |
| `tests.rad` | the same contracts as `test` blocks — **currently dead code**, see bug 08 |
| `bugs/` | minimal repros, one per finding |

## Results

Three of the four persistence surfaces are airtight, and they are exactly the
three that carry a blake3 envelope:

| surface | envelope | adversarial result |
|---|---|---|
| fork bytes / RADPACK | blake3 | 12/12 tampered payloads rejected |
| recorded traces | blake3 + `source_hash` | 8/8 tampered traces rejected |
| `save_world()` | **none** | 4/15 corruptions silently accepted |

Other things that hold up well:

- **Migration is confluent.** Migrating a gen1 save directly to gen3 and
  migrating it gen1 → gen2 → gen3 produce the identical `world_digest`
  (`1760db34…`). One `migrate` block reads two ancestor shapes by sniffing
  `contains(old, "sev")`, because `old` is a plain map.
- **Replay is exact and side-effect-free.** The replayed run reproduces
  `world_digest f850c415…` bit-for-bit, and `write_file` is virtualised: I
  deleted the archive and the receipt log, replayed, and both stayed deleted
  while the run reported success.
- **Numeric fidelity is exact** for `i64::MAX`, `i64::MIN`, `-0.0`, subnormals,
  and integral floats — `3.0` comes back as `float`, not `int`.
- **`fmt` is idempotent and semantics-preserving**: identical digest before and
  after formatting the whole directory.

## Bugs found

| # | severity | what |
|---|---|---|
| 01 | HIGH | `load_world()` validates field *names* but not *types*: a `str` lands in an `int` field, an `int` in a `bool` field, silently |
| 02 | HIGH | a `migrate` block's return value is unchecked, so a one-word typo poisons the archive — and the poison survives `save_world()` and crosses the digest-verified wire codec |
| 03 | MED-HIGH | 4 of 15 corrupt saves load silently: a JSON `null` into a `bool` field (the docs promise "no silent nulls, ever"), a duplicate entity name that silently erases one entity's identity, an out-of-range int that becomes a float |
| 04 | HIGH | `save_world()` can write a save `load_world()` refuses: `f64::MAX` serialises to 309 expanded decimal digits that re-parse out of range. Both persistence paths break; the failure is deferred to load time |
| 05 | MED-HIGH | `replay --with` halts on any edit that changes what the program *writes*, and blames "the recorded session never performed that io" when it did |
| 06 | MEDIUM | `replay --to-frame N` silently ignores `N=0` and every out-of-range `N`, then prints "Replay verified" |
| 07 | MEDIUM | lint `RAD-L009` calls a system "never run" when `simulate()` runs it, and suggests `run SystemName`, which does not parse |
| 08 | CRITICAL | `rad test` never executes `test` blocks — a file asserting `1 == 2` twice reports PASS with exit 0 |
| 08b | HIGH | the documented property-test form `test X for v in gen_int()` does not parse |

Full write-ups with expected-vs-actual are in the swarm mailbox under topic
`bugs` (seq 46, 51, 60, 68, 73).
