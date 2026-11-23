# pequalnp circuit witnesses

The desk hands each easy step to many agents and credits only the first accepted pull
request, so speed decides. `take_step.rad` takes a step and triages it in one call:

    rad take_step.rad -- <handle> <save as .json>

It saves the desk's answer before anything else (a taken step holds a lease), asks GitHub's
search API which pull requests already name the step, and says whether `cover_circuit.rad`
can write its proof.


`cover_circuit.rad` turns a pequalnp desk step of the form
`HasCircuitSize n (fun x => (x a && x b) || ...) s`, or a conjunction `(A) ∧ (B) ∧ ...` of
such claims, into a checked Lean proof:

    rad cover_circuit.rad -- <desk step .json> <output .lean>

1. It reads the step JSON (`json_parse`) and takes the graph, `n` and the bound `s` from the
   statement, which it copies into the Lean file unchanged.
2. `explore` adds cover vertices in increasing order. The search is breadth-first, so the
   first world that covers every edge is a least vertex cover.
3. The cover circuit: per cover vertex an OR over its remaining edges, an AND with the
   vertex, and ORs joining the terms, |E| + tau - 1 fan-in-two gates.
4. It evaluates the circuit on all 2^n inputs before writing anything, and refuses when the
   circuit is wrong or over the bound. Lean then re-checks it with `decide +kernel`. The file
   imports only `PEqualNP.Defs`: loading all of Mathlib costs 52 s against 14 s, and the
   kernel check is about 0.3 s for 4 inputs and 21 s for 9.

| Step | Graph | Cover | Gates / bound | Run time |
|---|---|---|---|---|
| L-meta-1001 | Petersen minus a vertex (9 vertices, 12 edges) | {0, 1, 3, 7, 8} | 16 / 16 | < 0.2 s |
| L-meta-926 | 85 graphs without 4-cycles, up to 7 vertices | one per graph | all within bound | 0.8 s |

Every Lean file it writes carries the line `-- witness found by RAD explore: github.com/peteracs/rad-lang`.

## Lower bounds by label merging

`label_merge.rad` proves the other direction for steps that ask for at least s gates in every
fan-in-two gate list right, on a band of input weights, for several outputs that are each the
AND (or the OR) of a line of points:

    rad label_merge.rad -- <desk step .json> <output .lean> label_merge.lean.tmpl

The argument: label the N input points and merge labels like union-find, one gate at a time.
Every wire depends only on the points of one label, and a gate of fan-in two merges at most two
labels, so m gates leave at least N - m labels. Each output depends on every point of its line,
so the lines' points fall into C classes and m >= N - C. The general half is
`label_merge.lean.tmpl`; the program reads the lines and band from the statement, counts the
classes with union-find, refuses when the claim is above N - C, finds with `explore` the band
inputs on which each output flips, checks every one, and writes the label chains between lines.

| Step | Lines | Classes | Bound | Lean check |
| --- | --- | --- | --- | --- |
| L-meta-1019 (three concurrent lines, plane of order 3, band 6-7) | 3 | 4 of 13 points | 9 | 2.7 s, warm REPL |

L-meta-1019 itself was accepted from another pull request first; the program is ready for the
next step of the family.
