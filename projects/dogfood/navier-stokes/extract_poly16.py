"""Extract the existing checked polynomial implementation without changing it."""
from pathlib import Path

here = Path(__file__).resolve().parent
path = here / 'principal_cancellation.rad'
source = path.read_text()
if not source.startswith('use "poly16.rad"'):
    algebra, body = source.split('component PrincipalEvidence', 1)
    algebra = algebra.replace('struct Term', 'pub struct Term').replace('pure fn ', 'pub pure fn ')
    (here / 'poly16.rad').write_text(algebra)
    path.write_text('use "poly16.rad"\n\ncomponent PrincipalEvidence' + body)
