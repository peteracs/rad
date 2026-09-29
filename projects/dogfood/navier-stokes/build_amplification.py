"""Exact full Fourier derivative of a 3D periodic six-mode initial field."""
import json
from pathlib import Path
import sympy as s


def calculate():
    amplitudes = s.symbols('a b c d e f', real=True)
    viscosity = s.symbols('nu', positive=True)
    waves = [(1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (0, 1, 1), (0, 1, 2)]
    polarizations = [(0, 1, 1), (1, 0, 1), (1, -1, 1), (1, 1, 0), (1, 1, -1), (0, -2, 1)]
    modes = {}
    for index, (wave, polarization, amplitude) in enumerate(zip(waves, polarizations, amplitudes)):
        factor = -s.I/2 if index in [2, 4] else s.Rational(1, 2)
        modes[wave] = factor*amplitude*s.Matrix(polarization)
        modes[tuple(-v for v in wave)] = s.conjugate(modes[wave])
    nonlinear = {}
    for p, up in modes.items():
        for q, uq in modes.items():
            k = tuple(p[i]+q[i] for i in range(3))
            nonlinear[k] = nonlinear.get(k, s.zeros(3, 1))+s.I*up.dot(s.Matrix(q))*uq
    derivative = {}
    for k in set(modes) | set(nonlinear):
        norm = sum(v*v for v in k)
        raw = nonlinear.get(k, s.zeros(3, 1))
        if norm:
            wave = s.Matrix(k)
            projected = raw-wave*(wave.dot(raw)/norm)
        else:
            projected = raw
        derivative[k] = (-projected-viscosity*norm*modes.get(k, s.zeros(3, 1))).applyfunc(s.expand)
    energy = sum(sum(v*v for v in k)*s.conjugate(u).dot(u)/2 for k, u in modes.items())
    rate = sum(sum(v*v for v in k)*s.re(s.conjugate(u).dot(derivative[k])) for k, u in modes.items())
    palinstrophy = sum(sum(v*v for v in k)**2*s.conjugate(u).dot(u) for k, u in modes.items())
    return amplitudes, viscosity, modes, derivative, s.expand(energy), s.expand(rate), s.expand(palinstrophy)


def main():
    amplitudes, nu, modes, derivative, enstrophy, rate, dissipation = calculate()
    print('enstrophy:', enstrophy)
    print('rate:', rate)
    print('dissipation:', dissipation)
    for k in sorted(derivative):
        if k not in modes and derivative[k] != s.zeros(3, 1) and k > (0, 0, 0):
            print('first generated mode:', k, list(derivative[k]))
            break
    receipt = dict(amplitudes=[str(a) for a in amplitudes],
                   enstrophy=str(enstrophy), rate=str(rate), dissipation=str(dissipation),
                   input_modes=len(modes), derivative_modes=sum(v != s.zeros(3, 1) for v in derivative.values()))
    Path(__file__).with_name('amplification_formula.json').write_text(json.dumps(receipt, indent=2)+'\n')


if __name__ == '__main__':
    main()
