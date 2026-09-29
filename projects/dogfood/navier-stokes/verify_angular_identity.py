"""Optional symbolic check with an arbitrary radial cutoff function (SymPy)."""
import sympy as s

x, y, z, radius = s.symbols("x y z q", positive=True)
variables = (x, y, z)
position = s.Matrix(variables)
A = s.diag(1, 2, -3)
q = position.dot(position)
Q = position.dot(A*position)
H = position.cross(A*position)
chi = s.Function("chi")(radius)
a = chi + s.Rational(2, 3)*radius*s.diff(chi, radius)
b = -s.Rational(2, 3)*s.diff(chi, radius)
c = 2*(s.diff(a, radius)-b)
d = 2*s.diff(c, radius)*(a+b*radius)+b*c
V = (a.subs(radius, q)*A*position+b.subs(radius, q)*Q*position)


def curl(field):
    return s.Matrix([s.diff(field[2], y)-s.diff(field[1], z),
                     s.diff(field[0], z)-s.diff(field[2], x),
                     s.diff(field[1], x)-s.diff(field[0], y)])


def zero(expression):
    for entry in expression:
        if s.simplify(entry) != 0:
            raise ValueError("general radial identity failed")


zero([sum(s.diff(V[i], variables[i]) for i in range(3))])
zero(curl(V)-c.subs(radius, q)*H)
zero(curl(V.jacobian(position)*V)
     -(d.subs(radius, q)*Q*H-2*(a*c).subs(radius, q)*A*H))
omega = c.subs(radius, q)*H
zero(omega.applyfunc(lambda entry: sum(s.diff(entry, v, 2) for v in variables))
     -(4*radius*s.diff(c, radius, 2)+14*s.diff(c, radius)).subs(radius, q)*H)
print("Arbitrary radial-function divergence, curl, nonlinear angular identity and diffusion identity verified symbolically.")
print("Integration and time-dependent PDE interpretation remain outside this algebra check.")
