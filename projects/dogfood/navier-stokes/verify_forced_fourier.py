"""Compare the RAD full-force operator with independent exact complex algebra."""
import json
from pathlib import Path
import random
import subprocess
import tempfile
import sympy as s

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
RAD=str(ROOT/'target/debug/rad.exe')

def bank(seed):
    rng=random.Random(seed)
    waves=[((1,0,0),(0,1,1)),((0,1,0),(1,0,1)),
           ((0,0,1),(1,1,0)),((1,1,0),(1,-1,1))]
    result=[]
    for k,p in waves:
        a,b=rng.randint(-3,3),rng.randint(-3,3)
        for polarity in [1,-1]:
            result.append(dict(k=[polarity*x for x in k],
                re=[a*x for x in p],im=[polarity*b*x for x in p]))
    return result

def literal(modes):
    return '['+','.join('FluidMode { k: '+json.dumps(m['k'])+', re: '+json.dumps(m['re'])+
                       ', im: '+json.dumps(m['im'])+' }' for m in modes)+']'

def expected(u,ut,nu,scale=1):
    convert=lambda modes:{tuple(m['k']):s.Matrix([s.Rational(a,2)+s.I*s.Rational(b,2)
        for a,b in zip(m['re'],m['im'])]) for m in modes}
    u={k:v/scale for k,v in convert(u).items()}
    ut={k:v/scale for k,v in convert(ut).items()}
    zero=s.zeros(3,1)
    conv={}
    for p,up in u.items():
        for q,uq in u.items():
            k=tuple(p[j]+q[j] for j in range(3))
            conv[k]=conv.get(k,zero)+s.I*up.dot(s.Matrix(q))*uq
    result={}
    for k in set(u)|set(ut)|set(conv):
        n=sum(x*x for x in k)
        wave=s.Matrix(k)
        adv=conv.get(k,zero)
        result[k]=(ut.get(k,zero)+adv-(wave*wave.dot(adv)/n if n else zero)
                   +nu*n*u.get(k,zero)).applyfunc(s.expand)
    return result

def main():
    cases=[(bank(i),bank(i+20),1+i%3,1) for i in range(8)]
    # Exercise an ut-only frequency and a nonzero real mean.
    cases.append(([],[dict(k=[0,0,0],re=[2,4,6],im=[0,0,0])],1,1))
    cases += [(bank(9),bank(10),2,scale) for scale in [2,8,64,240,480,512]]
    wide=[]
    for axis in range(3):
        for polarity in [-1,1]:
            k=[0,0,0]; k[axis]=polarity*8
            coefficient=[0,0,0]; coefficient[(axis+1)%3]=8192
            wide.append(dict(k=k,re=coefficient,im=[0,0,0]))
    cases.append((wide,[dict(k=[0,0,0],re=[8192,8192,8192],im=[0,0,0])],16,512))
    source=['use "../forced_fourier.rad"','fn main() -> nil {']
    for i,(u,ut,nu,scale) in enumerate(cases):
        call=f'actual_force({literal(u)},{literal(ut)},{nu})' if scale==1 else f'actual_force_scaled({literal(u)},{literal(ut)},{nu},{scale})'
        source += [f'for mode in {call} {{',
            f'let row: map<str, any> = {{ "case": {i}, "mode": mode }}',
            'print(json_stringify(row))','}']
    source.append('}')
    with tempfile.TemporaryDirectory(prefix='force-check-',dir=HERE) as temporary:
        path=Path(temporary)/'check.rad'
        path.write_text('\n'.join(source)+'\n')
        command=[RAD,str(path),'--strict-types','--deny-warnings']
        run=subprocess.run(command,capture_output=True,text=True)
        assert run.returncode==0,run.stdout+run.stderr
        rows=[json.loads(line) for line in run.stdout.splitlines() if line.startswith('{')]
        actual=[{} for _ in cases]
        for row in rows:
            mode=row['mode']; k=tuple(mode['k'])
            assert k not in actual[row['case']], 'duplicate output frequency'
            actual[row['case']][k]=s.Matrix([s.Rational(a,mode['denominator'])+
                s.I*s.Rational(b,mode['denominator']) for a,b in zip(mode['re'],mode['im'])])
        for i,case in enumerate(cases):
            reference=expected(*case)
            assert actual[i].keys()==reference.keys(), 'incomplete generated-frequency coverage'
            for k,v in reference.items():
                assert all(s.simplify(x)==0 for x in actual[i][k]-v),(i,k)
        invalid=[([dict(k=[1,0,0],re=[1,0,0],im=[0,0,0])],1,'Fourier divergence'),
                 (bank(0)+bank(0),1,'duplicate Fourier mode'),
                 (bank(0)[:-1],1,'missing conjugate Fourier mode'),
                 (bank(0),0,'positive bounded viscosity')]
        for u,nu,message in invalid:
            path.write_text('use "../forced_fourier.rad"\nfn main() -> nil {\n'
                f'let result: list<ForceMode> = actual_force({literal(u)},[],{nu})\n'
                'print(len(result))\n}\n')
            rejected=subprocess.run(command,capture_output=True,text=True)
            assert rejected.returncode!=0 and message in rejected.stdout+rejected.stderr,message
        path.write_text('use "../forced_fourier.rad"\nfn main() -> nil {\n'
            f'let result: list<ForceMode> = actual_force_scaled({literal(bank(0))},[],1,513)\n'
            'print(len(result))\n}\n')
        rejected=subprocess.run(command,capture_output=True,text=True)
        assert rejected.returncode!=0 and 'Fourier rational scale domain' in rejected.stdout+rejected.stderr
        too_wide=[dict(k=[0,0,0],re=[8193,0,0],im=[0,0,0])]
        path.write_text('use "../forced_fourier.rad"\nfn main() -> nil {\n'
            f'let result: list<ForceMode> = actual_force({literal(too_wide)},[],1)\n'
            'print(len(result))\n}\n')
        rejected=subprocess.run(command,capture_output=True,text=True)
        assert rejected.returncode!=0 and 'Fourier input coefficient bound' in rejected.stdout+rejected.stderr
    coefficient=8192
    convolution_bound=48*48*48*coefficient**2
    component_bound=1536*convolution_bound+2*16*768**2*512*coefficient+2*768*512*coefficient
    assert 48*component_bound < 600000000000000000 < 2**63
    print(f'PASS: {len(cases)} full-force cases, {len(rows)} Fourier outputs, six invalid inputs rejected; expanded grid int64 bound checked.')
    print('Finite instantaneous residual operator only; no repeatable amplification stage certified.')

if __name__=='__main__':
    main()
