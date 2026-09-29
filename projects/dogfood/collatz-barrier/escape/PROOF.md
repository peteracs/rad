# Polynomial escape of Collatz coefficient spread

This note proves an explicit finite restriction on every positive integer
Collatz orbit. The qualitative exclusion also follows from older orbit-sparsity
results; see Section 6. Priority for the explicit finite bound is unresolved.
It does not prove the
Collatz conjecture or that the multiplicative coefficient eventually contracts.

Use the shortcut map

\[
T(x)=\begin{cases}x/2&x\text{ even},\\(3x+1)/2&x\text{ odd}.\end{cases}
\]

For a positive integer start \(n\), write \(x_j=T^j(n)\), let \(q_j\) count
odd steps among the first \(j\) transitions, and define

\[
P_j=3^{q_j}/2^j,\qquad
R_N=\frac{\max_{0\le j\le N}P_j}{\min_{0\le j\le N}P_j}.
\tag{1}
\]

Thus \(P_0=1\) and \(R_N\ge1\). The quantity studied is **spread**, including
both the minimum and the maximum. Even on the cycle \(1,2,1,2,\ldots\),
the maximum stays at \(3/2\) while the minimum tends to zero.

## 1. Main result

Put

\[
z=39/25,\quad t=\log_3z,\quad s=\log_2(1+z),\quad
\eta=s-t,\quad \gamma=\frac{1-\eta}{s}.
\tag{2}
\]

The exact certificate described in Section 5 proves

\[
0.03585657<\gamma<0.03585658,\qquad \gamma>1/28.
\tag{3}
\]

**Theorem.** If \(x_0,\ldots,x_N\) are pairwise distinct and
\(N\ge\max(3n,16)\), then

\[
R_N>\frac14N^\gamma>\frac14N^{1/28}.
\tag{4}
\]

For every positive integer start, whether or not its orbit repeats,

\[
\liminf_{N\to\infty}\frac{\log R_N}{\log N}\ge\gamma.
\tag{5}
\]

No published computational verification floor is an assumption of this theorem.

## 2. Height and parity-block counting

**Height lemma.** Every \(j\le N\) satisfies

\[
x_j=P_j\left(n+\sum_{\substack{0\le i<j\\x_i\text{ odd}}}
                  \frac1{3P_i}\right)
\le R_N(n+j/3).
\tag{6}
\]

Indeed, division by \(P_{i+1}\) leaves \(x_i/P_i\) unchanged on an even
step and adds \(1/(3P_i)\) on an odd step. Also \(P_j\le R_N\), because
the minimum is at most \(P_0=1\), and \(P_j/P_i\le R_N\).

**Congruence lemma.** Two integers with the same first \(k\) parities are
congruent modulo \(2^k\). Subtract their affine formulas to get

\[
T^k(x)-T^k(y)=3^a(x-y)/2^k,
\]

where \(a\) is their common odd count. Integrality and
\(\gcd(3^a,2^k)=1\) prove the claim. Consequently, distinct integers in
\([1,H]\) have distinct length-\(k\) parity words whenever \(2^k>H\).

**Word-count lemma.** If a length-\(k\) block starts at time \(u\) with
\(u+k\le N\), its odd count \(a\) obeys

\[
3^a/2^k=P_{u+k}/P_u\ge1/R_N.
\]

For any \(z>1\), let \(t=\log_3z\) and
\(\eta=\log_2(1+z)-t\). The number \(W(k,R)\) of binary words meeting
the terminal condition \(3^a R\ge2^k\) therefore satisfies

\[
\begin{aligned}
W(k,R)
&=\sum_{3^aR\ge2^k}\binom ka\\
&\le R^t2^{-kt}\sum_{a=0}^k\binom ka z^a
=R^t(2^k)^\eta.
\end{aligned}
\tag{7}
\]

The first inequality follows from \(z^a\ge(2^k/R)^t\) on the summed
terms. No probability model, independence assumption, or assertion about a
typical orbit is involved. Counting by the terminal condition alone is a valid
upper bound; additional prefix conditions can only reduce it.

## 3. Proof of the theorem

Fix \(N\ge\max(3n,16)\), and abbreviate \(R=R_N\). Let

\[
H=R(n+N/3)\le2RN/3,\qquad k=\lfloor\log_2H\rfloor+1.
\]

Then \(H<2^k\le2H\le2RN\). For the choice (2), \(s>1\),
\(0<\eta<1\), and \(0<\gamma<1\): positivity of \(\eta\) follows
from \(1+z>z>1\) and \(\log2<\log3\); \(\eta<1\) follows from
the positive certificate for \(\gamma\).

If \(k\le N/2\), the states at times \(0,\ldots,N-k\) give
\(N-k+1>N/2\) distinct length-\(k\) parity words. All lookahead ends
by time \(N\). By (6), the congruence lemma, and (7),

\[
N/2<W(k,R)\le R^t(2^k)^\eta
\le R^{t+\eta}(2N)^\eta=R^s(2N)^\eta.
\]

It follows that

\[
R>2^{-(1+\eta)/s}N^{(1-\eta)/s}>N^\gamma/4,
\]

since \((1+\eta)/s<2\).

If \(k>N/2\), the definition of \(k\) already gives

\[
R>\frac{2^{N/2}}{2N}\ge N/2>N^\gamma/4.
\]

Here \(2^{N/2}\ge N^2\) for real \(N\ge16\): equality holds at
16, and the derivative of \((N/2)\log2-2\log N\) is positive
thereafter. This case uses no parity blocks and also covers \(k>N\).
This proves (4).

If an infinite orbit never repeats, apply (4) to every sufficiently large
\(N\), take logarithms, and divide by \(\log N\). If it repeats,
determinism makes it eventually periodic. A positive cycle of length
\(\ell\) contains an odd step; its return map has the form

\[
y\longmapsto cy+d,\qquad c=3^a/2^\ell,\quad d>0.
\]

At its positive fixed point, \(y=cy+d\) forces \(c<1\). Each cycle
multiplies \(P\) by this fixed \(c\), so its minimum decays exponentially
and \(R_N\) grows exponentially. In that case the ratio in (5) tends
to infinity. This proves (5) in all cases.

**Critical-discrepancy consequence.** The range of
\(q_j-j\log_3 2\), over \(0\le j\le N\), is exactly
\(\log_3 R_N\). In the distinct-state case of (4), it exceeds

\[
\frac{\log N}{28\log3}-\log_3 4.
\tag{8}
\]

In particular, no positive integer orbit has its critical discrepancy contained
in a fixed bounded interval forever.

If an orbit never has a coefficient contraction, then \(P_j\ge1\) for
all \(j\). Such an orbit cannot repeat, by the positive-cycle argument.
Its minimum coefficient is 1, so (4) becomes
\(\max_{j\le N}P_j>N^{1/28}/4\) for \(N\ge\max(3n,16)\).
This implication does not assert that a contraction eventually occurs.

## 4. Exact deadlines for fixed spread bounds

For \(M\ge1\), let \(S(k,M)\) count binary words whose every prefix
coefficient lies in \([1/M,M]\). This symmetric interval is necessary for
suffixes of an orbit of global spread at most \(M\); it is not sufficient
to make their own spread at most \(M\). Using this larger language is safe
for the following upper bound.

Put \(D=2^k\), \(r=S(k,M)\), and suppose
\(\Delta=3D-Mr>0\). Define

\[
a=\left\lceil\frac{3Mr}{\Delta}\right\rceil,\qquad
b=\left\lceil\frac{3rD}{\Delta}\right\rceil.
\tag{9}
\]

**Deadline lemma.** By step \(an+b+k\), either a state has repeated or
the coefficient spread has exceeded \(M\).

For a contradiction, put \(L=an+b+1\), and suppose the states through
time \(L+k-1\) are distinct and their spread is at most \(M\). The
first \(L\) states are at most \(M(n+L/3)\). Each has a length-\(k\)
word counted by \(r\), and each word occupies at most one residue class
modulo \(D\), by the congruence lemma. A class contains at most
\(H/D+1\) positive integers at most \(H\). Thus

\[
L\le r\left(\frac{M(n+L/3)}{D}+1\right),\qquad
\Delta L\le3Mrn+3rD,
\]

which implies \(L\le an+b\), a contradiction. The extra \(k\) in
the deadline supplies lookahead; dropping it is not justified.

Such a \(k\) exists for each fixed \(M\): (7) gives
\(S(k,M)/2^k\le M^t2^{-(1-\eta)k}\to0\).

RAD and an independent Python dynamic program give these sample certificates:

| Spread cap \(M\) | Block length \(k\) | Words \(r\) | \(a\) | \(b\) |
|---:|---:|---:|---:|---:|
| 2 | 1 | 2 | 6 | 6 |
| 4 | 4 | 10 | 15 | 60 |
| 8 | 13 | 2974 | 92 | 93226 |
| 16 | 28 | 49364507 | 154 | 2569012161 |
| 32 | 48 | 24909828526998 | 51 | 444605679428720 |

For example, by step \(15n+64\), there must be either a repeated state
or spread greater than 4. The [receipt](evidence.json) also includes caps
64, 128, and 256. These use the first feasible block length in the search;
they do not optimize the tradeoff between \(a\) and \(b\).

## 5. Exact exponent certification

All calculations use integers at scale \(S=2^{192}\). For
\(0\le r=a/b\le1/3\), use

\[
\log\frac{1+r}{1-r}=2\sum_{j=0}^{191}\frac{r^{2j+1}}{2j+1}
+\text{tail},\qquad
0\le\text{tail}\le\frac{2r^{385}}{385(1-r^2)}.
\tag{10}
\]

Set \(y_0=\lfloor Sa/b\rfloor\),
\(y_{j+1}=\lfloor y_j a^2/b^2\rfloor\), and
\(L=2\sum_{j=0}^{191}\lfloor y_j/(2j+1)\rfloor\).
The error in each \(y_j\) is less than \(9/8\) scale units, by
the recurrence \(e_{j+1}<e_j/9+1\). Each doubled term consequently
loses less than 6 units, and the scaled tail is less than 1. Hence

\[
L/S\le\log((1+r)/(1-r))<(L+1153)/S.
\tag{11}
\]

For \(\log2\) and \(\log3\), the imported routine uses
\(r=1/3\) and \(r=1/2\) with nested division. Its powers equal
\(\lfloor Sr^{2j+1}\rfloor\) exactly; each doubled term loses
less than 2 units, yielding the tighter upper endpoint \(L+385\).
The independent checker verifies both kinds of intervals against the exact
rational partial sum and tail in (10), including the \(r=1/2\) case.

For \(z=p/q\), compute \(\log z\) with
\(r=(p-q)/(p+q)\). Compute \(\log(1+z)\) as \(\log2\) plus
(10) with \(r=(p-q)/(p+3q)\). With

\[
U=\log2(\log3+\log z),\qquad V=\log3\log(1+z),
\quad\gamma=U/V-1,
\]

positive interval multiplication produces \(U_-\le S^2U\le U_+\)
and \(V_-\le S^2V\le V_+\). At \(p=156,q=100\), let
\(F=3585657\). The certificate reports strictly positive integers for

\[
10^8U_--(10^8+F)V_+>0,\qquad
(10^8+F+1)V_--10^8U_+>0.
\tag{12}
\]

These prove (3), including \(\gamma>1/28\) because
\(28F=100398396>10^8\). The 21 tested weights range from 150/100
through 170/100. Their certified exponent intervals select 156/100.
This establishes the best member of that finite portfolio, not a globally
optimal choice of real weight or an optimal exponent for the theorem.

## 6. Prior work, significance, and remaining gap

The parity-block congruence and pigeonhole technique is established.
In particular, Section 5 of Arturas Dubickas's
[On Integer Sequences Generated by Linear Maps (2009)](https://www.cambridge.org/core/services/aop-cambridge-core/content/view/C40C0C07FEC20797475BB2899C436C9A/S0017089508004655a.pdf/on_integer_sequences_generated_by_linear_maps.pdf)
uses equal parity blocks to obtain divisibility by a power of 2. It proves a
linear lower bound on parity factor complexity for a divergent Collatz orbit.
This note combines that mechanism with a spread-dependent height estimate
and a weighted binomial count. It does not claim to introduce the congruence
method. A fuller priority review is needed for the resulting spread inequality.

Garcia and Tal's
[A note on the generalized 3n+1 problem (1999)](https://doi.org/10.4064/aa-90-3-245-250)
is a closer predecessor. Their Proposition 1 and Lemma 3 imply that an
infinite orbit \(O\) satisfies
\(\#(O\cap[a,a+X))\le C X^\beta\log(2X)\) for some \(\beta<1\).
Their Corollary 1 states the resulting zero Banach density. Equal-time
iterates are injective on an infinite orbit, so Lemma 3 applies directly.
Dyadic summation gives \(\sum_j1/x_j<\infty\). In the identity

\[
x_j=nP_j\prod_{\substack{i<j\\x_i\text{ odd}}}
                   (1+1/(3x_i)),
\]

the product therefore has a finite positive limit. Since an infinite positive
integer orbit tends to infinity, \(P_j\to\infty\). This is a deduction
from their estimates, not a new theorem claimed here. It already excludes
bounded critical bands. It also gives stronger asymptotic information:
distinctness implies \(\max_{j\le N}x_j\ge N+1\), hence
\(R_N\ge c_n(N+1)\) for a positive constant \(c_n\).
The result in Sections 1--5 supplies explicit finite constants without an
infinite-orbit assumption; it does not improve that asymptotic theory.

The gain over finite trajectory experiments is a written finite theorem with
explicit constants and escape-or-repeat deadlines. It neither excludes nontrivial positive
cycles nor excludes divergent orbits with unbounded coefficient excursions.
Those are precisely compatible with the conclusion. The theorem therefore
does not settle Collatz, and this note makes no claim of a major breakthrough.

RAD certifies finite arithmetic and reproducible execution. The universal
claims depend on the written proof above. This is not an end-to-end formal
proof checked by a theorem prover.
