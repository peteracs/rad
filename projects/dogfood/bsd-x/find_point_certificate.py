"""Search finite reduction certificates for two points on 389a1."""
def primes(stop):
    return [p for p in range(3, stop, 2) if all(p % d for d in range(2, int(p**0.5)+1))]

def points(p):
    return [None]+[(x,y) for x in range(p) for y in range(p)
                   if (y*y+y-x*x*x-x*x+2*x)%p==0]

def add(P,Q,p):
    if P is None: return Q
    if Q is None: return P
    x,y=P; u,v=Q
    if x==u and (y+v+1)%p==0: return None
    a=((v-y)*pow(u-x,-1,p))%p if x!=u else ((3*x*x+2*x-2)*pow(2*y+1,-1,p))%p
    z=(a*a-1-x-u)%p
    return z,(-y-1+a*(x-z))%p

if __name__=='__main__':
    for p in primes(250):
        pts=points(p)
        doubles={add(P,P,p) for P in pts}
        P,Q=(0,0),(1,0)
        if P not in doubles and Q not in doubles and add(P,Q,p) not in doubles:
            print(dict(prime=p,order=len(pts),doubles=len(doubles),sum=add(P,Q,p)))
            break
