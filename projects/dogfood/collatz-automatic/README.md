# What a Collatz termination certificate must be

Working notes. Nothing here proves Collatz.

## 1. Certificates are automatic Lyapunov functions (proved)

Consider any Yolcu–Aaronson–Heule certificate for the mixed binary/ternary
system in [PROOF_TARGET.md](../collatz-termination/PROOF_TARGET.md). Every
rule weakly decreases in all contexts, and the boundary rules `ce -> cb`,
`cf -> caa` and `cg -> cab` strictly decrease at the front of the word.

Let `Φ(n)` be the rank of the binary word `c bin(n) d`. Then:

- **odd n >= 3:** `Φ(T n) <= Φ(n) - ε`;
- **even n:** `Φ(n/2) <= Φ(n)`.

**Proof.** An odd step applies `bd -> gd`. Every conversion rule has exactly
one ternary symbol on each side, so the single ternary symbol walks left one
position per rule until it meets `c`. There it fires exactly one strict
boundary rule, and the result is the binary word of `T n`. An even step is
`ad -> d`. QED.

For a matrix interpretation, `Φ` is a weighted automaton over the binary
digits of `n`, so it is a 2-regular sequence. The method succeeds only if
Collatz has a Lyapunov function of this finite-automaton form.

`lemma.rad` checks the combinatorial core on every trajectory from 2 to 299.
That covers 4664 odd steps, each with exactly one boundary firing and never
more than one ternary symbol. Each rewritten word also equals the binary word
of the arithmetic successor. The run takes under 0.5 s.

## 2. A rigorous necessary condition, and why it is not enough

Terras's theorem says the parity vectors of the first `j` steps are a
bijection on residues mod `2^j`. Sum the Lyapunov inequality over
`n = 2^j t + r` for all residues `r`. The average of `Φ` over j-bit blocks is
then at least `ε j / 2`. For matrices this means
`ρ(M_a + M_b) >= 2` on the reachable part.

**Self-check.** The argument never uses the multiplier 3. It holds verbatim
for `3x - 1`, which has the nontrivial cycles `5 -> 7 -> 5` and a 17-cycle,
so no certificate can exist there. The condition therefore cannot separate
true from false Collatz-type statements.

## 3. Calibration rule for every further argument

A necessary condition, or a no-go theorem, only counts if it passes three
tests:

1. It must fail for `3x - 1` and for `5x + 1`, which have nontrivial cycles
   or apparent divergence.
2. It must not rule out the weakened systems that Yolcu, Aaronson and Heule
   proved terminating.
3. Only then may it be applied to `3x + 1`.

Coarse spectral and averaging arguments fail test 1. Any real progress on
this route must use arithmetic that distinguishes `+1` from `-1`.

## 4. Calibration harness (RAD)

`calibrate.rad` runs 3x+1, 3x-1 and 5x+1 in three forked worlds. Findings
are submitted as `Report` intents and tallied by one resolver, and
`why(verdict, Verdict)` shows the world behind each verdict. The run takes
0.5 s:

- 3x-1 is blocked by the cycle 5, 7, 10.
- 5x+1 is blocked by the cycle 13, 33, 83, 208, 104, 52, 26.
- 3x+1 has no obstruction below 3000.

## 5. Mirror conservation law (proved; checked by `mirror.rad`)

Let `T+` be 3x+1 and `T-` be 3x-1, both shortcut maps. For `1 <= n < 2^L`
and `0 <= j <= L`, with `q` the number of odd steps among the first `j`:

```text
T-^j(2^L - n) = 3^q 2^(L-j) - T+^j(n),
```

and the two parity vectors agree for `j < L`.

**Proof.** Use induction on `j`. While `j < L` the term `3^q 2^(L-j)` is
even, so both numbers have the same parity. An even step halves both sides.
An odd step gives
`(3(3^q 2^(L-j) - x) - 1)/2 = 3^(q+1) 2^(L-j-1) - (3x+1)/2`. QED.

`mirror.rad` checks every `n < 2^L` for `L <= 12`: 8178 pairs and 90036
steps, in 0.12 s.

**Consequence for certificates.** Suppose `m` lies on a 3x-1 cycle with
odd-step density `θ`. Then the 3x+1 trajectory of `2^L - m` follows that
cycle's parity pattern for `L` steps. Every certificate must therefore
satisfy

```text
Φ(2^L - m) >= ε θ L - O(1),
```

with `θ = 1` for `m = 1`, `2/3` for the 5-cycle, and `7/11` for the
17-cycle. These constraints depend on the `+1` versus `-1` arithmetic, so
they pass the calibration rule.

For a matrix interpretation they fix how the automaton behaves on the words
`1^(L-k) w_m`, where `w_m` is the complemented low block. On the part of the
state space these words reach, `M_b` needs either spectral radius above 1 or
a Jordan block of size at least 2 at eigenvalue 1. The growth must also
dominate `ε θ L` for every 3x-1 cycle at once. These are exact spectral
requirements that can seed the certificate search in
`../collatz-termination`, in place of bounded-entry enumeration.

## 6. What a successful certificate looks like (Farkas, verified in RAD)

Yolcu, Aaronson and Heule prove Farkas's terminating variant of 3x+1 with a
5-dimensional arctic (max-plus) interpretation of the reversed system. Its
dynamic rules are strict at the top, and the conversions are weak.
`farkas/arctic.rad` transcribes that certificate. `farkas/check.rad` replays
real rewrites from every `n < 200`, starting from ternary words, with dynamic
rules taking priority. Across 3260 rewrites and 762 dynamic steps, no weak
rule increases the rank, and every dynamic step lowers it by at least 1. The
run takes 0.74 s.

`farkas/explain.rad` follows the argmax provenance of each max-plus value.
Every potential value is one path that restarts at a single digit and then
adds fixed per-digit weights: `e` gives +3 or +4, `f` +1 to +3, and the last
`g` +2. For example:

```text
27: d@0<-4 e@4<-2(+4) e@2<-2(+4) e@2<-0(+3) c@0:start1   rank 12
41: d@0<-4 g@4<-0(+2) f@0<-2(+3) f@2<-0(+3) c@0:start1   rank 9
```

So the Farkas certificate is a digit-weight count with a little state
context. It succeeds because every Farkas dynamic rule acts at the end of a
mixed word and removes weight there. For instance, `(n-1)/3` deletes a
ternary digit.

**The same mechanism for 3x+1.** Give digit weights `w_s`. The odd step
`bd -> gd` must decrease, so `w_b > w_g`. The leading conversion
`cg -> cab` must not increase, so `w_g >= w_a + w_b`. The even step
`ad -> d` needs `w_a > 0`. These three inequalities together give
`w_b > w_b`. The cause is that an odd step, once its ternary digit reaches
the front, adds a net binary digit. The potential is paying for the size
growth of 3x+1, which Farkas's map offsets with its `(n-1)/3` rule.

A 3x+1 certificate therefore cannot be a digit count. Its max-plus paths
must depend on position, so that a digit near the end is worth more than the
digits it becomes at the front. That dependence is what must anticipate
future halvings. Section 1 shows how much it must anticipate: in the
reversed orientation every step is strict, so the potential dominates the
total stopping time.
