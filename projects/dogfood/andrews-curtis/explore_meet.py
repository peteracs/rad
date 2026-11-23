#!/usr/bin/env python3
"""Let the searches of different presentations meet.

    python explore_meet.py --rad <rad binary> [--sources pending|resolved|all]
                           [--worlds 20000] [--max-total 40]
                           [--strategy descent|level] [--jobs 4]
                           [--cache <dir>] [--dry-run]

`ac_explore.rad reach` lists every pair of relators a search reaches from a
presentation within a budget of worlds. Two presentations whose lists share a
pair are joined by a chain of AC steps and automorphisms: one can be
trivialized exactly when the other can. The same holds when the two pairs
differ only by renaming generators (x -> x^-1, y -> y^-1, x <-> y), which are
automorphisms too; the searches do not try those, so they are compared here.

1. Reach lists are computed for every open presentation, for the start of
   every class in atlas_classes.tsv and, with `--sources resolved` (the
   default), for every presentation this project already trivialized or
   joined to a class. `--sources all` adds the 533 presentations the
   published greedy search solved. `--cache` keeps the lists on disk.
2. A shared pair makes an edge between two presentations. An open
   presentation connected to a trivialized one is trivialized; connected to
   a member of a class, it joins that class; a group of open presentations
   connected only to each other becomes a new class `ms<lowest index>`.
3. For each such presentation a chain to the target is assembled from
   `ac_explore.rad path` runs (the search is deterministic, so it finds the
   shared pair again): forward along its own path, across the renaming if
   there is one, backward along the other's with every step undone, and so
   on to the target.
4. verify.py checks the chain. Only then is it written to paths/ and a line
   appended to atlas_results.tsv (and atlas_classes.tsv for a new class).

`--dry-run` stops after step 2 and prints the groups.
"""
import argparse
import collections
import concurrent.futures
import hashlib
import os
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
UNDO = {"": "", "aut0": "aut1", "aut1": "aut0", "aut2": "aut3", "aut3": "aut2",
        "flipx": "flipx", "flipy": "flipy", "swap": "swap"}
RENAMES = {
    "flipx": {"x": "X", "X": "x"},
    "flipy": {"y": "Y", "Y": "y"},
    "swap": {"x": "y", "X": "Y", "y": "x", "Y": "X"},
}
# The eight ways to rename generators, each as the renamings to apply in turn.
WORDS = [(), ("flipx",), ("flipy",), ("flipx", "flipy"),
         ("swap",), ("swap", "flipx"), ("swap", "flipy"), ("swap", "flipx", "flipy")]


def _letters(word):
    """One renaming of the four letters for a sequence of renamings."""
    letters = {c: c for c in "xXyY"}
    for name in word:
        letters = {c: RENAMES[name].get(image, image) for c, image in letters.items()}
    return str.maketrans(letters)


TABLES = [_letters(word) for word in WORDS]


def canon(word):
    """Least rotation of a cyclic word or of its inverse, as ac_explore.rad writes it."""
    size = len(word)
    if size <= 1:
        return word.upper()
    twice = word + word
    best = min(twice[at:at + size] for at in range(size))
    twice = twice[::-1].swapcase()
    other = min(twice[at:at + size] for at in range(size))
    return best if best <= other else other


def renamed(pair, name):
    table = RENAMES[name]
    return ",".join(sorted(canon("".join(table.get(c, c) for c in word)) for word in pair.split(",")))


def least_renaming(pair):
    """The least of the eight ways to write `pair` with its generators renamed."""
    a, b = pair.split(",")
    best = None
    for table in TABLES:
        p, q = canon(a.translate(table)), canon(b.translate(table))
        image = f"{p},{q}" if p <= q else f"{q},{p}"
        if best is None or image < best:
            best = image
    return best


def rename_steps(pair, other):
    """The fewest renaming steps from `pair` to `other`, two pairs with one least renaming."""
    for word in sorted(WORDS, key=len):
        at, steps = pair, []
        for name in word:
            at = renamed(at, name)
            steps.append((name, at))
        if at == other:
            return steps
    raise ValueError(f"{pair} and {other} are not renamings of each other")


class Source:
    """One presentation a search starts from."""

    def __init__(self, name, r1, r2, home="", index=""):
        self.name = name      # table index, or a class label
        self.r1, self.r2 = r1, r2
        self.home = home      # "T", a class label, or "" for an open presentation
        self.index = index    # table index, "" for a class start that is not a table row
        self.start = ""       # the start pair as the search prints it


def load(sources):
    """The presentations to search from: known ones first, then the open ones."""
    table, greedy = {}, {}
    for line in (HERE / "data" / "miller_schupp.tsv").read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            index, r1, r2, solved = line.split("\t")
            table[index] = (r1, r2)
            greedy[index] = solved == "1"
    status = {}
    for line in (HERE / "results.tsv").read_text(encoding="utf-8").splitlines():
        cells = line.split("\t")
        if len(cells) >= 5 and cells[4] == "VERIFIED":
            status[cells[0]] = "T"
    for line in (HERE / "atlas_results.tsv").read_text(encoding="utf-8").splitlines():
        cells = line.split("\t")
        if len(cells) >= 5 and cells[3] in ("TRIVIAL", "JOINED", "CORE") and status.get(cells[0]) != "T":
            status[cells[0]] = "T" if cells[3] == "TRIVIAL" else cells[4]
    classes = collections.OrderedDict()
    for line in (HERE / "atlas_classes.tsv").read_text(encoding="utf-8").splitlines():
        if line.strip() and not line.startswith("#"):
            label, r1, r2 = line.split("\t")[:3]
            classes[label] = (r1, r2)

    out = [Source("T", "x", "y", home="T")]
    out += [Source(label, r1, r2, home=label) for label, (r1, r2) in classes.items()]
    if sources in ("resolved", "all"):
        out += [Source(index, *table[index], home=home, index=index) for index, home in status.items()]
    if sources == "all":
        out += [Source(index, *table[index], home="T", index=index) for index in table if greedy[index]]
    out += [Source(index, *table[index], index=index)
            for index in table if not greedy[index] and index not in status]
    return out, classes


class Searches:
    def __init__(self, rad, worlds, max_total, strategy, jobs, cache):
        self.rad = rad
        self.tail = [str(worlds), str(max_total), strategy]
        self.cache = pathlib.Path(cache) if cache else None
        if self.cache:
            self.cache.mkdir(parents=True, exist_ok=True)
        # `explore` gives the same answer at any thread count, so the cores
        # can be shared out between the searches that run side by side.
        self.env = dict(os.environ, RAYON_NUM_THREADS=str(max(1, (os.cpu_count() or 1) // jobs)))

    def run(self, mode, source, *extra):
        done = subprocess.run(
            [self.rad, str(HERE / "ac_explore.rad"), "--", mode, source.r1, source.r2, *extra, *self.tail],
            capture_output=True, text=True, timeout=1800, env=self.env,
        )
        lines = [line.strip() for line in done.stdout.splitlines() if line.strip()]
        at = next((i for i, line in enumerate(lines) if line.startswith(("REACHED", "SOLVED", "JOINED", "UNSOLVED"))), None)
        if at is None:
            raise RuntimeError(f"{mode} {source.name}: no result line\n{done.stdout[-400:]}{done.stderr[-400:]}")
        return lines[at], lines[at + 1:]

    def reach(self, source):
        """The start pair as the search writes it, and every pair it reaches."""
        kept = None
        if self.cache:
            name = hashlib.sha1("\t".join([source.r1, source.r2, *self.tail]).encode()).hexdigest()
            kept = self.cache / f"{name}.txt"
            if kept.exists():
                lines = kept.read_text(encoding="utf-8").splitlines()
                return lines[0], lines[1:]
        head, pairs = self.run("reach", source)
        start = head.split()[1]
        if kept:
            kept.write_text("\n".join([start] + pairs) + "\n", encoding="utf-8", newline="\n")
        return start, pairs

    def reach_filed(self, source):
        """`reach`, with every pair filed under its least renaming. Runs in a
        worker process, so the renaming is done on all cores."""
        start, pairs = self.reach(source)
        return start, pairs, [least_renaming(pair) for pair in pairs]

    def path(self, source, pair=None):
        """The steps from the source to `pair` (or to <x, y>) as (via, pair), or None."""
        head, lines = self.run("path", source, pair) if pair else self.run("solve", source)
        if not head.startswith(("SOLVED", "JOINED")) or "followed=true" not in head.split():
            return None
        return [(line.rpartition(" ")[0], line.rpartition(" ")[2]) for line in lines]


def backward(steps):
    """The same steps from the last pair to the first, each one undone."""
    return [("", steps[-1][1])] + [(UNDO[steps[i][0]], steps[i - 1][1]) for i in range(len(steps) - 1, 0, -1)]


def stored_chain(source):
    """The verified chain this project already has from a resolved presentation to its target."""
    name = f"ms_{source.index}.txt" if source.home == "T" else f"ms_{source.index}_to_{source.home}.txt"
    path = HERE / "paths" / name
    if not path.exists():
        return None
    lines = [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    return [(line.rpartition(" ")[0], line.rpartition(" ")[2]) for line in lines[1:]]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rad", required=True)
    parser.add_argument("--sources", choices=["pending", "resolved", "all"], default="resolved")
    parser.add_argument("--worlds", type=int, default=20000)
    parser.add_argument("--max-total", type=int, default=40)
    parser.add_argument("--strategy", choices=["descent", "level"], default="descent")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--cache", default="")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    searches = Searches(str(pathlib.Path(args.rad).resolve()), args.worlds, args.max_total, args.strategy,
                        args.jobs, args.cache)
    sources, classes = load(args.sources)
    by_name = {source.name: source for source in sources}
    print(f"{len(sources)} searches, {sum(1 for s in sources if not s.home)} of them from open presentations", flush=True)

    # 1. Reach lists, and 2. the edges. A pair is filed under the least of its
    # eight renamings and owned by the first search that lists one of them;
    # every later search that lists one meets the owner.
    owner = {}
    group = {source.name: source.name for source in sources}

    def find(name):
        while group[name] != name:
            group[name] = group[group[name]]
            name = group[name]
        return name

    edges = collections.defaultdict(list)   # name -> [other name]
    shared = {}                             # (name, other) -> (pair of name, pair of other)
    with concurrent.futures.ProcessPoolExecutor(max_workers=args.jobs) as pool:
        for source, (start, pairs, filed) in zip(sources, pool.map(searches.reach_filed, sources)):
            source.start = start
            best = {}
            for position, (pair, least) in enumerate(zip(pairs, filed)):
                hit = owner.get(least)
                if hit is None:
                    owner[least] = (source.name, position, pair)
                elif hit[0] != source.name:
                    cost = position + hit[1]
                    if hit[0] not in best or cost < best[hit[0]][0]:
                        best[hit[0]] = (cost, pair, hit[2])
            for other, (_, mine, theirs) in sorted(best.items(), key=lambda item: item[1][0]):
                if find(other) != find(source.name):
                    group[find(other)] = find(source.name)
                    edges[source.name].append(other)
                    edges[other].append(source.name)
                    shared[(source.name, other)] = (mine, theirs)
                    shared[(other, source.name)] = (theirs, mine)
    del owner

    members = collections.defaultdict(list)
    for source in sources:
        members[find(source.name)].append(source)
    plans = []   # (open source, target label, [names from it to the target's source])
    for group_members in members.values():
        opened = [source for source in group_members if not source.home]
        if not opened or len(group_members) == 1:
            continue
        homes = sorted({source.home for source in group_members if source.home},
                       key=lambda home: (home != "T", list(classes).index(home) if home in classes else -1))
        if len(homes) > 1:
            print(f"CLASSES MEET in one group: {' '.join(homes)}", flush=True)
        if homes:
            label = homes[0]
            targets = [source.name for source in group_members if source.home == label]
        else:
            core = min(opened, key=lambda source: int(source.index))
            label, targets = f"ms{core.index}", [core.name]
        # Breadth-first from the targets: the fewest shared pairs to cross.
        toward = {name: None for name in targets}
        queue = collections.deque(targets)
        while queue:
            name = queue.popleft()
            for other in edges[name]:
                if other not in toward:
                    toward[other] = name
                    queue.append(other)
        print(f"group {label}: {len(opened)} open, {len(group_members) - len(opened)} known "
              f"({' '.join(source.name for source in opened)})", flush=True)
        for source in opened:
            route = [source.name]
            while toward.get(route[-1]) is not None:
                route.append(toward[route[-1]])
            plans.append((source, label, route))
    print(f"{len(plans)} open presentations are in a group", flush=True)
    if args.dry_run or not plans:
        return

    # 3. The paths every chain needs, each computed once.
    wanted = set()
    for source, label, route in plans:
        for here, there in zip(route, route[1:]):
            mine, theirs = shared[(here, there)]
            wanted.add((here, mine))
            wanted.add((there, theirs))
        last = by_name[route[-1]]
        if last.home == "T" and last.index and stored_chain(last) is None:
            wanted.add((last.name, None))
    wanted = sorted(wanted, key=lambda item: (item[0], item[1] or ""))
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        found = dict(zip(wanted, pool.map(lambda item: searches.path(by_name[item[0]], item[1]), wanted)))

    # 4. Assemble, verify, record.
    counts = collections.Counter()
    # The start of a new class is recorded once a member's chain is verified.
    new_classes = {label: source for source, label, route in plans if len(route) == 1}
    with open(HERE / "atlas_results.tsv", "a", encoding="utf-8", newline="\n") as results:
        for source, label, route in plans:
            if len(route) == 1:
                continue
            chain = [("", source.start)]
            complete = True
            for here, there in zip(route, route[1:]):
                mine, theirs = shared[(here, there)]
                out, back = found[(here, mine)], found[(there, theirs)]
                if out is None or back is None:
                    complete = False
                    break
                chain += out[1:] + rename_steps(mine, theirs) + backward(back)[1:]
            last = by_name[route[-1]]
            # A resolved presentation is not the target itself, unless it is
            # the table row a class starts at: its own chain leads the rest
            # of the way.
            if complete and last.index and last.home and last.home != f"ms{last.index}":
                rest = stored_chain(last) or (found.get((last.name, None)) if last.home == "T" else None)
                if rest is None:
                    complete = False
                else:
                    chain += rest[1:]
            if not complete:
                counts["NO PATH"] += 1
                print(f"NO PATH  ms_{source.index} toward {label}", flush=True)
                continue
            head = (f"JOINED meet {source.start} meets class {label} "
                    f"route={'>'.join(route)} worlds={args.worlds} strategy={args.strategy}")
            text = "\n".join([head] + [f"{via} {pair}".strip() for via, pair in chain]) + "\n"
            command = [sys.executable, str(HERE / "verify.py"), source.r1, source.r2]
            if label != "T":
                target = classes.get(label) or (by_name[label[2:]].r1, by_name[label[2:]].r2)
                command += ["--to", target[0], target[1]]
            check = subprocess.run(command, input=text, capture_output=True, text=True)
            if check.returncode != 0:
                counts["REJECTED"] += 1
                print(f"REJECTED ms_{source.index} toward {label}: {check.stderr.strip() or check.stdout.strip()}", flush=True)
                continue
            if label in new_classes:
                core = new_classes.pop(label)
                with open(HERE / "atlas_classes.tsv", "a", encoding="utf-8", newline="\n") as out:
                    out.write(f"{label}\t{core.r1}\t{core.r2}\n")
                results.write("\t".join([core.index, core.r1, core.r2, "CORE", label, "0", "", ""]) + "\n")
                classes[label] = (core.r1, core.r2)
            (HERE / "paths").mkdir(exist_ok=True)
            name = f"ms_{source.index}.txt" if label == "T" else f"ms_{source.index}_to_{label}.txt"
            (HERE / "paths" / name).write_text(text, encoding="utf-8", newline="\n")
            status = "TRIVIAL" if label == "T" else "JOINED"
            results.write("\t".join([source.index, source.r1, source.r2, status, label, str(len(chain) - 1), "", ""]) + "\n")
            results.flush()
            counts[f"{status}:{label}"] += 1
            print(f"{status}  ms_{source.index}  <{source.r1}, {source.r2}>  class={label} steps={len(chain) - 1}", flush=True)
    print(" ".join(f"{name}={count}" for name, count in sorted(counts.items())) or "nothing new")


if __name__ == "__main__":
    main()
