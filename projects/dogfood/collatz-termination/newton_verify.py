"""Independent exact checking by direct binomial sums (no SMT dependency)."""

from math import comb

from verify import RULES, require


def evaluate(polynomial, x):
    return sum(c*comb(x,i) for i,c in enumerate(polynomial))


def word_value(word, row, x):
    for symbol in word if row['reverse'] else word[::-1]:
        x = evaluate(row['polynomials'][ord(symbol)-ord('a')],x)
    return x


def tail_coefficients(values):
    # Direct closed formula, independent of RAD's triangular differences.
    return [sum((-1)**(k-j)*comb(k,j)*values[j] for j in range(k+1))
            for k in range(len(values))]


def check_newton(row):
    degree, maximum, tail, omitted = (row[k] for k in ('degree','maximum','tail','omitted'))
    require(type(degree) is int and 1 <= degree <= 4,'Newton degree domain')
    require(type(maximum) is int and 1 <= maximum <= 15,'Newton coefficient bound')
    require(type(tail) is int and 0 <= tail <= 32,'Newton tail origin')
    require(type(omitted) is int and -1 <= omitted < 11,'Newton omitted rule domain')
    polynomials = row['polynomials']
    require(len(polynomials) == 7 and all(len(p) == degree+1 for p in polynomials),'Newton polynomial shape')
    require(all(type(c) is int and -maximum <= c <= maximum for p in polynomials for c in p),'Newton coefficient domain')
    for p in polynomials:
        require(p[0] >= 0,'negative Newton origin value')
        differences = [evaluate(p[1:],x) for x in range(tail+degree)]
        require(all(v >= 0 for v in differences[:tail]+tail_coefficients(differences[tail:])),
                'Newton letter monotonicity is unproved')
    boundary = [i for i in ([0,1] if row['reverse'] else [8,9,10]) if i != omitted]
    removed, comparisons, active = [], 0, 0
    for rule,(left,right) in enumerate(RULES):
        if rule == omitted:
            continue
        active += 1
        bound = degree ** max(len(left),len(right))
        values = [word_value(left,row,x)-word_value(right,row,x) for x in range(tail+bound+1)]
        certificate = values[:tail]+tail_coefficients(values[tail:])
        require(all(c >= 0 for c in certificate),f'negative Newton certificate at rule {rule}')
        comparisons += len(certificate)
        if rule in boundary and all(v > 0 for v in values[:tail+1]):
            removed.append(rule)
    require(removed and removed == row['strict_rules'],'Newton strict-rule claim differs')
    complete = omitted == -1 and removed == boundary
    require(complete == row['complete_proof'],'false Newton completeness claim')
    return dict(label=row['label'],active_rules=active,coefficient_comparisons=comparisons,
                strict_rules=removed,complete_proof=complete)


def newton_control(degree=2,tail=0):
    # x, x+p, binomial(x,p), binomial(x,p), 3*x+p, x+p, x+p.
    # The missing af -> eb rule fails. The cf rule drops by at least one.
    def vector(*entries):
        return list(entries)+[0]*(degree+1-len(entries))
    choose = [0]*degree+[1]
    return dict(label=f'newton-control-p{degree}-h{tail}',status='sat',semiring='newton',
                degree=degree,maximum=max(3,degree),tail=tail,reverse=False,omitted=3,
                strict_rules=[9],complete_proof=False,
                polynomials=[vector(0,1),vector(degree,1),choose,choose,
                             vector(degree,3),vector(degree,1),vector(degree,1)])


def signed_control():
    row = newton_control(3,2)
    row.update(label='newton-signed-nonconvex-control',maximum=7)
    row['polynomials'][2] = [0,7,-6,6]
    row['polynomials'][3] = [0,7,-6,6]
    return row
