//! Native kernels for the zeta simple-zero certificate and its strategy scouts.

mod certificate;
mod correlation;
mod spacing;
mod windows;

pub use certificate::*;
pub use correlation::*;
pub use spacing::*;
pub use windows::*;

#[cfg(test)]
use certificate::{initial_box, SparseMinimum};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_minimum_matches_direct_minima() {
        let values = [9, 4, 7, 3, 8, 2, 6];
        let sparse = SparseMinimum::new(&values).unwrap();
        for left in 0..values.len() {
            for right in left..values.len() {
                assert_eq!(
                    sparse.query(left as i64, right as i64).unwrap(),
                    *values[left..=right].iter().min().unwrap()
                );
            }
        }
        assert_eq!(sparse.query(2, values.len() as i64).unwrap(), 0);
    }

    #[test]
    fn product_order_matches_python_itertools() {
        let components = [[10, 11], [20, 22], [30, 33]];
        assert_eq!(
            initial_box(0, &components).unwrap(),
            [10, 11, 10, 11, 10, 11, 10, 11, 10, 11, 10, 11]
        );
        assert_eq!(initial_box(1, &components).unwrap()[10..], [20, 22]);
        assert_eq!(
            initial_box(728, &components).unwrap(),
            [30, 33, 30, 33, 30, 33, 30, 33, 30, 33, 30, 33]
        );
    }

    #[test]
    fn even_spacing_bound_rigorously_obstructs_the_short_block_route() {
        let report = even_spacing_block_bound(23).unwrap();
        assert_eq!(report.gap_micros, 2_000_000);
        assert_eq!(report.span_tax_upper_scaled, 88_000_000);
        assert!(report.energy_upper_scaled < 20_000_000);
        assert!(report.total_upper_scaled < 966_405_477);
    }

    #[test]
    fn unit_spacing_bound_obstructs_the_affine_span_route() {
        let report = integer_spacing_block_bound(23, 1).unwrap();
        assert_eq!(report.gap_micros, 1_000_000);
        assert_eq!(report.span_tax_upper_scaled, 44_000_000);
        assert!(report.energy_upper_scaled < 23 * 4_000_000_000_i64 / 289);
        assert!(report.total_upper_scaled < 966_405_477);
    }

    #[test]
    fn bandwidth_scout_locates_a_nearby_seventy_percent_frontier() {
        let at_one = bandwidth_scout(1_000_000).unwrap();
        let beyond = bandwidth_scout(1_100_000).unwrap();
        assert!((672_500_000..672_501_000).contains(&at_one.baseline_lower_scaled));
        assert!(beyond.baseline_lower_scaled > 700_000_000);
    }

    #[test]
    fn fourth_moment_transfer_scout_is_finite() {
        let report = fourth_moment_transfer_scout(20_000, 8, 2).unwrap();
        assert_eq!(report.max_shift, 8);
        assert!(report.nonzero_coefficients > 0);
        assert!(report.mean_coefficient_scaled > 0);
        assert!(report.aggregate_ratio_scaled > 0);
        assert!(report.fejer_aggregate_ratio_scaled > 0);
        assert!(report.sampled_aggregate_ratio_scaled > 0);
        assert!(report.minimum_ratio_scaled <= report.maximum_ratio_scaled);
    }

    #[test]
    fn cosine_pair_scout_recovers_the_current_baseline() {
        let report = cosine_pair_scout(707_107).unwrap();
        assert!((672_400_000..672_600_000).contains(&report.baseline_scaled));
        assert!(report.simple_bound_scaled >= report.baseline_scaled);
        assert!(report.minimizing_gap_micros > 0);
    }

    #[test]
    fn cosine_direct_sum_scout_contains_the_standard_window() {
        let report = cosine_direct_sum_scout(1_300_000, 0).unwrap();
        assert!((672_400_000..672_600_000).contains(&report.baseline_scaled));
        assert!(report.simple_bound_scaled >= report.baseline_scaled);
    }

    #[test]
    fn perturbed_window_scout_recovers_the_certified_basin() {
        let report = perturbed_window_scout(0).unwrap();
        let two_mode = perturbed_window_two_mode_scout(0, 0).unwrap();
        assert!((672_400_000..672_600_000).contains(&report.baseline_scaled));
        assert!((3_800_000..3_850_000).contains(&report.local_c_scaled));
        assert!(report.simple_bound_scaled > report.baseline_scaled);
        assert_eq!(report.minimizing_gaps_micros.len(), 6);
        assert_eq!(report.baseline_scaled, two_mode.baseline_scaled);
        assert_eq!(report.local_c_scaled, two_mode.local_c_scaled);
        assert_eq!(two_mode.second_mode_coefficient_micros, 0);
    }

    #[test]
    fn periodic_density_scout_has_exact_seven_tenths_density() {
        // Four gaps of 1 and three gaps of 2 total ten.
        let report = periodic_density_scout(0b000_0111, 1_000_000).unwrap();
        assert_eq!(report.long_gap_micros, 2_000_000);
        assert!(report.energy_per_point_scaled > 0);
    }
}
