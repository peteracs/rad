# Counterfactual Root Cause (`rad blame`)

`why()` tells you which handler wrote a value and which event it was
handling. That is lineage: a record of what happened. It does not tell you
whether the outcome *needed* any of it, and it never mentions an input whose
only contribution was a value that a later handler read.

`rad blame` asks the other question. A trace recorded with `--record` holds
every input that crossed the determinism boundary, so the session that would
have happened without one of them is not a guess: Rad can run it. `rad blame`
replays the recorded session many times with inputs withheld and reports
which of them the symptom depends on.

```bash
rad --record day.radr stockroom.rad < session.txt
rad blame day.radr --output "AUDIT FAILED hinge"
```

## What it reports

**The symptom** is the outcome to explain:

| Option | Symptom |
|---|---|
| *(none)* | The runtime error the recorded run ended in. |
| `--error <text>` | The run ends in a runtime error containing the text. |
| `--output <text>` | A printed line contains the text. |

The symptom must occur in the recorded session; `rad blame` checks that
first and refuses otherwise.

**The minimal cause** is a set of inputs that still produces the symptom when
every other input is withheld, and stops producing it when any one member is
withheld as well. That second property (1-minimality) is checked by replay
for every member before the report is printed. `--write-script <file>` writes
the cause as input lines: a reproduction you can feed straight back to the
program.

**The but-for table** classifies every input of the full session by what
happens when that input alone is withheld:

| Class | Withholding it alone… |
|---|---|
| necessary | removes the symptom |
| structural | leaves a session the program cannot complete |
| incidental | leaves the symptom in place |

The two answers differ when a symptom is **overdetermined**. If two inputs
can each trigger it, neither is necessary (the other stands in), yet a cause
exists; the minimal cause holds one of them and the report says so.

**Lineage**, with `--why <entity> <component>` or `--why-resource <resource>`,
adds the `why()` chain for that value in the recorded session and the inputs
delivered in the frame where the chain starts. The report then lists inputs
that are required but absent from the lineage, and inputs the lineage names
that the symptom does not depend on.

### An input that should matter and does not

The but-for table also shows the opposite of a cause. The RADSHEET incident
(`projects/dogfood/radsheet/incident`) is a build whose `SUM` drops the last
row of its range. Record the editing session again and ask what the wrong
total depends on:

```bash
rad --record sheet.radr projects/dogfood/radsheet/incident/buggy.rad < projects/dogfood/radsheet/incident/edits.txt
rad blame sheet.radr --output "Q1 total (B5) = 2490" --why B5 Cell
```

```text
Minimal cause: 4 of 17 inputs.
  #2    frame 2     readline  "alice B1 1200"
  #5    frame 8     readline  "bob B2 880"
  #8    frame 14    readline  "alice B3 410"
  #14   frame 26    readline  "alice B5 =SUM(B1:B4)"

Each input withheld alone from the full session:
  necessary      4   the symptom disappears: #2 #5 #8 #14
  incidental    13   the symptom still occurs: #1 #3 #4 #6 #7 #9-#13 #15-#17

  lineage names input #14; the minimal cause is #2 #5 #8 #14
  Not in the lineage, yet required: #2 #5 #8.
```

Input #11 is `bob B4 75`. B4 is inside `SUM(B1:B4)`, and the total does not
depend on it: that line of the table is the bug. Lineage names only the
keystroke that typed the formula.

## What an input is

An input is one message on an ordered external stream: a line from
`readline` or `input`, a chunk from `tcp_read`, a datagram from `udp_recv_*`.
Withholding one means that message never arrived; the rest of its stream
keeps its order.

Clock reads, file reads, HTTP calls, `sys_args` and connection setup are not
inputs in this sense. Removing one does not describe a session that could
have happened, so they are served as recorded.

"As recorded" needs care once an input is missing. A clock reading, a file's
contents and an HTTP response are samples of the outside world at a moment,
so each one stays with the input the program was handling when it asked.
With `browse` withheld, `pay` still reads the clock value `pay` read in the
recording, not the one `browse` read. A read that has no recorded answer at
that point (the withheld command was the one that loaded the configuration)
gets the most recent earlier answer. A call the recording never made with
those arguments has no answer at all: that history is classified as one the
program cannot complete, never as one without the symptom.

Random numbers are not recorded. They come from a generator seeded by the
trace, so a history with an input withheld draws what the shorter session
would have drawn.

When a history runs out of standard input, the program sees end of input,
exactly as it would from a closed pipe: `readline()` returns `""`. A program
that keeps reading a closed stdin is halted after a few empty reads. A socket
has no neutral end, so a history that asks an exhausted socket for more is
classified as one the program cannot complete.

## How the search works

Every claim in the report is the result of a replay. With the default
`--strategy guided`:

1. Replay the recorded session and confirm the symptom. Its instruction
   count bounds every alternate history at four times that plus a small
   allowance, so a history that loops is cut off.
2. Replay with every input withheld. If the symptom still occurs, no input
   causes it, and the report says to look at the program instead.
3. Withhold each input alone (the but-for pass, one replay per input).
4. Try the lineage inputs, then the necessary and structural inputs, as the
   cause. If the necessary inputs do not reproduce the symptom alone, keep
   them and run delta debugging (ddmin) over the others to find what they
   still need.
5. Withhold each member of the result in turn until none can go.

`--strategy ddmin --no-but-for` skips steps 3 and 4 and runs plain delta
debugging over all inputs. It needs fewer replays on long sessions with a
small cause, and gives no but-for table.

The two strategies can return different causes when a symptom has several:
each is 1-minimal, and neither is preferred. There is no option yet to
exclude an input and search for another cause.

### Cost on long sessions

Every history replays from the first input; nothing is resumed from a
checkpoint. The but-for pass is one full replay per input, so the default
strategy is quadratic in session length. Plain ddmin replays roughly
(cause size x log of the session length) histories when the cause is small,
most of them short, and up to the square of the session length when it is
not. Measured on stockroom days of increasing length (one hinge order
shipped and then cancelled, 8 workers, a 4-command cause near the start):

| Commands | One replay | `--strategy ddmin --no-but-for` | Default (guided) |
|---|---|---|---|
| 400 | 0.06 s | 172 histories (median of 40 days) | 424 histories, 3 s |
| 4,000 | 0.2 s | 192 histories, 1.4 s | 4,030 histories, 108 s |
| 40,000 | 1.6 s | 172 histories, 6.4 s | not run |

Use the default up to a few thousand inputs and `--strategy ddmin
--no-but-for` beyond that.

Histories run on worker threads (`--jobs`, default: the machine's
parallelism, at most 8). The result does not depend on the thread count:
each round takes the lowest-numbered success, and the set of histories
replayed is the same.

## With an edited program

`--with <file.rad>` replays every history against that program instead of
the one embedded in the trace, as `rad replay --with` does. Use it to state a
symptom the original program never checked:

```bash
# audit.rad is the recorded program plus one assertion at the end
rad blame incident.radr --with audit.rad --error "books do not balance"
```

The same option checks a fix against the recorded session. If the symptom
no longer occurs with the fixed program, `rad blame` says so and exits 1:
there is nothing left to blame.

## Limits

- An alternate history holds the rest of the world still. The other inputs
  arrive when they did, and an HTTP endpoint answers what it answered, even
  though a real server might have answered differently had an earlier
  request never been sent. The report is exact for what the program does
  with its inputs and an approximation for what the world would have done.
- Sockets, accepts and sends are served in call order per connection. A
  withheld message does not remove the replies that followed it on other
  connections.
- The symptom is matched as text. `--output "total = 2490"` asks about that
  line; a history that prints a different wrong total counts as one without
  the symptom. State the real condition as an assertion in a `--with`
  program when that matters.
- The cause is minimal, not unique. A session can hold several sufficient
  causes; the report shows one and flags overdetermination when the but-for
  table reveals it.
- 1-minimal means no single member can be dropped. A smaller cause that
  shares no subset relation with the reported one is not ruled out.
- Lineage is mapped to inputs by frame: the inputs delivered in the frame
  where main-timeline code started the chain. A program that reads several
  inputs per frame gets all of them named.
- Cost is one replay per history. The but-for pass is linear in the number
  of inputs; use `--no-but-for` for tapes with many thousands.
- `--max-replays` (default 100000) bounds the search; past it the command
  fails without a partial answer.

## Machine output

`--json` writes one object with `"format": "rad-blame"` and `"version": 1`:
the symptom, every input with its but-for class and whether it is in the
minimal cause and the lineage, the minimal script, and the search statistics
(strategy, replays, cache hits, workers, elapsed time, steps).

The dogfood incident for this command is
[`projects/dogfood/stockroom`](https://github.com/peteracs/rad/tree/main/projects/dogfood/stockroom).
