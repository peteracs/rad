use super::*;
use num_bigint::BigUint;

const FREE: [i64; 1] = [0];

fn free(per_bit: usize) -> Relaxation<'static> {
    Relaxation {
        k: 0,
        per_bit,
        top_bits: 6,
        shift: 0,
        barriers: &FREE,
    }
}

fn two_node(weights: [i64; 2]) -> Potential {
    longest_potential(&[0, 1, 2], &[1, 0], &weights).unwrap()
}

#[test]
fn negative_and_zero_cycles_are_certified() {
    assert_eq!(two_node([-1, -2]).status, Status::Certified);
    let zero = two_node([5, -5]);
    assert_eq!(zero.status, Status::Certified);
    assert_eq!(zero.max_potential, 5);
}

#[test]
fn a_positive_cycle_returns_a_checked_witness() {
    let found = two_node([3, -1]);
    assert_eq!(found.status, Status::Witness);
    assert_eq!(found.witness.len(), 2);
    assert_eq!(found.witness_weight, 2);
}

#[test]
fn a_positive_self_loop_is_found_and_a_dead_end_is_certified() {
    let found = longest_potential(&[0, 1], &[0], &[1]).unwrap();
    assert_eq!((found.status, found.witness), (Status::Witness, vec![0]));
    let acyclic = longest_potential(&[0, 1, 1], &[1], &[7]).unwrap();
    assert_eq!(acyclic.status, Status::Certified);
    assert_eq!(acyclic.max_potential, 7);
}

#[test]
fn weights_that_could_overflow_are_refused() {
    assert!(longest_potential(&[0, 1], &[0], &[MAX_WEIGHT + 1]).is_err());
}

#[test]
fn the_odd_step_constant_is_exact() {
    let power = |base: u32, exponent: u32| BigUint::from(base).pow(exponent);
    let three = power(3, MAX_PER_BIT as u32);
    let bits = |q13: usize| MAX_PER_BIT as u32 + q13 as u32;
    assert!(power(2, bits(ODD_STEP_Q13)) <= three);
    assert!(three < power(2, bits(ODD_STEP_Q13 + 1)));
}

#[test]
fn payoffs_bound_the_exact_power_from_above_and_stay_tight() {
    let per_bit = 8192;
    let bins = per_bit * 16;
    let table = payoff_table(per_bit, bins);
    for i in [0, 1, per_bit / 2, per_bit, bins / 3, bins - 1] {
        // payoff^per_bit * 2^i >= (2^30)^per_bit, checked exactly.
        let above = BigUint::from((table[i] - SLACK) as u64).pow(per_bit as u32) << i;
        assert!(above >= BigUint::from(1u32) << (30 * per_bit));
        let exact = ONE as f64 * (-(i as f64) / per_bit as f64).exp2();
        assert!((table[i] as f64 - exact).abs() <= (SLACK + 2) as f64);
    }
}

#[test]
fn ceiling_and_certificate_agree() {
    let relaxation = free(64);
    let found = ceiling(&relaxation).unwrap();
    assert_eq!(found.ceiling * 1000 / ONE, 752);
    assert!(found.cycle_odd > 0);
    let above = certify(&relaxation, found.ceiling + 2000).unwrap();
    assert_eq!(above.potential.status, Status::Certified);
    let below = certify(&relaxation, found.ceiling - 2_000_000).unwrap();
    assert_eq!(below.potential.status, Status::Witness);
    assert!(below.potential.witness_weight > 0);
}

#[test]
fn policy_iteration_does_not_chase_float_noise() {
    // Tolerances below the noise of weights near 2^30 never converge here.
    let found = ceiling(&free(8192)).unwrap();
    assert!(found.policy_rounds < 400, "{}", found.policy_rounds);
}

#[test]
fn barriers_only_remove_closed_walks() {
    let barriers = [0, ONE / 2, -ONE];
    let barred = Relaxation {
        k: 1,
        barriers: &barriers,
        ..free(16)
    };
    let unbarred = Relaxation {
        barriers: &[0, 0, 0],
        ..barred
    };
    let with = ceiling(&barred).unwrap();
    assert!(with.ceiling <= ceiling(&unbarred).unwrap().ceiling);
    assert!(with.live < barred.top_bits * barred.per_bit * 3);
}

#[test]
fn a_graph_with_no_live_node_has_no_ceiling_and_is_certified() {
    let barriers = [100 * ONE];
    let empty = Relaxation {
        barriers: &barriers,
        ..free(16)
    };
    let found = ceiling(&empty).unwrap();
    assert_eq!((found.ceiling, found.live), (0, 0));
    assert_eq!(
        certify(&empty, 0).unwrap().potential.status,
        Status::Certified
    );
}

#[test]
fn invalid_parameters_are_refused() {
    let wrong_count = Relaxation { k: 1, ..free(64) };
    assert!(ceiling(&wrong_count).unwrap_err().contains("expected 3"));
    assert!(ceiling(&free(48)).unwrap_err().contains("power of two"));
    let too_deep = Relaxation { k: 13, ..free(64) };
    assert!(ceiling(&too_deep).is_err());
}
