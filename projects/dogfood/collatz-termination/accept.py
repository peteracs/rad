"""Audit saved searches and check every SAT model in Python and RAD."""

from collections import Counter
from copy import deepcopy
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from verify import require, check, check_polynomial, known_natural_control, arctic_control, polynomial_control
from newton_verify import check_newton, newton_control, signed_control

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
OUT=HERE/'out'
RAD=ROOT/'target/release'/('rad.exe' if os.name=='nt' else 'rad')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args,name,workers=1,success=True):
    result=subprocess.run([str(x) for x in args],cwd=ROOT,capture_output=True,timeout=60,
                          env={**os.environ,'RAYON_NUM_THREADS':str(workers)})
    (OUT/f'{name}.stdout').write_bytes(result.stdout)
    (OUT/f'{name}.stderr').write_bytes(result.stderr)
    require((result.returncode==0)==success,(result.stdout+result.stderr).decode('utf-8',errors='replace'))
    return result


def write(name,rows):
    path=OUT/name
    path.write_text(json.dumps(rows,indent=2)+'\n',encoding='utf-8',newline='\n')
    return path


def main():
    OUT.mkdir(exist_ok=True)
    searches=[]
    queries={}
    for path in sorted(OUT.glob('*.json')):
        row=json.loads(path.read_text(encoding='utf-8'))
        if not isinstance(row,dict) or 'query_sha256' not in row:
            continue
        query=OUT/f"{row['label']}.smt2"
        require(digest(query)==row['query_sha256'],'search query hash differs')
        queries[query.name]=digest(query)
        # Earlier queries predate rational/polynomial metadata. Their exact
        # saved formulas remain authoritative; no reexecution is claimed.
        row.setdefault('semiring','polynomial' if 'degree' in row else 'natural')
        row.setdefault('denominator',1)
        row.setdefault('omitted',-1)
        require(row['status'] in ('sat','unsat','unknown'),'unknown result category')
        if row['status']!='sat': require(not row['complete_proof'],'model absence cannot prove termination')
        searches.append(row)
    require(searches,'run a search before acceptance')
    scaled=known_natural_control()
    scaled.update(label='rational-rescaling-control',maximum=14,denominator=2)
    scaled['matrices']=[[[2*x for x in r] for r in a] for a in scaled['matrices']]
    controls=[known_natural_control(),arctic_control(),polynomial_control(),scaled,
              newton_control(2),newton_control(3,2),newton_control(4),signed_control()]
    rows=controls+searches
    expected=[(check_newton(row) if row['semiring']=='newton' else check_polynomial(row) if row['semiring']=='polynomial' else check(row))
              for row in rows if row['status']=='sat']
    summary=dict(checked_models=len(expected),unresolved_searches=sum(r['status']!='sat' for r in rows),
                 complete_proofs=sum(r['complete_proof'] for r in expected))
    source=write('checked-input.json',rows)
    for workers in (1,4):
        trace=OUT/f'checked-{workers}.radr'
        result=run([RAD,HERE/'verify.rad','--strict-types','--deny-warnings','--record',trace,'--',source],f'checked-{workers}',workers)
        decoded=[json.loads(line) for line in result.stdout.decode('utf-8').splitlines() if line.startswith('{')]
        require(decoded==expected+[summary],'RAD and Python model checks differ')
        replay=run([RAD,'replay',trace],f'replay-{workers}',5-workers)
        require(b'Replay verified: world digest matches' in replay.stdout+replay.stderr,'replay differs')
    mutants=[]
    row=known_natural_control();row['omitted']=-1;mutants.append(row)
    row=known_natural_control();row['complete_proof']=True;mutants.append(row)
    row=deepcopy(scaled);row['denominator']=1;mutants.append(row)
    row=arctic_control();row['matrices'][0][0]=[-1,-1];mutants.append(row)
    row=polynomial_control();row['omitted']=-1;mutants.append(row)
    row=polynomial_control();row['complete_proof']=True;mutants.append(row)
    row=newton_control();row['omitted']=-1;mutants.append(row)
    row=newton_control();row['complete_proof']=True;mutants.append(row)
    row=newton_control();row['tail']=-1;mutants.append(row)
    row=newton_control();row['polynomials'][0][1]=-1;mutants.append(row)
    row=dict(label='timeout-forged-as-proof',status='unknown',complete_proof=True);mutants.append(row)
    for i,row in enumerate(mutants):
        path=write(f'mutation-{i}.json',[row])
        run([RAD,HERE/'verify.rad','--strict-types','--deny-warnings','--',path],f'mutation-{i}',success=False)
    run([RAD,HERE/'test_newton.rad','--strict-types','--deny-warnings'],'newton-arithmetic')
    run([sys.executable,'-O','-m','unittest','discover','-s',HERE,'-p','test_*.py'],'tests')
    result=dict(format='rad-collatz-termination-search-v1',completed_utc=datetime.now(timezone.utc).isoformat(),
                reported_search_statuses=dict(Counter(row['status'] for row in searches)),
                full_system_searches=sum(row['omitted']==-1 for row in searches),
                checked=summary,rad_mutations_rejected=len(mutants),
                sources={p.name:digest(p) for p in sorted(HERE.iterdir()) if p.suffix in ('.py','.rad','.md')},
                imported_sources={'../collatz-barrier/natural.rad':digest(HERE/'../collatz-barrier/natural.rad')},
                queries=queries,binary_sha256=digest(RAD),
                artifacts={p.name:digest(p) for p in sorted(OUT.iterdir()) if p.is_file()},
                scope='SAT certificates are independently recomputed. UNSAT and UNKNOWN are solver reports for bounded templates. Saved query bytes record searches while encoder source evolved. No complete Collatz certificate has been found unless checked.complete_proofs is positive.')
    (HERE/'evidence.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8',newline='\n')
    print(json.dumps({k:result[k] for k in ('reported_search_statuses','full_system_searches','checked','rad_mutations_rejected')},indent=2))


if __name__=='__main__': main()
