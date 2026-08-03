# RFC-0002 movement dogfood

This project exercises the complete Causal Laws transaction:

```text
Velocity + Wind + Knockback
    -> Displacement proposals
    -> one Displacement resolver
    -> complete candidate Position
    -> WorldBounds + NonPenetration validation
    -> atomic commit or structured rejection
```

Run the accepted path with:

```text
rad projects/dogfood/causal-constraints/main.rad --experimental-laws
```

`rejected.rad` stages a position occupied by a solid and demonstrates the
canonical, atomic rejection path.
