//! One checked plan shared by `range` pricing and execution.
//!
//! Keeping count calculation and value generation here prevents the native
//! builtin from drifting away from the resource quote. All arithmetic is
//! widened before it is checked; execution is count-bounded rather than an
//! open-ended incremental loop.

use crate::value::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RangePlan {
    pub(crate) start: i64,
    pub(crate) step: i64,
    pub(crate) count: usize,
}

impl RangePlan {
    pub(crate) fn from_args(args: &[Value]) -> Result<Self, String> {
        if args.is_empty() {
            return Err("range() requires at least 1 argument".into());
        }
        let integer = |index: usize| {
            args[index]
                .as_int()
                .ok_or_else(|| "range() expects int".to_string())
        };
        let (start, end, step) = match args.len() {
            1 => (0, integer(0)?, 1),
            2 => (integer(0)?, integer(1)?, 1),
            _ => (integer(0)?, integer(1)?, integer(2)?),
        };
        if step == 0 {
            return Err("range() step cannot be zero".into());
        }

        let start_wide = i128::from(start);
        let end_wide = i128::from(end);
        let step_wide = i128::from(step);
        let distance = if step > 0 {
            end_wide - start_wide
        } else {
            start_wide - end_wide
        };
        let count_wide = if distance <= 0 {
            0
        } else {
            let stride = step_wide.abs();
            (distance - 1) / stride + 1
        };
        let count = usize::try_from(count_wide)
            .map_err(|_| "range() result length exceeds this platform".to_string())?;

        let plan = Self { start, step, count };
        if count > 0 {
            // Validate the final generated value during planning. Intermediate
            // values lie between the checked start and final value.
            plan.value_at(count - 1)?;
        }
        Ok(plan)
    }

    pub(crate) fn value_at(self, index: usize) -> Result<i64, String> {
        if index >= self.count {
            return Err("range() plan index is outside the generated result".into());
        }
        let index = i128::try_from(index)
            .map_err(|_| "range() index exceeds checked arithmetic".to_string())?;
        let offset = i128::from(self.step)
            .checked_mul(index)
            .ok_or_else(|| "range() step multiplication overflowed".to_string())?;
        let value = i128::from(self.start)
            .checked_add(offset)
            .ok_or_else(|| "range() value addition overflowed".to_string())?;
        i64::try_from(value).map_err(|_| "range() generated value exceeds i64".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::RangePlan;
    use crate::gc::GcHeap;
    use crate::value::Value;

    fn plan(values: &[i64]) -> Result<RangePlan, String> {
        let mut gc = GcHeap::new();
        let args = values
            .iter()
            .copied()
            .map(|value| Value::from_int(&mut gc, value))
            .collect::<Vec<_>>();
        RangePlan::from_args(&args)
    }

    #[test]
    fn boundary_ranges_are_finite_and_exact() {
        let positive = plan(&[i64::MAX - 1, i64::MAX, 2]).unwrap();
        assert_eq!(positive.count, 1);
        assert_eq!(positive.value_at(0).unwrap(), i64::MAX - 1);

        let negative = plan(&[i64::MIN + 1, i64::MIN, -2]).unwrap();
        assert_eq!(negative.count, 1);
        assert_eq!(negative.value_at(0).unwrap(), i64::MIN + 1);

        assert_eq!(plan(&[10, 0, 1]).unwrap().count, 0);
        assert_eq!(plan(&[0, 10, -1]).unwrap().count, 0);
        assert!(plan(&[0, 10, 0]).unwrap_err().contains("zero"));
    }

    #[test]
    fn minimum_step_uses_widened_arithmetic() {
        let plan = plan(&[i64::MAX, i64::MIN, i64::MIN]).unwrap();
        assert_eq!(plan.count, 2);
        assert_eq!(plan.value_at(0).unwrap(), i64::MAX);
        assert_eq!(plan.value_at(1).unwrap(), -1);
    }

    #[test]
    fn generated_values_match_the_checked_reference_count() {
        let boundaries = [
            (i64::MIN, i64::MAX, i64::MAX),
            (i64::MAX, i64::MIN, i64::MIN),
            (-17, 18, 3),
            (17, -18, -3),
            (0, 0, 1),
        ];
        for (start, end, step) in boundaries {
            let plan = plan(&[start, end, step]).unwrap();
            let values = (0..plan.count)
                .map(|index| plan.value_at(index).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(values.len(), plan.count);
            assert!(values
                .iter()
                .all(|value| if step > 0 { *value < end } else { *value > end }));
        }
    }

    #[test]
    fn fuzz_checked_range_plans_never_wrap_or_disagree_with_reference() {
        let mut state = 0x5241_4E47_455F_5630_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as i64
        };

        for _ in 0..50_000 {
            let start = next();
            let end = next();
            let mut step = next();
            if step == 0 {
                step = 1;
            }

            let plan = plan(&[start, end, step]).unwrap();
            let distance = if step > 0 {
                i128::from(end) - i128::from(start)
            } else {
                i128::from(start) - i128::from(end)
            };
            let expected = if distance <= 0 {
                0
            } else {
                usize::try_from((distance - 1) / i128::from(step).abs() + 1).unwrap()
            };
            assert_eq!(plan.count, expected);

            if expected == 0 {
                continue;
            }
            let first = plan.value_at(0).unwrap();
            let last = plan.value_at(expected - 1).unwrap();
            assert_eq!(first, start);
            if step > 0 {
                assert!(last < end);
                assert!(i128::from(last) + i128::from(step) >= i128::from(end));
            } else {
                assert!(last > end);
                assert!(i128::from(last) + i128::from(step) <= i128::from(end));
            }
        }
    }
}
