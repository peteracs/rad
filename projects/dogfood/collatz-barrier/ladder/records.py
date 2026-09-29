"""Exploratory float64 scan (NOT a certificate): strict records of E_q = F_q(0) - q/(2 ln 2) and their upper hull.

Usage: python records.py 1000000000
"""
# Exploratory float64: strict prefix records of E_q=F_q(0)-qI, then upper hull of records.
import numpy as np, math, sys
from decimal import Decimal, getcontext
getcontext().prec=80
alpha=Decimal(3).ln()/Decimal(2).ln()
A=np.uint64(int((alpha-1)*(Decimal(2)**64)))
N=int(sys.argv[1]); CH=10_000_000
I=1/(2*math.log(2))
base=0.0; start=np.uint64(0); best=0.0; recs=[(0,0.0)]; q0=0
while q0<N:
    n=min(CH,N-q0)
    with np.errstate(over='ignore'):
        fr=start+A*np.arange(n,dtype=np.uint64); start=start+A*np.uint64(n)
    E=base+np.cumsum(np.exp2(-(fr.astype(np.float64)/2.0**64))-I)
    run=np.maximum.accumulate(np.maximum(E,best))
    idx=np.nonzero((E>best)&(E>=run))[0]
    # strict new records inside chunk
    prev=best
    for i in idx:
        if E[i]>prev: recs.append((q0+1+int(i),float(E[i]))); prev=E[i]
    best=max(best,float(E.max())); base=float(E[-1]); q0+=n
hull=[]
for p in recs:
    while len(hull)>=2:
        (x1,y1),(x2,y2)=hull[-2],hull[-1]
        if (y2-y1)*(p[0]-x1) <= (p[1]-y1)*(x2-x1): hull.pop()
        else: break
    hull.append(p)
qs=[1,1,2,5,12,41,53,306,665,15601,31867,79335,111202,190537,10590737,10781274,53715833,64497107,118212940,182710047,300922987]
print("records:",len(recs))
for i,(q,e) in enumerate(hull):
    g=q-hull[i-1][0] if i else 0
    print(q,g,round(e,6), "gap/conv:", [ (c,g//c) for c in qs[2:] if g and g%c==0][-1:] )
