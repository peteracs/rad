# A uniform bound for the Collatz correction product

This note gives an explicit bound for the accumulated additive correction on
every finite positive Collatz orbit segment with distinct states. The argument
uses integer image collisions, followed by a convergent sum over dyadic
intervals. It does not assume eventual convergence to 1.

Use \(T(x)=x/2\) for even \(x\), and \(T(x)=(3x+1)/2\) for odd
\(x\). For \(x_j=T^j(n)\), with \(n\ge1\), let \(q_j\) count
odd transitions before time \(j\), and set

\[
P_j=3^{q_j}/2^j,\qquad
Q_j=\prod_{\substack{0\le i<j\\x_i\text{ odd}}}
               \left(1+\frac1{3x_i}\right).
\]

The exact product identity is \(x_j=nP_jQ_j\).

## 1. Uniform finite theorem

**Theorem.** If \(x_0,\ldots,x_N\) are pairwise distinct, then

\[
\sum_{\substack{0\le i\le N\\x_i\text{ odd}}}\frac1{x_i}
<S_0<33\log2,\qquad
1\le Q_j<2048\quad(0\le j\le N).
\tag{1}
\]

Here \(S_0\) is an explicit rational number, approximately 21.5024,
whose exact numerator and denominator are in the RAD output. Consequently,

\[
x_j<2048\,nP_j.
\tag{2}
\]

The constant is independent of \(n,N\), the number of odd steps, and
the maximum value along the segment. Its value is not claimed optimal.

## 2. Exact image capacities

Fix \(k\ge1\) and let \(D=2^k\). For each residue \(0\le r<D\),
write \(a(r)\) for its odd count through \(k\) steps and
\(y(r)=T^k(r)\). Then

\[
0\le y(r)<3^{a(r)},\qquad
a(r)>0\Longrightarrow y(r)\not\equiv0\pmod3.
\tag{3}
\]

For the first assertion, induct on the intermediate bound
\(x_i<3^{a_i}2^{k-i}\). Before the last transition the right side is
an even integer. An even input is at most that bound minus 2; an odd input
is at most that bound minus 1. Either branch gives the corresponding strict
bound at the next step. An odd transition produces a nonzero residue modulo
3, and subsequent divisions by 2 preserve that property, proving the second
assertion.

Two equal length-\(k\) parity words imply equal residues modulo \(D\),
by subtracting their affine formulas. Thus the \(D\) residues correspond
bijectively to the \(D\) binary parity words. Among odd residues, the
number with \(q\) odd steps is \(\binom{k-1}{q-1}\).

The number of distinct pairs \((a(r),y(r))\) over **odd** residues is
therefore at most

\[
B_k=\sum_{q=1}^k\min\left\{\binom{k-1}{q-1},\;2\cdot3^{q-1}\right\}.
\tag{4}
\]

Define \(B_0=1\) separately. The odd count must be retained: at depth
3, residues 1 and 4 both end at 2, but their odd counts are 2 and 1.
After translating both inputs by 8, they end at 11 and 5.

For an integer block offset \(b\ge0\), the affine identity gives

\[
T^k(r+bD)=y(r)+b3^{a(r)}.
\tag{5}
\]

It follows that the odd inputs in each aligned block
\([bD,(b+1)D)\) have at most \(B_k\) distinct images under \(T^k\).

## 3. A bound at every depth

Choose \(z=39/25\) and define

\[
\alpha=\frac{\log(1+z)}{\log(3z)},\qquad
\beta=\alpha\log_2 3.
\]

Split (4) at \(r=\lfloor\alpha k\rfloor\). The terms with
\(q\le r\) sum to at most \(3^r-1<3^{\alpha k}\). For the
remaining terms, weighted binomial counting gives

\[
\sum_{q>\alpha k}\binom{k-1}{q-1}
\le z^{-\alpha k}\,z(1+z)^{k-1}
=\frac{z}{1+z}\,3^{\alpha k}.
\]

Thus

\[
B_k<\frac{103}{64}\,2^{\beta k}.
\tag{6}
\]

The [exact exponent certificate](../escape/PROOF.md) proves
\(\gamma>1/28\), where
\(1+\gamma=\log2\log(3z)/(\log3\log(1+z))=1/\beta\).
Therefore \(\beta<28/29\). The exact integer comparison

\[
2\cdot42^{29}-43^{29}
=2540082905201934529830083110292607564845846501>0
\]

proves \(2^{-1/29}<42/43\). Combining these facts gives the
all-depth geometric bound

\[
\frac{B_k}{2^k}<\frac{103}{64}(42/43)^k.
\tag{7}
\]

The same inequality holds directly for \(k=0\).

## 4. Finite lookahead and reciprocal summation

Assume \(x_0,\ldots,x_N\) are distinct. Fix \(k\le N\). For
indices \(i\le N-k\), the values \(T^k(x_i)=x_{i+k}\) are distinct.
By (5), at most \(B_k\) such odd states lie in \([2^k,2^{k+1})\).
There are only \(k\) remaining indices. Hence

\[
\#\{i\le N:x_i\text{ odd},\ 2^k\le x_i<2^{k+1}\}\le B_k+k.
\tag{8}
\]

For \(k>N\), the bound holds because there are at most \(N+1\le k\)
states altogether. At \(k=0\), the interval contains only 1, so (8)
holds with \(B_0=1\).

The terminal allowance \(k\) is essential to this proof. A distinct
prefix need not remain injective under arbitrary future lookahead: the prefix
\(3,5,8,4,2,1\) is distinct, but \(T(4)=T(1)=2\).

Using \(1/x_i\le2^{-k}\) in its dyadic interval and summing (8),

\[
\sum_{\substack{i\le N\\x_i\text{ odd}}}\frac1{x_i}
\le\sum_{k=0}^{\infty}\frac{B_k+k}{2^k}.
\]

Compute (4) exactly through \(K=256\), use (7) afterwards, and use
\(\sum_{k=0}^{\infty}k/2^k=2\). This yields

\[
S_0=2+\sum_{k=0}^{256}\frac{B_k}{2^k}
       +\frac{4429}{64}(42/43)^{257}.
\tag{9}
\]

RAD proves \(S_0<33\log2\) by integer interval arithmetic. Finally,
\(\log(1+u)\le u\) gives

\[
\log Q_j\le\frac13
\sum_{\substack{i<j\\x_i\text{ odd}}}\frac1{x_i}
<11\log2,
\]

proving (1) and (2).

## 5. Smaller correction above a large minimum

If all states in the segment are at least \(2^h\), the same proof starts
the dyadic sum at \(h\). For \(0\le h\le256\), put

\[
S_h=\frac{2(h+1)}{2^h}
+\sum_{k=h}^{256}\frac{B_k}{2^k}
+\frac{4429}{64}(42/43)^{257}.
\tag{10}
\]

The same bound also controls the reciprocal contribution of states at least
\(2^h\) in an arbitrary distinct segment. The certificate proves
\(S_h<3b\log2\) for these values:

| Minimum state | Certified \(b\) | Correction \(Q_j<2^b\) |
|---:|---:|---:|
| 1 | 11 | 2048 |
| \(2^{16}\) | 6 | 64 |
| \(2^{32}\) | 4 | 16 |
| \(2^{64}\) | 2 | 4 |
| \(2^{71}\) | 2 | 4 |
| \(2^{128}\) | 1 | 2 |
| \(2^{256}\) | 1 | 2 |

No external convergence verification is required for these implications.
The condition concerns every state of the segment, not only its start.

For reproducibility, (4) is computed by a Pascal recurrence in RAD and by
Python's independent integer binomial coefficients. The rational denominator
used in (10) is \(2^{256}\cdot64\cdot43^{257}\). If its numerator is
\(A\), and \(L/2^{192}\le\log2\) is the certified logarithm lower
bound, the output records the positive margin
\(3bDL-A2^{192}\). The inequality follows without rounding a logarithm
to floating point.

## 6. Consequences and the remaining proof target

An orbit that never repeats satisfies (2) at every time. Its positive integer
states are distinct and hence tend to infinity. Therefore \(P_j\to\infty\).
Also \(\max_{j\le N}x_j\ge N+1\), so

\[
\max_{j\le N}P_j>\frac{N+1}{2048n}.
\tag{11}
\]

On a distinct segment, a coefficient \(P_j\le1/2048\) forces
\(x_j<n\). If all states are at least \(2^{71}\), the sufficient
coefficient threshold improves to \(1/4\).

The theorem does not force either threshold to occur. Positive cycles also
remain a separate possibility. Repeating the cycle \(1,2\) for 54 steps
gives \(Q_{54}=(4/3)^{27}>2048\); removing the distinctness hypothesis
would make (1) false. A Collatz proof still needs to force descent or exclude
the remaining divergent and cyclic behavior.

For every \(n\) that reaches 1, the segment through its first arrival at 1
is distinct. The product in (1) is the usual Collatz residue at that arrival,
so the theorem gives \(\operatorname{Res}(n)<2048\), uniformly over
all such \(n\). This proves the boundedness assertion on the domain where
the residue is defined; it does not prove that every positive start reaches 1.

## 7. Relationship to prior work

Garcia and Tal's
[A note on the generalized 3n+1 problem (1999)](https://doi.org/10.4064/aa-90-3-245-250)
uses equal-time collisions to bound the density of individual orbits.
Its quantitative estimates already imply summable reciprocals on divergent
orbits. This note provides a direct binomial/image capacity, includes the
finite terminal allowance, and certifies explicit correction constants.
The existence of a bounded correction is not presented as unrelated to that
earlier work; priority for this particular finite formulation remains open.

Roosendaal's [weak residue conjecture](https://www.ericr.nl/wondrous/)
asks for a uniform residue bound, under the page's standing convergence
assumption. Equation (1) supplies such a bound on convergent trajectories
without that standing assumption. It does not prove the sharp bound at 993.
Luo's [Iteration Steps of 3x+1 Problem (2025)](https://arxiv.org/abs/2506.23070)
uses the same name for the more specific bound \(\operatorname{Res}(n)\le2\);
the present constant does not establish that stronger assertion.

Finite arithmetic certificates and literal checks support the written
all-size proof. The artifact is not an end-to-end theorem-prover proof.

## 8. Coalescence as an additional induction rule

The collision calculation also supplies a separate proof-search rule. If
\(0<s<r<2^k\) have the same odd count \(q\) and endpoint \(y\), then
for every integer \(b\ge0\),

\[
T^k(r+b2^k)=y+b3^q=T^k(s+b2^k).
\tag{12}
\]

Thus \(r+b2^k\) cannot be the least counterexample: its trajectory meets
that of the smaller positive integer \(s+b2^k\). This conclusion does not
require the coefficient to contract.

RAD exhausts all input residues through depth 18, groups by \((q,y)\),
and emits each surviving residue whose group has a smaller representative.
Here "surviving" means \(3^{q_j}\ge2^j\) at every positive prefix
\(j\le k\). All such prefixes actually increase the input, since the
additive term is positive. At depth 18, this removes 1391 of the 7495
coefficient-surviving residue classes. The certificate contains the smaller
representative, odd count, and endpoint for every removal. Python reconstructs
all groups independently and checks the translated trajectories as well.

The first example is \(T^6(15+64b)=T^6(14+64b)=20+81b\).
An elementary infinite family makes the distinction from contraction clear.
Let \(n=2^m u-1\), with \(m\ge1\), positive odd \(u\), and
\(u\equiv(-1)^m\pmod4\). After \(m\) steps the pair \((n,n-1)\)
becomes \((3v+2,v)\), where \(v=3^{m-1}u-1\equiv2\pmod4\).
Two more steps send both to \((3v+2)/4\). Their odd counts both equal
\(m\). When \(m\ge4\), the upper trajectory's coefficient stays
greater than 1 through the merge, since its parity word is \(1^m00\)
and \(3^m>2^{m+2}\).

This family is established prior work: see Ladue,
[Clusters of Integers with Equal Total Stopping Times in the 3X + 1 Problem
(2017)](https://arxiv.org/abs/1709.02979), especially Theorems 3.2 and 4.1,
and its comparison with Garner's 1985 results. We implement it within a more
general exhaustive affine-collision test; we do not claim to have discovered
the family first.

Neither the finite count nor this infinite subfamily exhausts the remaining
positive inputs. A universal induction proof must establish a reduction for
every possible least counterexample, not only show that many cylinders reduce.
