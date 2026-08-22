mod temporal_tests {
    use super::*;

    fn signals(entries: &[(&str, &[bool])]) -> std::collections::HashMap<String, Vec<bool>> {
        entries
            .iter()
            .map(|(name, values)| ((*name).to_string(), values.to_vec()))
            .collect()
    }

    #[test]
    fn never_after_rejects_a_state_that_reappears_after_terminal() {
        let temporal = [ModelTemporal::NeverAfter(
            "Running".to_string(),
            "Succeeded".to_string(),
        )];
        let values = signals(&[
            ("Running", &[true, false, false, true]),
            ("Succeeded", &[false, false, true, true]),
        ]);
        let error = VM::model_temporal_holds(&temporal, &values, &Default::default())
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
        let passing = signals(&[
            ("CancelRequested", &[true, false, false]),
            ("Cancelled", &[false, false, true]),
        ]);
        VM::model_temporal_holds(&temporal, &passing, &Default::default())
            .expect("consequence at the inclusive bound passes");

        let failing = signals(&[
            ("CancelRequested", &[true, false, false, false]),
            ("Cancelled", &[false, false, false, true]),
        ]);
        let error = VM::model_temporal_holds(&temporal, &failing, &Default::default())
            .expect_err("consequence beyond the bound fails");
        assert!(error.contains("eventually_within"), "{error}");
    }
}
