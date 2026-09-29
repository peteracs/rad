# Uniform Collatz correction bounds

For the shortcut map, write `P_j = 3^(odd steps)/2^j` and
`T^j(n) = n * P_j * Q_j`. This workload certifies the explicit bound

```text
Q_j < 2048 whenever the states from time 0 through time j are distinct.
```

The constant holds at every depth and every positive starting value.
If all states in the segment are at least `2^71`, the bound improves to 4.
The [written proof](PROOF.md) uses exact image collisions and a certified
infinite geometric tail. It also explains the remaining Collatz proof target
and the connection to the weak residue conjecture. Historical priority for
this finite formulation is unresolved.

Run from the repository root:

```powershell
py -O projects/dogfood/collatz-barrier/correction/accept.py
```

The [receipt](evidence.json) records the exact campaign:

- 524286 residue inputs through depth 18, including literal translation checks;
- 3125 affine coalescence certificates across these depths; at depth 18 they
  remove 1391 of the 7495 classes with no coefficient contraction;
- seven arbitrary-integer certificates, each with an exact binomial sum
  through depth 256 and a proved tail covering every greater depth;
- independent Python integer and `Fraction` verification;
- 2053 literal starting values, including inputs up to 1024 bits;
- eleven tests targeting missing hypotheses and corrupted certificates;
- isolated worlds, write-footprint checks, serialized snapshots, and matching
  mathematical output and replay across one and four workers.

The large collision lists use `let unique mut`. RAD checks exclusive ownership
and lowers their appends to in-place operations. Sorting must reassign the
same unique binding before iteration. An initial ordinary-list implementation
repeatedly copied growing lists; the correction-only program ran in about
4.5 seconds locally after that change. This was an ownership choice in the workload, not a
discovered VM correctness defect.

The coalescence extension records a smaller positive representative for every
eliminated cylinder. This supports induction even when the trajectory itself
has not descended. The proof credits the known consecutive-integer families
and leaves universal coverage as an explicit remaining target.

The exact arithmetic libraries are ordinary RAD modules. This campaign uses
no Collatz-specific builtin or native worker. Ignored `out/` contains the raw
JSONL, stderr, and replay traces; the receipt hashes actual file bytes and
all imported source dependencies. Acceptance checks remain active under `-O`.
