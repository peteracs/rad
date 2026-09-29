# Question-led Collatz investigation

Completed 2026-09-07. **No Collatz proof was found.** This investigation
produced exact obstructions to specified families of termination proofs,
counterexamples to proposed rankings, and reusable RAD generators and
certificate checkers. Priority of the mathematical observations has not
been established.

The strongest result is an actual shortcut-Collatz segment

\[
674 \xrightarrow{54\text{ steps}} 650.
\]

Its endpoints have identical counts of every encoded digit factor of
lengths one through four, including boundary factors, and are equal modulo
24. Its canonical mixed-base realization uses all eleven rewrite rules.
Consequently, **no function of those factor counts and the integer modulo
24 can weakly decrease on every rewrite while strictly decreasing on every
application of any one selected rule**. This includes nonlinear functions,
not just weighted sums. The [proof and complete orbit](PROOF.md) explain
the encoding, the argument, and its limits. The integers are different:
this is an obstruction to a proof representation, not a Collatz cycle.

## Questions, results, and decisions

These were new questions for this investigation; we do not claim that
nobody has asked them before. Each iteration changed the question in
response to a result, rather than merely increasing a search budget.

| Question | RAD experiment and result | Decision |
| --- | --- | --- |
| Can signed weights on individual symbols give a rank? | `generate.rad`: 16 constraints, 5 variables; exact contradiction with 5 rows. | Retain neighboring symbols. |
| Do counts of pairs or triples suffice, with unrestricted real weights? | 271/6,046 constraints and 46/246 variables; checked duals with 9/28 rows. Each of the five boundary rules was also tested separately as the strict rule; all ten problems were infeasible. | Increase memory and challenge incomplete context coverage. |
| Does a four-symbol candidate passing short contexts extend to arbitrary words? | A candidate passed 6,546 constraints. `refine.rad` checked all 147,421 rows and found 62,928 failures. Adding 176 counterexamples produced another candidate with 64,445 failures; another 176 were added. The resulting exact query timed out. | Seek a structural obstruction from a real orbit. |
| Can two states on one orbit erase precisely the information used by the rank? | `orbit_alias.rad`: 200 questions, 17,034 examined states, 114 matching endpoint pairs. The selected four-symbol witness was 674 to 650. | Compile its literal rewrites into a proof, then retain arithmetic information. |
| Does retaining a small integer residue separate that witness? | The same endpoints are equal modulo 24. An expanded 350-question sample examined 52,704 states and found five matches, all in the tested four-symbol/modulo-3 class. | Separate endpoint residues from a richer model that reads residues along the entire word. |
| Can weights depending on the value of the entire prefix modulo a fixed modulus rank the system? | `generate_modular.rad`: exact checked contradictions for moduli 2, 3, 5, 7, 15, and 31. Modulus 127 remains unresolved. | Preserve the obstructions and the unresolved case; stop exploration at the usage limit. |

The full four-symbol linear query and its floating dual search also timed
out. The later literal orbit settled the stated four-symbol family by a
different argument. A timeout itself was never counted as an obstruction.
The second orbit sample found no matches at memory five or six; that small
negative sample provides no feasibility or termination theorem.

For the prefix-residue model, the counts were:

| Modulus | Constraints | Variables | Exact certificate rows | Status |
| ---: | ---: | ---: | ---: | --- |
| 2 | 29 | 14 | 8 | Infeasible, checked |
| 3 | 42 | 21 | 8 | Infeasible, checked |
| 5 | 68 | 35 | 15 | Infeasible, checked |
| 7 | 94 | 49 | 10 | Infeasible, checked |
| 15 | 198 | 105 | 46 | Infeasible, checked |
| 31 | 406 | 217 | 79 | Infeasible, checked |
| 127 | 1,654 | 889 | None | Unresolved |

For modulus 127, exact solving timed out. A floating-point LP subsequently
proposed a dual support, but exact rational reconstruction failed to
produce a certificate within its limit. Its receipt explicitly records
`exact_obstruction: false`. This is not an UNSAT result.

## What RAD contributed

[generate.rad](generate.rad) derives exact obligations in speculative
worlds, split by rewrite rule. `simulate_many` executes the lanes;
write-footprint checks, live-world digests, and byte serialization check
isolation. The final acceptance repeats generation with one and four
workers and replays each recording with the other worker count.

[refine.rad](refine.rad) checks candidates against complete local contexts
and supplies concrete counterexamples. [orbit_alias.rad](orbit_alias.rad)
asks deterministic trajectory questions in parallel worlds.
[check_dual.rad](check_dual.rad), [check_alias.rad](check_alias.rad), and
[check_modular.rad](check_modular.rad) reconstruct the mathematics from
sparse certificate descriptors instead of trusting solver labels or saved
coefficient maps. Python supplies separate arbitrary-precision oracles and
Z3 exact linear solving. Optional SciPy searches only propose witnesses.

This turn added research tooling, including a reusable prefix-residue rank
model and a compiler from actual orbit segments to exact rewrite
obstructions. It fixed the standalone SMT export so tracked constraints
are actually asserted, and avoided constructing zero terms in sparse dual
reconstruction. These are investigation-tool changes; this turn did not
modify RAD's core or native kernel.

The earlier [matrix-repair campaign](../REPAIR.md) remains separate. Its
recorded benchmark was about 164.8 times faster end to end on the specified
20,000-trial workload, and its campaigns totalled 89.6 million mutation
trials. Those are previous workload measurements, not new measurements or
counts of distinct proof candidates. No matrix termination proof resulted.

## Why the mathematical bridge remains

An important distinction is between statements about almost all inputs
and statements about every input. [Tao's result](https://arxiv.org/abs/1909.03562)
controls the minimum orbit value for almost all starting values in
logarithmic density; an exceptional orbit is outside that guarantee. A
finite search or a typical-parity heuristic does not remove that
quantifier gap. Our earlier correction-product bound likewise leaves the
actual parity sequence's multiplicative drift uncontrolled.

There is also a concrete representation problem. A proposed rank must
retain enough information to order successive states. Our witness proves
that a particular summary loses information needed for such an order.
The mixed-base termination formulation and earlier limitations of other
rewrite interpretations are established work of
[Yolcu, Aaronson, and Heule](https://emreyolcu.com/research/rewriting-collatz.pdf).
Our computation does not supply a general explanation for why all previous
mathematical approaches have fallen short.

RAD helps turn a conjectured invariant into obligations, falsify it, and
retain an exact reason to change the representation. This investigation
eliminated entire templates without bounding their coefficients. It did
not establish a universal descent invariant, exclude every nontrivial
cycle, or exclude every unbounded orbit. There is no defensible percentage
of progress toward a proof.

Unanswered questions for future work include whether comparable real-orbit
witnesses exist at larger factor lengths, whether the modulus-127 problem
has an exact certificate, and whether an interpretation retaining digit
order, such as noncommuting matrices with suitable invariants, can certify
descent. These are recorded directions, not running jobs or claimed
solutions.

## Reproduce the checked results

From the repository root, with the release RAD executable built and Python
with `z3-solver` installed:

```powershell
py -O projects/dogfood/collatz-termination/local-rank/accept.py
```

This rechecks all **20 durable certificates**, reconstructs the 330-rewrite
literal path, compares worker configurations and recordings, checks a
positive reduced-system control and its rejection after restoring the
missing rule, rejects **nine deliberately corrupted inputs in RAD**, and
runs **14 Python regression tests**. Substantive Python acceptance checks
raise explicit errors and still execute under `-O`.

The [certificate manifest](certificates/manifest.json), compact CSV
certificates, and [orbit witness](certificates/orbit.json) are durable
inputs. [evidence.json](evidence.json) records source, certificate, binary,
and raw-artifact hashes. Generated `out/` files are ignored by Git and
include unsuccessful searches as well as successes. Historical raw runs
can predate final source edits; acceptance reconstructs the durable proofs
from the final source. SciPy is not required for acceptance, and no costly
search campaign needs to be repeated to check the saved obstructions.
