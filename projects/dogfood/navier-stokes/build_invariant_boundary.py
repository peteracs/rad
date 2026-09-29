"""Full vector-field boundary derivatives; no truncation of generated modes."""
import json
from pathlib import Path
import sympy as s
from build_amplification import calculate


def boundary(shape):
    amps, nu, raw_u, raw_v, _, _, _ = calculate()
    subs = dict(zip(amps, shape[:6]))
    zero = s.zeros(3, 1)
    u = {k: v.subs(subs) for k, v in raw_u.items() if v.subs(subs) != zero}
    v = {k: x.subs(subs).applyfunc(s.expand) for k, x in raw_v.items()
         if x.subs(subs) != zero}
    if len(shape) == 7:
        u[(1,0,1)] = s.Matrix([0, -s.I*s.Rational(shape[6],2), 0])
        u[(-1,0,-1)] = s.conjugate(u[(1,0,1)])
        conv = {}
        for p, up in u.items():
            for q, uq in u.items():
                k = tuple(p[i]+q[i] for i in range(3))
                conv[k] = conv.get(k,zero) + s.I*up.dot(s.Matrix(q))*uq
        v = {}
        for k in set(conv) | set(u):
            n = sum(t*t for t in k)
            wave = s.Matrix(k)
            raw = conv.get(k,zero)
            value = (-raw + (wave*wave.dot(raw)/n if n else zero)
                     -nu*n*u.get(k,zero)).applyfunc(s.expand)
            if value != zero:
                v[k] = value
    # F'(u)F(u) = -P[(v.grad)u + (u.grad)v] + nu Delta v.
    nonlinear = {}
    for left, right in [(u, v), (v, u)]:
        for p, up in left.items():
            for q, uq in right.items():
                k = tuple(p[i] + q[i] for i in range(3))
                nonlinear[k] = nonlinear.get(k, zero) + s.I*up.dot(s.Matrix(q))*uq
    acceleration = {}
    for k in u:
        n = sum(x*x for x in k)
        wave = s.Matrix(k)
        raw = nonlinear.get(k, zero)
        acceleration[k] = (-raw + wave*wave.dot(raw)/n - nu*n*v[k]).applyfunc(s.expand)
    inner = lambda x, y: s.re(s.conjugate(x).dot(y))
    K = s.expand(sum(inner(x, x)/2 for x in u.values()))
    E = s.expand(sum(sum(t*t for t in k)*inner(x, x)/2 for k, x in u.items()))
    D = s.expand(sum(sum(t*t for t in k)**2*inner(x, x) for k, x in u.items()))
    R = s.expand(sum(sum(t*t for t in k)*inner(x, v[k]) for k, x in u.items()))
    Ddot = s.expand(2*sum(sum(t*t for t in k)**2*inner(x, v[k]) for k, x in u.items()))
    # Crucial: the squared-velocity term includes every generated frequency.
    Rdot = s.expand(sum(sum(t*t for t in k)*inner(x, x) for k, x in v.items())
                       + sum(sum(t*t for t in k)*inner(x, acceleration[k]) for k, x in u.items()))
    S = s.expand(R + nu*D)
    Sdot = s.expand(Rdot + nu*Ddot)
    boundary_nu = S/(2*D)
    inward = s.factor((Sdot - 2*nu*Ddot).subs(nu, boundary_nu))
    leaked_norm = s.expand(sum(sum(t*t for t in k)*inner(x, x) for k, x in v.items() if k not in u))
    qdot = s.expand((-2*nu*E*D + K*Ddot)/E**2 - 2*K*D*R/E**3)
    result = {key: str(value) for key, value in dict(K=K,E=E,D=D,R=R,
        Ddot=Ddot,S=S,Sdot=Sdot,Rdot=Rdot,boundary_nu=boundary_nu,
        inward=inward,shape_ratio=K*D/E**2,shape_rate=qdot,
        generated_enstrophy_acceleration=leaked_norm).items()}
    assert all(s.simplify(s.Matrix(k).dot(x)) == 0 for k, x in v.items())
    assert s.expand(sum(inner(x, v[k]) for k, x in u.items()) + 2*nu*E) == 0
    result['input_modes'] = len(u)
    result['derivative_modes'] = len(v)
    result['shape'] = shape
    return result


def main():
    shapes = [[1,1,1,1,1,f] for f in [-2,-1,0,1,2]]
    shapes += [[a,b,c,d,e,0] for a,b,c,d,e in
               [(1,1,4,1,1),(1,4,1,1,1),(4,1,1,1,1),
                (1,1,1,1,4),(1,1,1,4,1),(1,1,-1,1,2),
                (1,1,2,1,-1)]]
    shapes += [[1,1,1,1,1,0,g] for g in [-16,-4,-1,1,4,16]]
    results = [boundary(shape) for shape in shapes]
    Path(__file__).with_name('invariant_boundary.json').write_text(json.dumps(results, indent=2)+'\n')
    for row in results:
        print(row['shape'], 'nu=',row['boundary_nu'], 'inward=', row['inward'])


if __name__ == '__main__':
    main()
