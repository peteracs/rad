"""Unbounded Python-integer checking, independently of the SMT encoding."""

RULES = [('ad', 'd'), ('bd', 'gd'), ('ae', 'ea'), ('af', 'eb'),
         ('ag', 'fa'), ('be', 'fb'), ('bf', 'ga'), ('bg', 'gb'),
         ('ce', 'cb'), ('cf', 'caa'), ('cg', 'cab')]
INF = -1000000


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check(row):
    d, maximum, omitted = row['dimension'], row['maximum'], row['omitted']
    require(row['semiring'] in ('natural', 'arctic'), 'unknown semiring')
    arctic = row['semiring'] == 'arctic'
    denominator = row['denominator']
    require(type(denominator) is int and 1 <= denominator <= 31 and (not arctic or denominator == 1), 'denominator domain')
    require(type(d) is int and 1 <= d <= 16 and type(maximum) is int and 1 <= maximum <= 31, 'matrix domain')
    require(type(omitted) is int and -1 <= omitted < 11, 'omitted rule domain')
    matrices = row['matrices']
    require(len(matrices) == 7, 'alphabet incomplete')
    zero, one = (INF, 0) if arctic else (0, 1)
    for a in matrices:
        require(len(a) == d+1 and all(len(v) == d+1 for v in a), 'matrix shape')
        for i in range(d+1):
            for j in range(d+1):
                v = a[i][j]
                require(type(v) is int, 'noninteger coefficient')
                if i == d:
                    require(v == ((one if arctic else denominator) if j == d else zero), 'homogeneous coordinate')
                else:
                    require((v == INF or -maximum <= v <= maximum) if arctic else 0 <= v <= maximum, 'coefficient domain')
        if arctic:
            require(a[0][0] >= 0 or a[0][d] >= 0, 'carrier not closed')

    def word(text):
        result = [[one if i == j else zero for j in range(d+1)] for i in range(d+1)]
        for symbol in text[::-1] if row['reverse'] else text:
            a = matrices[ord(symbol)-ord('a')]
            after = [[zero]*(d+1) for _ in range(d+1)]
            for i in range(d+1):
                for j in range(d+1):
                    if arctic:
                        terms = [result[i][k]+a[k][j] for k in range(d+1) if result[i][k] != INF and a[k][j] != INF]
                        after[i][j] = max(terms, default=INF)
                    else:
                        after[i][j] = sum(result[i][k]*a[k][j] for k in range(d+1))
            result = after
        return result

    boundary = [i for i in ([0, 1] if row['reverse'] else [8, 9, 10]) if i != omitted]
    removed, checks, active = [], 0, 0
    for number, (left, right) in enumerate(RULES):
        if number == omitted:
            continue
        active += 1
        a, b = word(left), word(right)
        la, rb = (1, 1) if arctic else (denominator ** len(right), denominator ** len(left))
        for i in range(d):
            for j in range(d+1):
                require(a[i][j] * la >= b[i][j] * rb, f'rule {number} is not weakly decreasing')
                checks += 1
        if number in boundary:
            strict = (all(a[0][j] > b[0][j] or a[0][j] == b[0][j] == INF for j in range(d+1))
                      if arctic else a[0][d] * la > b[0][d] * rb)
            if strict:
                removed.append(number)
    require(removed and removed == row['strict_rules'], 'strict-rule claims differ')
    complete = omitted == -1 and removed == boundary
    require(complete == row['complete_proof'], 'unproved completeness claim')
    return dict(label=row['label'], active_rules=active, coefficient_comparisons=checks,
                strict_rules=removed, complete_proof=complete)


def known_natural_control():
    # Example 4.3 of Yolcu--Aaronson--Heule, checked as a reduced system.
    affine = [([[1,1],[1,0]],[0,0]), ([[1,3],[3,4]],[1,1]),
              ([[1,5],[0,0]],[0,0]), ([[1,0],[1,0]],[1,1]),
              ([[7,2],[2,5]],[2,1]), ([[2,1],[1,1]],[1,0]),
              ([[2,2],[2,4]],[0,2])]
    matrices = [[a[0]+[b[0]], a[1]+[b[1]], [0,0,1]] for a,b in affine]
    return dict(label='published-positive-control', status='sat', semiring='natural',
                dimension=2, maximum=7, denominator=1, reverse=False, omitted=3, matrices=matrices,
                strict_rules=[8,9,10], complete_proof=False)


def arctic_control():
    affine = [(0,-1),(0,-2),(-1,0),(INF,2),(2,2),(2,2),(2,2)]
    return dict(label='arctic-positive-control',status='sat',semiring='arctic',dimension=1,
                maximum=2,denominator=1,reverse=False,omitted=1,
                matrices=[[[a,b],[INF,0]] for a,b in affine],strict_rules=[8,9,10],complete_proof=False)


def polynomial_control():
    return dict(label='nonlinear-positive-control',status='sat',semiring='polynomial',degree=2,
                maximum=3,reverse=False,omitted=3,strict_rules=[9],complete_proof=False,
                polynomials=[[0,1,0],[3,1,0],[0,0,1],[0,0,1],[3,3,0],[3,1,0],[3,1,0]])


def check_polynomial(row):
    degree, maximum, omitted = row['degree'], row['maximum'], row['omitted']
    require(type(degree) is int and 1 <= degree <= 4 and type(maximum) is int and 1 <= maximum <= 7,
            'polynomial domain')
    require(degree < 4 or maximum <= 2, 'RAD polynomial arithmetic could overflow')
    require(type(omitted) is int and -1 <= omitted < 11, 'omitted rule domain')
    polynomials = row['polynomials']
    require(len(polynomials) == 7 and all(len(p) == degree+1 for p in polynomials), 'polynomial shape')
    require(all(type(c) is int and 0 <= c <= maximum for p in polynomials for c in p), 'coefficient domain')

    def product(a,b):
        result={}
        for i,x in a.items():
            for j,y in b.items():
                result[i+j]=result.get(i+j,0)+x*y
        return result

    def interpret(word):
        result={1:1}
        for symbol in word[::-1] if row['reverse'] else word:
            p=dict(enumerate(polynomials[ord(symbol)-ord('a')]))
            after,power={}, {0:1}
            for i in range(max(result)+1):
                for j,c in power.items():
                    after[j]=after.get(j,0)+result.get(i,0)*c
                power=product(power,p)
            result=after
        return result

    boundary=[i for i in ([0,1] if row['reverse'] else [8,9,10]) if i != omitted]
    removed, comparisons, active=[],0,0
    for rule,(left,right) in enumerate(RULES):
        if rule == omitted: continue
        active+=1
        a,b=interpret(left),interpret(right)
        for i in range(max(max(a),max(b))+1):
            require(a.get(i,0)>=b.get(i,0),f'negative polynomial coefficient for rule {rule}')
            comparisons+=1
        if rule in boundary and a.get(0,0)>b.get(0,0): removed.append(rule)
    require(removed and removed==row['strict_rules'],'incorrect polynomial strict rules')
    complete=omitted==-1 and removed==boundary
    require(complete==row['complete_proof'],'false polynomial completeness')
    return dict(label=row['label'],active_rules=active,coefficient_comparisons=comparisons,
                strict_rules=removed,complete_proof=complete)
