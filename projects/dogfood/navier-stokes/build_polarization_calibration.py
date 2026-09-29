"""Exact Fourier-derived two-polarization search model; variable p=x/4, q=y/4."""
from pathlib import Path
import json
import sympy as s
from verify_coupled_modes import nonlinear,pair

HERE = Path(__file__).resolve().parent

def main():
    p,q = s.symbols('p q',real=True)
    waves = [(1,0,0),(0,1,0),(1,1,0),(0,0,1),(0,1,1)]
    polar = [(0,1,1),(1,0,1),(1,-1,p),(1,1,0),(q,1,-1)]
    donor = {tuple(sign*x for x in k):(s.Rational(1,2) if i not in [2,4] else -sign*s.I/2)*s.Matrix(pol)
             for i,(k,pol) in enumerate(zip(waves,polar)) for sign in [-1,1]}
    nd = nonlinear(donor)
    receiver = {k:2*x for k,x in nd.items() if sum(t*t for t in k)>2 and x!=s.zeros(3,1)}
    nr = nonlinear(receiver)
    def production(u,nu):
        return s.expand(sum(sum(t*t for t in k)*s.re(s.conjugate(x).dot(nu.get(k,s.zeros(3,1)))) for k,x in u.items()))
    def norm(u,order):
        return s.expand(sum((1+sum(t*t for t in k))**order*s.conjugate(x).dot(x) for k,x in u.items()))
    expressions = dict(receiver_production=production(receiver,nr),
        receiver_loss=s.expand(sum(sum(t*t for t in k)**2*s.conjugate(x).dot(x) for k,x in receiver.items())),
        receiver_norm=norm(receiver,0),donor_production=production(donor,nd),
        donor_loss=s.expand(sum(sum(t*t for t in k)**2*s.conjugate(x).dot(x) for k,x in donor.items())),
        donor_h11=norm(donor,11),donor_h7=norm(donor,7),donor_norm=norm(donor,0))
    data = {}
    rad = ['// Generated from full Fourier convolution; parameters are quarter-integers in [-2,2].']
    for name,expr in expressions.items():
        scaled = s.Poly(s.expand(expr.subs({p:p/4,q:q/4})),p,q)
        denominator = int(s.ilcm(*[c.q for _,c in scaled.terms()]))
        terms = [(list(powers),int(coefficient*denominator)) for powers,coefficient in scaled.terms()]
        data[name] = dict(denominator=denominator,terms=terms)
        rad += [f'pub pure fn polarization_{name}(x: int,y: int) -> int {{',
                '    assert(x >= -8 and x <= 8 and y >= -8 and y <= 8,"polarization box")',
                '    let mut result: int = 0']
        for powers,coefficient in terms:
            rad += ['    result = result+'+'*'.join([str(coefficient)]+['x']*powers[0]+['y']*powers[1])]
        rad += ['    return result','}']
    (HERE/'polarization_polynomials.rad').write_text('\n'.join(rad)+'\n',encoding='utf-8')
    (HERE/'polarization_polynomials.json').write_text(json.dumps(data,indent=2)+'\n',encoding='utf-8')
    positive = [(x,y) for x in range(-8,9) for y in range(-8,9)
                if expressions['receiver_production'].subs({p:s.Rational(x,4),q:s.Rational(y,4)})>0
                and expressions['donor_production'].subs({p:s.Rational(x,4),q:s.Rational(y,4)})>0]
    print({name:dict(terms=len(info['terms']),denominator=info['denominator']) for name,info in data.items()})
    print('positive donor and receiver:',len(positive),'of 289')
    print('first examples:',positive[:5])

if __name__ == '__main__':
    main()
