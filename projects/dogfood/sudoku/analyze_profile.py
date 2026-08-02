import gzip, json, sys, os
from collections import Counter

path = sys.argv[1]
with gzip.open(path, "rt", encoding="utf-8") as f:
    prof = json.load(f)

def walk(p, out):
    out.append(p)
    for sub in p.get("processes", []):
        walk(sub, out)

roots = [prof]
procs = []
walk(prof, procs)

best = None
for p in procs:
    for t in p.get("threads", []):
        n = len(t.get("samples", {}).get("stack", []))
        if best is None or n > best[1]:
            best = (t, n, p.get("meta", {}).get("product", "?"))

t, nsamples, product = best
print(f"thread: {t.get('name')} of {product}, {nsamples} samples")

strings = t["stringArray"]
frame_func = t["frameTable"]["func"]
func_name = t["funcTable"]["name"]
stack_frame = t["stackTable"]["frame"]
samples_stack = t["samples"]["stack"]

self_count = Counter()
for s in samples_stack:
    if s is None:
        continue
    leaf_frame = stack_frame[s]
    fname = strings[func_name[frame_func[leaf_frame]]]
    self_count[fname] += 1

total = sum(self_count.values())
print(f"total leaf samples: {total}")
for name, c in self_count.most_common(30):
    print(f"{c*100.0/total:5.1f}%  {name}")
