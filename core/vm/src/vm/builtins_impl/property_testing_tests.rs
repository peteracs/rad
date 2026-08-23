mod temporal_tests {
    use super::*;

    fn signals(
        temporal: &[ModelTemporal],
        entries: &[(&str, &[bool])],
    ) -> (ModelObservationPlan, Vec<Vec<bool>>, Vec<usize>) {
        let plan = ModelObservationPlan::new(temporal);
        let values = plan
            .names
            .iter()
            .map(|name| {
                entries
                    .iter()
                    .find_map(|(entry, values)| (*entry == name).then(|| values.to_vec()))
                    .unwrap_or_default()
            })
            .collect();
        let occurrences = vec![0; plan.names.len()];
        (plan, values, occurrences)
    }

    #[test]
    fn never_after_rejects_a_state_that_reappears_after_terminal() {
        let temporal = [ModelTemporal::NeverAfter(
            "Running".to_string(),
            "Succeeded".to_string(),
        )];
        let (plan, values, occurrences) = signals(&temporal, &[
            ("Running", &[true, false, false, true]),
            ("Succeeded", &[false, false, true, true]),
        ]);
        let error = VM::model_temporal_holds(&temporal, &plan, &values, &occurrences)
            .expect_err("Running reappeared after Succeeded");
        assert!(error.contains("never_after"), "{error}");
    }

    #[test]
    fn eventually_within_uses_the_declared_inclusive_bound() {
        let temporal = [ModelTemporal::EventuallyWithin(
            "CancelRequested".to_string(),
            "Cancelled".to_string(),
            2,
        )];
        let (passing_plan, passing, passing_occurrences) = signals(&temporal, &[
            ("CancelRequested", &[true, false, false]),
            ("Cancelled", &[false, false, true]),
        ]);
        VM::model_temporal_holds(&temporal, &passing_plan, &passing, &passing_occurrences)
            .expect("consequence at the inclusive bound passes");

        let (failing_plan, failing, failing_occurrences) = signals(&temporal, &[
            ("CancelRequested", &[true, false, false, false]),
            ("Cancelled", &[false, false, false, true]),
        ]);
        let error =
            VM::model_temporal_holds(&temporal, &failing_plan, &failing, &failing_occurrences)
            .expect_err("consequence beyond the bound fails");
        assert!(error.contains("eventually_within"), "{error}");
    }
}
