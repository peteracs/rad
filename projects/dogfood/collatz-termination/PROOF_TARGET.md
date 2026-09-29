# The precise uniform proof being searched for

This file states a sufficient condition for Collatz convergence and explains
why the finite certificate checks would establish it. **The campaign has not
found a certificate meeting that condition.**

## 1. The complete rewriting system

Use binary symbols `a,b`, ternary symbols `e,f,g`, and boundary symbols `c,d`.
The rules are:

| Index | Rule | Kind |
|---:|---|---|
| 0 | `ad -> d` | even Collatz step |
| 1 | `bd -> gd` | odd shortcut step |
| 2 | `ae -> ea` | base conversion |
| 3 | `af -> eb` | base conversion |
| 4 | `ag -> fa` | base conversion |
| 5 | `be -> fb` | base conversion |
| 6 | `bf -> ga` | base conversion |
| 7 | `bg -> gb` | base conversion |
| 8 | `ce -> cb` | leading-digit conversion |
| 9 | `cf -> caa` | leading-digit conversion |
| 10 | `cg -> cab` | leading-digit conversion |

Valid words have exactly one `c` at the beginning and one `d` at the end.
To decode the interior, start with the leading integer 1; reading `a,b`
replaces the value by `2x,2x+1`, while `e,f,g` replaces it by
`3x,3x+1,3x+2`. Thus `cd` represents 1. Every positive integer has a valid
binary representation of this form.

Rules 2 through 10 preserve the represented integer, as direct affine
substitution shows. Rule 0 divides an even value by two. Rule 1 sends an odd
value `2x+1` to `3x+2`, its shortcut Collatz successor.

Base conversion terminates: order words lexicographically by their number
of ternary symbols and then the number of pairs in which a binary symbol
precedes a ternary symbol. Rules 8 through 10 decrease the first count;
rules 2 through 7 preserve it and decrease the second count. A valid word
with a ternary symbol always admits one of those conversion rules. Hence
conversion eventually produces a binary representation.

Starting with any positive integer greater than 1, we can therefore repeatedly
convert to binary and apply a dynamic rule. A nonconvergent Collatz trajectory
would produce an infinite valid rewrite sequence. Proving termination of all
valid rewrite sequences is consequently sufficient to prove Collatz.

This is the mixed-base construction in Yolcu, Aaronson, and Heule,
[An Automated Approach to the Collatz Conjecture](https://emreyolcu.com/research/rewriting-collatz.pdf),
Section 3.2. The ASCII rule spelling agrees with the
[authors' rule file](https://github.com/emreyolcu/rewriting-collatz/blob/main/rules/collatz-T.srs).
The construction is not claimed as new here.

## 2. Why only the boundary rules need strict decrease

Let B be rules 8,9,10 and D be rules 0,1. Without B, the full system
terminates by the lexicographic measure (number of binary symbols, number
of binary-before-ternary pairs). Dynamic rules reduce the first count;
conversion rules preserve it and reduce the second count.

Without D, the conversion argument in Section 1 proves termination.

Therefore either of the following would suffice:

1. Every rule is weakly decreasing in a nonnegative ranking quantity,
   and every B rule decreases it by at least some fixed positive epsilon.
2. Reverse every word, require every rule to be weakly decreasing, and
   require every reversed D rule to decrease the quantity by at least a
   fixed positive epsilon.

In either case, only finitely many strict boundary steps can occur, because
the initial ranking value is finite and bounded below by zero. After their
last occurrence the remaining rules terminate by the measures above.

The strict rules occur only at the left edge of a valid word (after reversal
in case 2). Their strictness need not survive an arbitrary prefix context.
Weak decrease must survive every context. This distinction permits the
relaxed interpretations checked by RAD. We claim sufficiency for valid words;
we do not silently apply this boundary argument to arbitrary strings with
multiple boundary symbols.

## 3. Nonnegative rational matrix certificates

Interpret a symbol as `f_s(x)=A_s x+b_s` on nonnegative real vectors, with
nonnegative rational coefficients and a common positive denominator Q.
Interpret a word by function composition in its written order. Every symbol
preserves componentwise weak order and the domain.

For each rule, it suffices that every coefficient and constant in the left
word's affine interpretation is at least its counterpart on the right. This
ensures weak decrease inside any context. For a selected boundary rule,
require also that the first constant coordinate is strictly larger.
That gives a uniform positive decrease independent of the suffix vector.

Although the domain is real, strict decrease is not merely an arbitrarily
small decrease: finitely many rational gaps have a positive minimum. The
ranking coordinate can sustain at most its initial value divided by that
minimum gap strict steps. No discreteness assumption on real vectors is used.

The solver stores homogeneous matrices as integer numerators. A word of
length L has denominator `Q^L`. For unequal word lengths, the checker compares
integer products after cross multiplication. In particular it does not
compare unnormalized numerator matrices. `Q=1` gives integer matrices.

For dimension at most 16, numerators and Q at most 31, and word length at
most 3, every cross-multiplied entry is at most
`17^2 * 31^6 < 2^39`. RAD's signed 64-bit arithmetic is exact throughout.
The bit-vector encoder chooses a larger width than the corresponding
nonnegative upper bound, so no modular overflow is used to satisfy a query.

## 4. Max-plus certificates

Here addition is maximum and multiplication is addition, with a separate
minus-infinity value. Finite letter coefficients are integers in a searched
bounded interval; minus infinity is represented by `-1000000` only in the
artifact format. It is absorbing under multiplication, not an ordinary
negative weight. Word products have length at most three, so finite sums
cannot reach this sentinel.

The carrier has a nonnegative finite first coordinate; other coordinates
may be arbitrary integers or minus infinity. For every letter, either its
first diagonal coefficient or its first affine constant must be nonnegative.
This guarantees closure of the carrier, including finiteness of the first
output coordinate.

Require coefficientwise weak decrease for every rule. On a selected boundary
rule, require every first-row coefficient and constant to be strictly larger,
except that minus infinity may compare with minus infinity. The right first
coordinate is finite, so a maximizing right-hand term exists and its matching
left term increases by at least one. Thus the first coordinate strictly drops
by at least one when rewriting from left to right.

## 5. Nonlinear scalar polynomial certificates

Interpret each symbol by a polynomial with nonnegative integer coefficients
on nonnegative integers. Such functions preserve the domain and weak order.
For every rule require the polynomial difference, left minus right, to have
only nonnegative coefficients. For a selected boundary rule require its
constant difference to be positive. These conditions imply the same weak
context preservation and a strict decrease of at least one at the boundary.

The current domains are degrees up to three with coefficients at most seven,
or degree four with coefficients at most two. To bound intermediates, put
`u_0=1` and `u_(i+1)=M*(1+u_i+...+u_i^p)`. Then `u_3` bounds every
coefficient of a word of length at most three. These domains give `u_3<2^62`.
RAD checks with signed integers; Python uses unbounded integers and a separate
coefficient expansion; the solver can use either bounded bit vectors or
nonlinear integer arithmetic.

## 6. Integer-valued polynomials on the discrete domain

[NEWTON.md](NEWTON.md) extends the scalar interpretation to integer-valued
polynomials in the binomial basis, including signed coefficients. Exact
finite-difference certificates prove closure, monotonicity, and every rule
comparison on all nonnegative integers. RAD uses arbitrary-precision
arithmetic, so the signed 64-bit bounds of Section 5 do not restrict this
separate certificate family. The strict-boundary termination argument is
the same. That document also proves the grounding and degree reductions
used to make the synthesis problem smaller.

## 7. What the receipt means

Every full-system search asks for weak decrease of all eleven rules and
strict decrease of at least one selected boundary rule. This is the first
stage of a possible rule-removal proof. A model orienting only some boundary
rules is partial: the remaining rules still require a checked argument.
A model making all selected boundary rules strict closes Section 2 directly.

A search with an omitted rule is a control or a reduced problem. It cannot
receive `complete_proof=true`. A timeout or UNSAT report cannot receive that
flag either. None of the full-system queries in this campaign produced a
model, partial or complete. Thus the universal proof obligation remains open.

UNSAT means the solver rejected one bounded template family; it does not
exclude larger dimensions, larger coefficients, different denominators,
different degrees, or different interpretation methods. UNKNOWN is preserved
with its reason. The independent checkers validate concrete models, not the
solver's absence claims.
