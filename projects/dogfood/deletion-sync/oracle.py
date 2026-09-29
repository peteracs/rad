"""Independent exact set-based oracle for deletion-resilient reset words."""

from collections import deque
import argparse
import json
from itertools import combinations


def image(states, letter, n, collapse=1):
    if letter == "a":
        return frozenset((q + 1) % n for q in states)
    return frozenset(0 if q >= n - collapse else q for q in states)


def solve(n, budget=1, collapse=1):
    if not 2 <= n <= 12 or not 0 <= budget <= 5 or not 1 <= collapse < n:
        raise ValueError("unsupported oracle parameters")
    full = frozenset(range(n))
    start = (full,) * (budget + 1)
    parents = {start: None}
    queue = deque([start])
    expanded = 0
    while queue:
        state = queue.popleft()
        if len(state[-1]) == 1:
            word = ""
            cursor = state
            while parents[cursor] is not None:
                cursor, letter = parents[cursor]
                word = letter + word
            return {"n": n, "budget": budget, "length": len(word), "word": word,
                    "target": next(iter(state[-1])), "discovered": len(parents),
                    "expanded": expanded}
        for letter in "ab":
            successor = tuple(
                image(states, letter, n, collapse) | (state[j - 1] if j else frozenset())
                for j, states in enumerate(state)
            )
            if successor not in parents:
                parents[successor] = (state, letter)
                queue.append(successor)
        expanded += 1
    return {"n": n, "budget": budget, "length": -1, "word": "", "target": -1,
            "discovered": len(parents), "expanded": expanded}


def direct_images(n, word, budget, collapse=1):
    """Enumerate deletion positions and individual trajectories (no subset recurrence)."""
    outcomes = set()
    for count in range(min(budget, len(word)) + 1):
        for erased in combinations(range(len(word)), count):
            erased = set(erased)
            for initial in range(n):
                q = initial
                for position, letter in enumerate(word):
                    if position in erased:
                        continue
                    if letter == "a":
                        q = (q + 1) % n
                    elif letter == "b":
                        q = 0 if q >= n - collapse else q
                    else:
                        raise ValueError("invalid alphabet")
                outcomes.add(q)
    return outcomes


def square_word(n):
    if n == 2:
        return "bb"
    return "b" + "a" * (n - 1) + ("bab" + "a" * (n - 2)) * (n - 2) + "ab"


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--max-n", type=int, default=9)
    parser.add_argument("--budget", type=int, default=1)
    parser.add_argument("--collapse", type=int, default=1)
    args = parser.parse_args()
    for size in range(2, args.max_n + 1):
        if args.collapse < size:
            print(json.dumps(solve(size, args.budget, args.collapse)), flush=True)
