# explore: every future, not a sample

`simulate` runs one future of a forked world and `simulate_par` runs a sample
of them. `explore` runs all of them: it searches every world reachable from a
fork, in parallel, and returns either a path to a goal or the statement that
there is none.

```rad
let found = explore(fork(), [system::Pour], {"goal": "Jugs.done"})
found["status"]   // "goal", "exhausted", "limit" or "depth"
found["path"]     // the labels of the moves, start to goal
```

A move is a system. Inside it, `offer(label)` records the world as it is now
as one successor and rewinds to where the move started. A system that never
offers is one move named after itself, so a plain list of systems means "run
these in every order". `follow(fork, moves, path)` replays a path with none of
the search's state, which makes a path a certificate.

A search without a goal reports what it found through `"collect"`: one field
of every world reached, in the order they were reached. Two searches from
different starts have met when their lists share a value.
`projects/dogfood/andrews-curtis/explore_meet.py` uses exactly that.

The reference is in `docs/src/reference/builtins_parts/speculation.md`.

## The programs

| File | Question | Answer it prints |
|---|---|---|
| `jugs.rad` | Measure 4 with jugs of 3 and 5. Measure 3 with jugs of 2 and 4. | A shortest route of 6 pours. `exhausted` after 6 worlds: it cannot be done, and `"collect"` lists what the big jug held in each of them: 0, 2 or 4. |
| `eight_puzzle.rad` | How many positions does the 8-puzzle have, and how far is the farthest? | 181,440 positions, 31 moves, two positions at that distance, and a 31-move solution of one of them. |
| `vault.rad` | Solve a crate-pushing level; is a position with a cornered crate still winnable? | A 17-step solution, replayed by `follow` and then played on the live world. `exhausted` after 600 positions from the cornered one: lost. |
| `lost_update.rad` | Can two systems that each read and then write a balance lose an update? | The schedule `Alice Bob Alice Bob`, balance 120 where 130 was due. With the step made atomic: `exhausted` after 4 worlds. |

`vault.rad` is the one to read first. The function the live game calls to
take a step, `try_step`, is the function the move system calls, so the rules
that are searched are the rules that are played.

## Checking them

`oracle.py` is a breadth-first search in Python that shares no code with Rad.
It models each program's rules again from scratch and must agree with every
line the program prints: the status, the path letter for letter, the number of
worlds seen and the count at each depth.

```sh
python oracle.py                 # print what the programs should print
python oracle.py --rad <rad>     # run them and compare, line by line
```

Agreement on the path, not only its length, holds because both searches are
breadth-first, try moves in the same order and keep the first route to a
position. `explore` expands worlds in parallel and still returns that path at
any thread count: worlds are merged in the order a sequential search would
have produced them.

## What this does not show

- These state spaces are small (at most 181,440 worlds). The 8-puzzle run
  takes about six seconds here; it is a correctness check, not a benchmark.
- A world waiting to be expanded holds a snapshot. Memory follows the
  frontier, and `"limit"` is the only bound on it.
- `"exhausted"` is a statement about the moves as written. A move system that
  leaves out a legal move proves less than it appears to.
