# Stockroom: the incident `rad blame` was built for

An order desk keeps stock, reservations and shipments, and audits itself at
the end of the day. This build has a bug: `cancel` puts an order's quantity
back on the shelf without checking that the order is still open. Cancel an
order that already shipped and the shelf count grows by goods that left the
building. Nothing fails until the audit, hundreds of commands later.

| File | What it is |
|---|---|
| `stockroom.rad` | the program, with the bug |
| `session.txt` | the incident: one day, 400 commands, one of them the customer of order `o17` cancelling after dispatch |
| `make_session.py` | writes `session.txt` (seeded; every command is one a clerk could type) |
| `experiment.py` | lineage against intervention on random days |

## The incident

```bash
rad --record day.radr projects/dogfood/stockroom/stockroom.rad < projects/dogfood/stockroom/session.txt
```

```text
AUDIT FAILED hinge: received 708, accounted for 764
```

56 hinges too many. Ask where the shelf count came from:

```text
Stock of hinge = { on_hand: 204, received: 708, shipped: 509 }   (set in frame 397)
  <- by `on Received` handler
  <- Received { sku: "hinge", qty: 38 } emitted in frame 396
  <- by top-level code
```

`why()` is correct and no help. The last writer of the shelf count is a
routine delivery at command 397. The discrepancy was put there 325 commands
earlier and has been carried through every write since.

Ask which commands the failed audit depends on:

```bash
rad blame day.radr --output "AUDIT FAILED hinge" --why hinge Stock --write-script minimal.txt
```

```text
rad blame  day.radr
  symptom    the program prints "AUDIT FAILED hinge"
  session    400 recorded io, 400 inputs that can be withheld (readline)
  searched   424 histories replayed

Minimal cause: 4 of 400 inputs. These alone still produce the symptom;
withhold any one of them as well and it does not occur.

  #7    frame 6     readline  "receive hinge 366"
         without it in the full session: the symptom still occurs (another input stands in)
  #48   frame 47    readline  "order o17 hinge 56"
         without it in the full session: the symptom is gone
  #63   frame 62    readline  "ship o17"
         without it in the full session: the symptom is gone
  #72   frame 71    readline  "cancel o17"
         without it in the full session: the symptom is gone

Each input withheld alone from the full session:
  necessary      3   the symptom disappears: #48 #63 #72
  structural     0   the session cannot finish
  incidental   397   the symptom still occurs: #1-#47 #49-#62 #64-#71 #73-#400

Overdetermined: #7 is in the minimal cause yet incidental in the full
session. Another input can stand in for it, so the session holds more than
one sufficient cause; this report shows one of them.

Lineage of Stock of hinge (what why() reports):
  ...
  lineage names input #397; the minimal cause is #7 #48 #63 #72
  Not in the lineage, yet required: #7 #48 #63 #72. The chain follows the write, not the values it read.
  In the lineage, yet not required for this symptom: #397.
```

Order, ship, cancel: the same order shipped and then cancelled. The fourth
member is a delivery that gave the order something to reserve. Two
deliveries of hinges came before the order (commands 7 and 21) and either
covers it, which is why withholding this one alone changes nothing and the
report calls it overdetermined.

`minimal.txt` is the reproduction:

```text
receive hinge 366
order o17 hinge 56
ship o17
cancel o17
```

```bash
rad projects/dogfood/stockroom/stockroom.rad < minimal.txt
# AUDIT FAILED hinge: received 366, accounted for 422
```

Four lines, typed into a fresh process, fail the audit; any three pass it.
`adapters/cli/tests/blame.rs` runs exactly that: record, blame, then feed the
script and every three-line subset back through the real binary.

## How often does lineage find it?

One incident proves little, so `experiment.py` repeats it on random days: a
clerk cancels an order that already shipped with a small chance per
cancellation, the day is recorded, and `rad blame` explains the first failed
audit line with the lineage of that sku's `Stock` for comparison.

```bash
python projects/dogfood/stockroom/experiment.py --rad target/release/rad --trials 40
```

Result of that command (seed 7, 400 commands per day, release build,
8 workers, Windows host, 2026-11-11):

| | |
|---|---|
| Days with a failed audit | 40 |
| Minimal cause | 4 commands on every day: a delivery, the order, its shipment, its cancellation |
| Lineage names a command of the cause | 0 of 40 days |
| Commands between the cause and the audit | median 231 (15 to 387) |
| Histories replayed, guided (with the but-for table) | median 426 (414 to 604) |
| Histories replayed, `--strategy ddmin --no-but-for` | median 172.5 (60 to 247) |
| Wall time, guided / ddmin | median 3.5 s / 1.5 s |
| Guided and ddmin agree on the size of the cause | 40 of 40 days |

Lineage never named a command of the cause because the audit runs at the
end of the day and the last writer of a busy shelf count is almost always a
later, unrelated command. The guided search replays more histories than
plain delta debugging: its but-for pass costs one replay per command, and
what that buys is the classification of all 400, not speed.

What this does and does not show: on this program, the last writer of a
value is rarely the command that made the value wrong, and withholding
inputs finds the commands that did. It is one program with one kind of bug,
written for this purpose. The comparison says nothing about how often
lineage suffices in programs where the faulty write is also the last one.
