"""Optional exact symbolic check; requires SymPy. No PDE solution asserted."""
import sympy as s

x, y, z = s.symbols("x y z")
variables = (x, y, z)
position = s.Matrix(variables)
matrix = s.diag(1, 2, -3)
radius_squared = position.dot(position)


def curl(field):
    return s.Matrix([
        s.diff(field[2], y)-s.diff(field[1], z),
        s.diff(field[0], z)-s.diff(field[2], x),
        s.diff(field[1], x)-s.diff(field[0], y),
    ])


def equal(actual, expected):
    if s.simplify(actual-expected) != s.zeros(3, 1):
        raise ValueError(f"symbolic identity failed: {actual} versus {expected}")


potential = -position.cross(matrix*position)/3
velocity = s.simplify(curl((1-radius_squared)**2*potential))
divergence = sum(s.diff(velocity[i], variable) for i, variable in enumerate(variables))
if s.simplify(divergence) != 0:
    raise ValueError("solenoidal cutoff failed")
vorticity = curl(velocity)
diffusion = vorticity.applyfunc(lambda entry: sum(s.diff(entry,v,2) for v in variables))
equal(diffusion, 168*position.cross(matrix*position))
beta = s.Rational(4, 9)
inviscid = (1-beta)*velocity + beta*velocity.jacobian(position)*position + velocity.jacobian(position)*velocity
point = {x: s.Rational(1,2), y: s.Rational(1,2), z: 0}
equal(curl(inviscid).subs(point), s.Matrix([0,0,s.Rational(191,216)]))
equal(diffusion.subs(point), s.Matrix([0,0,42]))
print("Exact solenoidal identity, collar diffusion polynomial, and residual coefficients verified.")
