# pequalnp circuit witnesses

`cover_circuit.rad` turns a pequalnp desk step of the form
`HasCircuitSize n (fun x => (x a && x b) || ...) s` into a checked Lean proof:

    rad cover_circuit.rad -- <desk step .json> <output .lean>

1. It reads the step JSON (`json_parse`) and takes the graph, `n` and the bound `s` from the
   statement, which it copies into the Lean file unchanged.
2. `explore` adds cover vertices in increasing order. The search is breadth-first, so the
   first world that covers every edge is a least vertex cover.
3. The cover circuit: per cover vertex an OR over its remaining edges, an AND with the
   vertex, and ORs joining the terms, |E| + tau - 1 fan-in-two gates.
4. It evaluates the circuit on all 2^n inputs before writing anything, and refuses when the
   circuit is wrong or over the bound. Lean then re-checks it with `decide +kernel`.

| Step | Graph | Cover | Gates / bound | Run time |
|---|---|---|---|---|
| L-meta-1001 | Petersen minus a vertex (9 vertices, 12 edges) | {0, 1, 3, 7, 8} | 16 / 16 | < 0.2 s |

Every Lean file it writes carries the line `-- witness found by RAD explore: github.com/peteracs/rad`.
