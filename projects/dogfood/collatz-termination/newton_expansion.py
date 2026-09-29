"""Division-free symbolic composition for the Newton search backend."""

from functools import lru_cache
from math import comb, factorial

import z3


def product(a,b):
    result = [0]*(len(a)+len(b)-1)
    for i,x in enumerate(a):
        for j,y in enumerate(b):
            result[i+j] = result[i+j]+x*y
    return [z3.simplify(x) if isinstance(x,z3.AstRef) else x for x in result]


def ordinary_numerators(vector):
    degree = len(vector)-1
    scale = factorial(degree)
    answer, falling = [0]*(degree+1), [1]
    for i,c in enumerate(vector):
        for j,v in enumerate(falling):
            answer[j] = answer[j]+c*(scale//factorial(i))*v
        falling = product(falling,[-i,1])
    return [z3.simplify(x) if isinstance(x,z3.AstRef) else x for x in answer]


def polynomial_certificate(difference,tail):
    size = len(difference)
    prefix = [z3.simplify(sum(c*x**j for j,c in enumerate(difference))) for x in range(tail)]
    coefficients = []
    for k in range(size):
        weights = [sum((-1)**(k-i)*comb(k,i)*(tail+i)**j for i in range(k+1))
                   for j in range(size)]
        coefficients.append(z3.simplify(sum(c*w for c,w in zip(difference,weights))))
    return prefix,coefficients


def rule_certificates(letters, degree):
    denominator = factorial(degree)
    polys = {s:ordinary_numerators(v) for s,v in letters.items()}
    for vector in polys.values():
        while len(vector)>1 and (z3.is_int_value(vector[-1]) or z3.is_bv_value(vector[-1])) and vector[-1].as_long()==0:
            vector.pop()

    @lru_cache(None)
    def word(text):
        if not text:
            return [0,1],1
        inner,divisor = word(text[1:])
        outer_degree = len(polys[text[0]])-1
        power = [1]
        answer = [0]*(outer_degree*(len(inner)-1)+1)
        for i,c in enumerate(polys[text[0]]):
            for j,v in enumerate(power):
                answer[j] = answer[j]+c*v*divisor**(outer_degree-i)
            if i < outer_degree:
                power = product(power,inner)
        return [z3.simplify(v) for v in answer],denominator*divisor**outer_degree

    def certificate(left,right,tail):
        a,ad = word(left)
        b,bd = word(right)
        size = max(len(a),len(b))
        a = a+[0]*(size-len(a)); b = b+[0]*(size-len(b))
        difference = [z3.simplify(x*bd-y*ad) for x,y in zip(a,b)]
        return polynomial_certificate(difference,tail)

    return certificate


def profile_arithmetic_bound(maximum,tail,rules):
    """Absolute bound for every numerator and tail-expression intermediate.

    Sum of absolute coefficients of x(x-1)...(x-i+1) is i!, hence a
    degree-p binomial polynomial with |c_i|<=M has numerator norm at most
    24*M*(p+1) with the common denominator 4!.
    """
    degrees = dict(a=2,b=2,c=1,d=0,e=4,f=4,g=4)
    norms = {s:24*maximum*(d+1) for s,d in degrees.items()}
    norms['c'] = 24

    def word_bound(word):
        norm,denominator,degree = 1,1,1
        for s in word[::-1]:
            norm = norms[s]*max(norm,denominator)**degrees[s]
            denominator = 24*denominator**degrees[s]
            degree *= degrees[s]
        return norm,denominator,degree

    upper = 1
    for left,right in rules:
        a,ad,ap = word_bound(left)
        b,bd,bp = word_bound(right)
        degree = max(ap,bp)
        # A k-th finite-difference weight is bounded by
        # 2^k*(tail+degree+1)^degree, for every k<=degree.
        upper = max(upper,(a*bd+b*ad)*2**degree*(tail+degree+1)**degree)
    return upper
