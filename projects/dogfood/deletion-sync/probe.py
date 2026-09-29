"""Small derivation aid; the independent verifier is oracle.py."""
from oracle import image
import heapq

for n in (4, 5, 6, 7):
    pair = (frozenset(range(n)),) * 2
    def consume(word):
        global pair
        for letter in word:
            pair = (image(pair[0], letter, n), image(pair[1], letter, n) | pair[0])
        return tuple(sorted(s) for s in pair)
    print(n, "prefix", consume("b" + "a" * (n - 1)))
    for j in range(1, n - 1):
        print("block", j, consume("bab" + "a" * (n - 2)))
    print("suffix", consume("ab"))

for n in range(3, 8):
    for costs in ((100000, 1), (1, 100000)):
        full = frozenset(range(n))
        start = (full, full)
        distances = {start: 0}
        queue = [(0, 0, start, "")]
        counter = 0
        while queue:
            distance, _, state, word = heapq.heappop(queue)
            if distance != distances[state]:
                continue
            if len(state[1]) == 1:
                print("weighted", n, costs, "a", word.count("a"), "b", word.count("b"), word)
                break
            for letter, cost in zip("ab", costs):
                nxt = (image(state[0], letter, n), image(state[1], letter, n) | state[0])
                if distance + cost < distances.get(nxt, 10**20):
                    distances[nxt] = distance + cost
                    counter += 1
                    heapq.heappush(queue, (distance + cost, counter, nxt, word + letter))
