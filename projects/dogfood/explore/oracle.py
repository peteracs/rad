#!/usr/bin/env python3
"""Independent answers for the explore dogfood. Python 3 standard library only.

    python oracle.py                 print what every program must print
    python oracle.py --rad <rad>     run the programs and compare, line by line

Each puzzle is searched here by a plain breadth-first search that shares no
code with Rad. It tries moves in the order the Rad program offers them and
stops at the first goal it generates, which is what `explore` documents, so
the number of positions seen and the path itself must match, not only the
length of the answer.
"""
import argparse
import pathlib
import subprocess
import sys
from collections import deque

HERE = pathlib.Path(__file__).resolve().parent


def search(start, moves, is_goal=None):
    """Breadth-first. Returns status, positions seen, path, depth, levels."""
    parent = {start: None}
    depth = {start: 0}
    if is_goal and is_goal(start):
        return "goal", 1, [], 0, [1]
    queue = deque([start])
    while queue:
        state = queue.popleft()
        for label, nxt in moves(state):
            if nxt in parent:
                continue
            parent[nxt] = (state, label)
            depth[nxt] = depth[state] + 1
            if is_goal and is_goal(nxt):
                path = []
                at = nxt
                while parent[at]:
                    at, label = parent[at]
                    path.append(label)
                return "goal", len(parent), path[::-1], depth[nxt], None
            queue.append(nxt)
    levels = [0] * (max(depth.values()) + 1)
    for d in depth.values():
        levels[d] += 1
    return "exhausted", len(parent), [], len(levels) - 1, levels


def reached(start, moves):
    """Every position, in the order a breadth-first search first reaches it."""
    order = [start]
    known = {start}
    for state in order:
        for _, nxt in moves(state):
            if nxt not in known:
                known.add(nxt)
                order.append(nxt)
    return order


def rad_list(items):
    return "[" + ", ".join(str(item) for item in items) + "]"


# --- jugs.rad -------------------------------------------------------------

def jug_moves(cap_a, cap_b):
    def moves(state):
        a, b = state
        ab = min(a, cap_b - b)
        ba = min(b, cap_a - a)
        yield "fill a", (cap_a, b)
        yield "fill b", (a, cap_b)
        yield "empty a", (0, b)
        yield "empty b", (a, 0)
        yield "a into b", (a - ab, b + ab)
        yield "b into a", (a + ba, b - ba)
    return moves


def jugs():
    out = []
    status, seen, path, depth, _ = search((0, 0), jug_moves(3, 5), lambda s: 4 in s)
    out.append(f"measure 4 with 3 and 5   {status} moves={len(path)} seen={seen} path={' > '.join(path)}")
    status, seen, _, depth, levels = search((0, 0), jug_moves(3, 5))
    out.append(f"every state of 3 and 5   {status} seen={seen} depth={depth} levels={rad_list(levels)}")
    status, seen, _, depth, levels = search((0, 0), jug_moves(2, 4), lambda s: 3 in s)
    out.append(f"measure 3 with 2 and 4   {status} seen={seen} depth={depth} levels={rad_list(levels)}")
    out.append(f"the big jug of 2 and 4   {rad_list(b for _, b in reached((0, 0), jug_moves(2, 4)))}")
    return out


# --- eight_puzzle.rad -----------------------------------------------------

def slide_moves(rows, cols):
    def moves(cells):
        blank = cells.index("0")
        row, col = divmod(blank, cols)
        for label, ok, to in (
            ("U", row > 0, blank - cols),
            ("D", row < rows - 1, blank + cols),
            ("L", col > 0, blank - 1),
            ("R", col < cols - 1, blank + 1),
        ):
            if ok:
                c = list(cells)
                c[blank], c[to] = c[to], "0"
                yield label, "".join(c)
    return moves


def eight_puzzle():
    status, seen, _, depth, levels = search("123456780", slide_moves(3, 3))
    out = [f"positions {status} seen={seen} depth={depth}", f"levels    {rad_list(levels)}"]
    # The two positions farthest from solved, and a shortest way back from one.
    hardest = "867254301"
    status, seen, path, depth, _ = search(hardest, slide_moves(3, 3), lambda s: s == "123456780")
    out.append(f"hardest   {status} moves={len(path)} path={''.join(path)}")
    return out


# --- vault.rad ------------------------------------------------------------

ROWS = [
    "#########",
    "#   #   #",
    "# $   $ #",
    "#  .#.  #",
    "#   @   #",
    "#########",
]
WIDTH = 9
TEXT = "".join(ROWS)
WALLS = {i for i, ch in enumerate(TEXT) if ch == "#"}
GOALS = {i for i, ch in enumerate(TEXT) if ch == "."}
DELTAS = (("U", -WIDTH), ("D", WIDTH), ("L", -1), ("R", 1))


def vault_step(state, delta):
    """state = (player, crates as a tuple indexed by crate id), or None."""
    player, crates = state
    to = player + delta
    if to in WALLS:
        return None
    crates = list(crates)
    if to in crates:
        beyond = to + delta
        if beyond in WALLS or beyond in crates:
            return None
        crates[crates.index(to)] = beyond
    return (to, tuple(crates))


def position(state):
    return (state[0], tuple(sorted(state[1])))


def vault_moves(named):
    """named(state) is the identity: the whole state, or the position."""
    def moves(state):
        for label, delta in DELTAS:
            nxt = vault_step(state, delta)
            if nxt is not None:
                yield label, nxt
    return moves


class Keyed:
    """Search over full states while two states with one key count as one."""

    def __init__(self, state, key):
        self.state, self.key = state, key(state)

    def __hash__(self):
        return hash(self.key)

    def __eq__(self, other):
        return self.key == other.key


def vault_search(start, key, goal):
    moves = vault_moves(key)
    wrap = lambda s: Keyed(s, key)
    return search(
        wrap(start),
        lambda k: ((label, wrap(nxt)) for label, nxt in moves(k.state)),
        (lambda k: goal(k.state)) if goal else None,
    )


def vault():
    start = (TEXT.index("@"), tuple(i for i, ch in enumerate(TEXT) if ch == "$"))
    won = lambda s: all(c in GOALS for c in s[1])
    whole = lambda s: s
    out = []
    status, seen, path, depth, _ = vault_search(start, position, won)
    out.append(f"solve     {status} moves={len(path)} seen={seen} path={''.join(path)}")
    out.append("follow    same_world=true")
    played = start
    for label in path:
        played = vault_step(played, dict(DELTAS)[label])
    out.append(f"play      won={'true' if won(played) else 'false'}")
    cornered = start
    for label in "LLUURUL":
        cornered = vault_step(cornered, dict(DELTAS)[label])
    status, seen, _, depth, _ = vault_search(cornered, position, won)
    out.append(f"cornered  {status} seen={seen} depth={depth}")
    status, seen, _, depth, _ = vault_search(start, position, None)
    out.append(f"positions {status} seen={seen} depth={depth}")
    status, seen, _, depth, _ = vault_search(start, whole, None)
    out.append(f"worlds    {status} seen={seen} depth={depth}")
    return out


# --- lost_update.rad ------------------------------------------------------

def lost_update():
    def racy(state):
        balance, jobs = state
        for index, name in enumerate(("Alice", "Bob")):
            amount, pc, read = jobs[index]
            new_balance, job = balance, (amount, pc, read)
            if pc == 0:
                job = (amount, 1, balance)
            elif pc == 1:
                new_balance, job = read + amount, (amount, 2, read)
            new_jobs = list(jobs)
            new_jobs[index] = job
            yield name, (new_balance, tuple(new_jobs))

    def atomic(state):
        balance, jobs = state
        for index, name in enumerate(("AliceAtomic", "BobAtomic")):
            amount, pc, read = jobs[index]
            new_balance, job = balance, (amount, pc, read)
            if pc == 0:
                new_balance, job = balance + amount, (amount, 2, read)
            new_jobs = list(jobs)
            new_jobs[index] = job
            yield name, (new_balance, tuple(new_jobs))

    start = (100, ((10, 0, 0), (20, 0, 0)))
    broken = lambda s: all(job[1] == 2 for job in s[1]) and s[0] != 130
    out = []
    status, seen, path, _, _ = search(start, racy, broken)
    out.append(f"racy    {status} seen={seen} schedule={' '.join(path)}")
    final = start
    for name in path:
        final = dict(racy(final))[name]
    out.append(f"        balance={final[0]}, should be 130")
    status, seen, _, _, _ = search(start, atomic, broken)
    out.append(f"atomic  {status} seen={seen}")
    return out


PROGRAMS = {
    "jugs.rad": jugs,
    "eight_puzzle.rad": eight_puzzle,
    "vault.rad": vault,
    "lost_update.rad": lost_update,
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad")
    args = parser.parse_args()
    failures = 0
    for name, answer in PROGRAMS.items():
        want = answer()
        if not args.rad:
            print(f"# {name}")
            print("\n".join(want))
            continue
        done = subprocess.run([args.rad, str(HERE / name)], capture_output=True, text=True, timeout=600)
        got = [line.rstrip() for line in done.stdout.splitlines() if line.strip()]
        if got == want:
            print(f"ok    {name}: {len(want)} lines agree")
            continue
        failures += 1
        print(f"FAIL  {name}")
        for index in range(max(len(got), len(want))):
            g = got[index] if index < len(got) else "<missing>"
            w = want[index] if index < len(want) else "<missing>"
            print(f"  {'  ' if g == w else '!='} rad:    {g}")
            if g != w:
                print(f"     oracle: {w}")
        if done.stderr.strip():
            print(done.stderr.strip())
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
