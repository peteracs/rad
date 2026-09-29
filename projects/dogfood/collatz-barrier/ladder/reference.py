"""Independent exact-rounding reference for the sharp envelope ladder.

Every table entry is an exact floor/ceiling of 2^{-{k alpha}} computed from
Python big integers (no recurrence, no floating point). Sums use numpy int64
only after checking that no partial sum can overflow.
"""

from fractions import Fraction
import numpy as np

SCALE_BITS = 40
S = 1 << SCALE_BITS
# sigma* = 3 s*, a dyadic rational so that RAD can compare without overflow.
SIGMA_NUM, SIGMA_BITS = 756421, 20
SIGMA = Fraction(SIGMA_NUM, 1 << SIGMA_BITS)
SIGMA_UNIT = SIGMA_NUM << (SCALE_BITS - SIGMA_BITS)  # sigma* * S, exact integer
BLOCK = 15601  # continued-fraction denominator of log_2 3


def require(condition, message):
    if not condition:
        raise ValueError(message)


def forward_bounds(count, top_bits=160):
    """lo/hi of S*2^{-{k alpha}} for 0 <= k < count, exact outward rounding."""
    lo, hi, p3 = [], [], 1
    for _ in range(count):
        fl = p3.bit_length() - 1
        shift = max(0, fl - top_bits)
        top = p3 >> shift  # 3^k in [top, top+1) * 2^shift
        numerator = 1 << (fl + SCALE_BITS - shift)
        if shift == 0:
            q, r = divmod(numerator, top)
            lo.append(q)
            hi.append(q + (r > 0))
        else:
            lo.append(numerator // (top + 1))
            hi.append(-(-numerator // top))
        p3 *= 3
    return lo, hi


def backward_bounds(count):
    """lo/hi of S*2^{-{-k alpha}} = S*3^k/2^{ceil(k alpha)} for 1 <= k < count."""
    lo, hi, p3 = [None], [None], 3
    for _ in range(1, count):
        b = p3.bit_length()  # ceil(k alpha) for k >= 1
        q, r = divmod(p3 << SCALE_BITS, 1 << b)
        lo.append(q)
        hi.append(q + (r > 0))
        p3 *= 3
    return lo, hi


def block_maxima(length):
    """Lower and upper bounds of M_m = max_x F_m(x), 0 <= m <= length.

    Between discontinuities F_m strictly decreases, so the maximum is taken at
    one of the m left endpoints x = {-h alpha}, 0 <= h < m.
    """
    flo, fhi = forward_bounds(length)
    blo, bhi = backward_bounds(length)
    result = []
    for fwd, back in ((flo, blo), (fhi, bhi)):
        table = [back[k] for k in range(length - 1, 0, -1)] + fwd  # k = -(length-1) .. length-1
        require(max(table) <= S, "phase value exceeds one")
        prefix = np.concatenate([[0], np.cumsum(np.array(table, dtype=np.int64))])
        require(int(prefix[-1]) < (1 << 62), "prefix sum overflow")
        origin = length - 1
        bound = [0]
        for m in range(1, length + 1):
            h = np.arange(m)
            bound.append(int((prefix[origin - h + m] - prefix[origin - h]).max()))
        result.append(bound)
    return result[0], result[1]


def phase_zero(count):
    lo, hi = forward_bounds(count)
    flo, fhi = [0], [0]
    for a, b in zip(lo, hi):
        flo.append(flo[-1] + a)
        fhi.append(fhi[-1] + b)
    return flo, fhi


def extremal_offsets(count):
    """Exact C_q for 0 <= q <= count: C_{h+1} = 3 C_h + 2^floor(h alpha)."""
    offsets, c, p3 = [0], 0, 1
    for _ in range(count):
        c = 3 * c + (1 << (p3.bit_length() - 1))
        p3 *= 3
        offsets.append(c)
    return offsets


def exact_f(offsets, q):
    """F_q(0) = 3 C_q / 3^q as an exact rational."""
    return Fraction(3 * offsets[q], 3 ** q)
