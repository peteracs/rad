"""Derive receiver-shape polynomials from the full Fourier operator for RAD search."""
from pathlib import Path
import json
import math
import sympy as s
from verify_coupled_modes import nonlinear,pair

HERE = Path(__file__).resolve().parent

def main():
    symbols = s.symbols('a b c d e',real=True)
    waves = [(1,0,0),(0,1,0),(1,1,0),(0,0,1),(0,1,1)]
    polar = [(0,1,1),(1,0,1),(1,-1,1),(1,1,0),(1,1,-1)]
    donor = {tuple(sign*x for x in k):(symbols[i]/2 if i not in [2,4] else -sign*symbols[i]*s.I/2)*s.Matrix(p)
             for i,(k,p) in enumerate(zip(waves,polar)) for sign in [-1,1]}
    nd = nonlinear(donor)
    receiver = {k:2*x for k,x in nd.items() if sum(t*t for t in k)>2 and x!=s.zeros(3,1)}
    nr = nonlinear(receiver)
    production = s.expand(sum(sum(t*t for t in k)*s.re(s.conjugate(x).dot(nr.get(k,s.zeros(3,1)))) for k,x in receiver.items()))
    loss = s.expand(sum(sum(t*t for t in k)**2*s.conjugate(x).dot(x) for k,x in receiver.items()))
    norm = s.expand(pair(receiver,receiver))
    donor_production = s.expand(sum(sum(t*t for t in k)*s.re(s.conjugate(x).dot(nd.get(k,s.zeros(3,1)))) for k,x in donor.items()))
    expressions = dict(production=production,loss=loss,norm=norm,donor_production=donor_production)
    data = {}
    rad = ['// Generated from complete symbolic Fourier convolution by build_receiver_calibration.py.',
           '// Inputs a..e are normalized integer amplitudes; no PDE return map is assumed.']
    for name,expr in expressions.items():
        poly = s.Poly(expr,*symbols)
        denominator = s.ilcm(*[c.q for _,c in poly.terms()])
        terms = [(list(powers),int(coefficient*denominator)) for powers,coefficient in poly.terms()]
        data[name] = dict(denominator=int(denominator),terms=terms,degree=int(poly.total_degree()))
        rad += [f'pub pure fn receiver_{name}_n(x: list<int>) -> int {{',
                '    assert(len(x) == 5,"receiver parameter dimension")',
                '    for value in x { assert(value >= 1 and value <= 8,"receiver parameter box") }',
                '    let mut result: int = 0']
        for powers,coefficient in terms:
            factors = [str(coefficient)]+[f'x[{j}]' for j,power in enumerate(powers) for _ in range(power)]
            rad += ['    result = result+'+'*'.join(factors)]
        rad += ['    return result','}']
    (HERE/'receiver_polynomials.rad').write_text('\n'.join(rad)+'\n',encoding='utf-8')
    (HERE/'receiver_polynomials.json').write_text(json.dumps(data,indent=2)+'\n',encoding='utf-8')
    print({name:dict(terms=len(info['terms']),degree=info['degree'],denominator=info['denominator']) for name,info in data.items()})
    print('receiver production:',production)

if __name__ == '__main__':
    main()
