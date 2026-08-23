// Differential contracts for the compact sparse-support representation.
//
// These tests deliberately compare the optimized envelope/position machinery
// with the independent full affine-state and materialized-bigint models.

#[test]
fn envelope_search_matches_full_affine_state_search() {
    let full = sparse_support_profile(3, 1, 128, 20, 5).unwrap();
    let compact = sparse_support_summary(3, 1, 128, 20, 5).unwrap();
    assert_eq!(
        compact.deepest_survival_by_weight,
        full.deepest_survival_by_weight
    );
    assert_eq!(
        compact.deepest_witness_by_weight,
        full.deepest_witness_by_weight
    );
}

#[test]
fn sparse_position_comparison_matches_materialized_biguint() {
    for positions in [
        vec![],
        vec![0],
        vec![0, 1, 39],
        vec![0, 1, 3, 4, 45, 49],
        vec![0, 1, 2, 6, 39, 47, 98],
    ] {
        let residue = support_positions_value(&positions);
        for delta in [0u32, 1, 17, 257] {
            let delta = BigUint::from(delta);
            let below = if residue >= delta {
                &residue - &delta
            } else {
                BigUint::from(0u8)
            };
            let above = &residue + &delta;
            assert_eq!(value_less_than_positions(&below, &positions), below < residue);
            assert_eq!(value_less_than_positions(&above, &positions), above < residue);
        }
    }
}
