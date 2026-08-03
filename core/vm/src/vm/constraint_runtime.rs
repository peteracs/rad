//! Resource meters used only while RFC-0002 constraints execute.
//!
//! Keeping these meters separate from the ordinary VM sandbox budget is
//! important: one constraint invocation must not consume another one's
//! semantic allowance, and retained rejection data must be bounded while it
//! is collected rather than after the fact.

use crate::constraint_types::{ConstraintEvaluationFailure, ConstraintViolation};
use crate::value::{Builtin, Object, Value};

#[derive(Clone, Copy, Debug)]
pub(crate) struct BuiltinResourceCharge {
    pub(crate) fuel: u64,
    pub(crate) heap: usize,
}

/// Price constraint-safe builtins before they execute. Builtins with an
/// unbounded or not-yet-audited native implementation fail closed instead of
/// running outside the child meter. This is intentionally more conservative
/// than ordinary RAD execution: the RFC-0002 resource contract must be true
/// for native loops and temporary Rust collections as well as bytecode.
pub(crate) fn builtin_resource_charge(
    builtin: Builtin,
    args: &[Value],
) -> Result<BuiltinResourceCharge, String> {
    let object = std::mem::size_of::<Object>();
    let values = |count: usize| {
        count
            .saturating_mul(std::mem::size_of::<Value>())
            .saturating_mul(2)
            .saturating_add(object)
    };
    let count_arg = |index: usize| {
        args.get(index)
            .and_then(Value::as_int)
            .filter(|value| *value >= 0)
            .map(|value| value as usize)
    };
    let list_len = |index: usize| args.get(index).and_then(Value::as_list).map(|v| v.len());
    let text_len = |index: usize| args.get(index).and_then(Value::as_str).map(str::len);
    let collection_len = |index: usize| {
        args.get(index).map(|value| {
            value
                .as_list()
                .map(|items| items.len())
                .or_else(|| value.as_tuple().map(|items| items.len()))
                .or_else(|| value.as_map().map(|items| items.len()))
                .or_else(|| value.as_component().map(|component| component.values.len()))
                .or_else(|| value.as_sum_type().map(|sum| sum.fields.len()))
                .or_else(|| value.as_str().map(str::len))
                .or_else(|| value.as_bytebuf().map(|bytes| bytes.len()))
                .unwrap_or(1)
        })
    };
    let sized = |fuel: usize, heap: usize| BuiltinResourceCharge {
        fuel: u64::try_from(fuel).unwrap_or(u64::MAX),
        heap,
    };
    let fixed = || sized(1, object.saturating_add(128));

    let charge = match builtin {
        Builtin::Filled => {
            let count = count_arg(0).unwrap_or(0);
            sized(count.max(1), values(count))
        }
        Builtin::Range => {
            let ints = args.iter().map(Value::as_int).collect::<Option<Vec<_>>>();
            ints.and_then(|ints| {
                let (start, end, step) = match ints.as_slice() {
                    [end] => (0i128, *end as i128, 1i128),
                    [start, end] => (*start as i128, *end as i128, 1i128),
                    [start, end, step, ..] => (*start as i128, *end as i128, *step as i128),
                    _ => return None,
                };
                if step == 0 {
                    return None;
                }
                let distance = if step > 0 {
                    end.saturating_sub(start)
                } else {
                    start.saturating_sub(end)
                };
                let count = if distance <= 0 {
                    0
                } else {
                    usize::try_from((distance - 1) / step.abs() + 1).unwrap_or(usize::MAX)
                };
                Some(sized(
                    count.max(1),
                    values(count).saturating_add(count.saturating_mul(object)),
                ))
            })
            .unwrap_or_else(fixed)
        }
        Builtin::ByteBufNew => {
            let count = count_arg(0).unwrap_or(0);
            sized(count.max(1), count.saturating_mul(2).saturating_add(object))
        }
        Builtin::BufferAppend => {
            let bytes = args
                .first()
                .and_then(Value::as_buffer)
                .map(String::len)
                .unwrap_or(0)
                .saturating_add(text_len(1).unwrap_or(0))
                .saturating_mul(2)
                .saturating_add(object);
            sized(bytes.max(1), bytes)
        }
        Builtin::BufferToStr => {
            let bytes = args
                .first()
                .and_then(Value::as_buffer)
                .map(String::len)
                .unwrap_or(0)
                .saturating_mul(2)
                .saturating_add(object);
            sized(bytes.max(1), bytes)
        }
        Builtin::ByteBufSetU8
        | Builtin::ByteBufSetU32Le
        | Builtin::ByteBufSetI32Le
        | Builtin::ByteBufToList => args
            .first()
            .and_then(Value::as_bytebuf)
            .map(|bytes| sized(bytes.len().max(1), values(bytes.len())))
            .unwrap_or_else(fixed),
        Builtin::ByteBufFromList => list_len(0)
            .map(|count| sized(count.max(1), values(count)))
            .unwrap_or_else(fixed),
        Builtin::BitsetNew => fixed(),
        Builtin::BitsetSet | Builtin::BitsetClear => {
            let existing_words = args.first().and_then(Value::as_bitset).map(Vec::len);
            let bit = args.get(1).and_then(Value::as_int);
            match (existing_words, bit) {
                (Some(_), Some(bit)) if bit < 0 => fixed(),
                (Some(existing_words), Some(bit)) => {
                    let required_words = usize::try_from(bit)
                        .unwrap_or(usize::MAX)
                        .saturating_div(64)
                        .saturating_add(1);
                    let target_words = if matches!(builtin, Builtin::BitsetSet)
                        && required_words > existing_words
                    {
                        let mut capacity = existing_words.max(8);
                        while capacity < required_words {
                            let next = capacity.saturating_mul(2);
                            if next == capacity {
                                capacity = usize::MAX;
                                break;
                            }
                            capacity = next;
                        }
                        capacity
                    } else {
                        existing_words
                    };
                    // `into_bitset` first clones the source Vec. Growth may
                    // then hold that clone and the resized buffer at once;
                    // the final GC Object is a third independently accounted
                    // allocation. Quote their conservative peak together.
                    let word = std::mem::size_of::<u64>();
                    let heap = existing_words
                        .saturating_mul(word)
                        .saturating_add(target_words.saturating_mul(word))
                        .saturating_add(object);
                    sized(existing_words.saturating_add(target_words).max(1), heap)
                }
                _ => fixed(),
            }
        }
        Builtin::Replace => {
            let source = args.first().and_then(Value::as_str);
            let from = args.get(1).and_then(Value::as_str);
            let to = args.get(2).and_then(Value::as_str);
            let Some((source, from, to)) = source.zip(from).zip(to).map(|((a, b), c)| (a, b, c))
            else {
                return Ok(fixed());
            };
            let matches_upper = if from.is_empty() {
                source.len().saturating_add(1)
            } else {
                source.len().saturating_div(from.len())
            };
            // If replacement shrinks a match, the largest result is the
            // no-match source. If it expands a match, applying the maximum
            // possible non-overlapping match count is the upper bound.
            let output = source
                .len()
                .saturating_add(matches_upper.saturating_mul(to.len().saturating_sub(from.len())));
            // `str::replace` builds a temporary String; importing that String
            // into Object::Str allocates the retained Arc<str> separately.
            let bytes = output.saturating_mul(2).saturating_add(object);
            sized(source.len().saturating_add(matches_upper).max(1), bytes)
        }
        Builtin::Push => {
            if let Some(source) = args.first().and_then(Value::as_str) {
                let appended = text_len(1).unwrap_or(0);
                let output = source.len().saturating_add(appended);
                sized(
                    output.max(1),
                    output.saturating_mul(2).saturating_add(object),
                )
            } else {
                let count = list_len(0).unwrap_or(1).saturating_add(1);
                sized(count.max(1), values(count))
            }
        }
        Builtin::Pop | Builtin::PopLast | Builtin::DropLast | Builtin::DropFirst => {
            if let Some(source) = args.first().and_then(Value::as_str) {
                sized(
                    source.len().max(1),
                    source.len().saturating_mul(2).saturating_add(object),
                )
            } else {
                let count = list_len(0).unwrap_or(1);
                sized(count.max(1), values(count))
            }
        }
        Builtin::Reverse => {
            if let Some(source) = args.first().and_then(Value::as_str) {
                sized(
                    source.len().max(1),
                    source.len().saturating_mul(2).saturating_add(object),
                )
            } else {
                let count = list_len(0).unwrap_or(1);
                sized(count.max(1), values(count))
            }
        }
        Builtin::Slice => {
            let count = collection_len(0).unwrap_or(1);
            sized(count.max(1), values(count))
        }
        Builtin::Map | Builtin::Filter => {
            let count = collection_len(0).unwrap_or(1);
            let string_materialization = if args.first().and_then(Value::as_str).is_some() {
                count.saturating_mul(object.saturating_add(32))
            } else {
                0
            };
            sized(
                count.max(1),
                values(count).saturating_add(string_materialization),
            )
        }
        Builtin::Append | Builtin::Extend => {
            match (
                args.first().and_then(Value::as_list),
                args.first().and_then(Value::as_str),
                args.get(1).and_then(Value::as_list),
                args.get(1).and_then(Value::as_str),
            ) {
                (Some(left), _, Some(right), _) => {
                    let count = left.len().saturating_add(right.len());
                    sized(count.max(1), values(count))
                }
                (Some(left), _, _, Some(right)) => {
                    let chars_upper = right.len();
                    let count = left.len().saturating_add(chars_upper);
                    sized(
                        count.max(1),
                        values(count)
                            .saturating_add(chars_upper.saturating_mul(object.saturating_add(32)))
                            .saturating_add(object),
                    )
                }
                (_, Some(left), _, Some(right)) => {
                    let output = left.len().saturating_add(right.len());
                    sized(
                        output.max(1),
                        output.saturating_mul(2).saturating_add(object),
                    )
                }
                // Rendering an arbitrary list into one String requires a
                // graph-aware display bound that v0 deliberately lacks.
                (_, Some(_), Some(_), _) => {
                    return Err(
                        "constraint builtin 'append' cannot prove string output from list values"
                            .into(),
                    )
                }
                _ => fixed(),
            }
        }
        Builtin::Values => match args.first() {
            Some(value) if value.as_map().is_some() => {
                let count = value.as_map().map(|map| map.len()).unwrap_or(0);
                sized(count.max(1), values(count))
            }
            Some(value) if value.as_component().is_some() => {
                let component = value.as_component().unwrap();
                let names = component
                    .layout
                    .iter()
                    .map(|name| name.len().saturating_add(std::mem::size_of::<String>()))
                    .fold(0usize, usize::saturating_add);
                sized(
                    component.values.len().max(1),
                    values(component.values.len()).saturating_add(names),
                )
            }
            Some(value) if value.as_sum_type().is_some() => {
                let sum = value.as_sum_type().unwrap();
                let names = sum
                    .fields
                    .keys()
                    .map(|name| name.len().saturating_add(std::mem::size_of::<String>()))
                    .fold(0usize, usize::saturating_add);
                sized(
                    sum.fields.len().max(1),
                    values(sum.fields.len()).saturating_add(names),
                )
            }
            _ => fixed(),
        },
        Builtin::Enumerate | Builtin::Zip => {
            let left = collection_len(0).unwrap_or(1);
            let right = if matches!(builtin, Builtin::Zip) {
                collection_len(1).unwrap_or(1)
            } else {
                0
            };
            let count = if matches!(builtin, Builtin::Zip) {
                left.min(right)
            } else {
                left
            };
            let string_nodes = args
                .iter()
                .take(if matches!(builtin, Builtin::Zip) {
                    2
                } else {
                    1
                })
                .filter_map(Value::as_str)
                .map(str::len)
                .fold(0usize, usize::saturating_add)
                .saturating_mul(object.saturating_add(32));
            sized(
                count.max(1),
                count
                    .saturating_mul(object.saturating_add(64))
                    .saturating_add(values(left.saturating_add(right).saturating_add(count)))
                    .saturating_add(string_nodes),
            )
        }
        Builtin::SetAt if args.first().is_some_and(|value| value.as_list().is_some()) => {
            let count = list_len(0).unwrap_or(1);
            sized(count.max(1), values(count))
        }
        Builtin::Split | Builtin::Chars => {
            let source = args.first().and_then(Value::as_str);
            let Some(source) = source else {
                return Ok(fixed());
            };
            let parts_upper = if matches!(builtin, Builtin::Chars) {
                source.len()
            } else {
                args.get(1)
                    .and_then(Value::as_str)
                    .map(|delimiter| {
                        if delimiter.is_empty() {
                            // `str::split("")` includes both boundary
                            // segments in addition to every character.
                            source.len().saturating_add(2)
                        } else {
                            source
                                .len()
                                .saturating_div(delimiter.len())
                                .saturating_add(1)
                        }
                    })
                    .unwrap_or(1)
            };
            let heap = source
                .len()
                .saturating_mul(2)
                .saturating_add(parts_upper.saturating_mul(object.saturating_add(32)))
                .saturating_add(values(parts_upper));
            sized(source.len().saturating_add(parts_upper).max(1), heap)
        }
        Builtin::Trim | Builtin::SubstringBytes | Builtin::ToUpper | Builtin::ToLower => {
            let count = text_len(0).unwrap_or(1);
            sized(count, count.saturating_mul(16).saturating_add(object))
        }
        // Callback-produced keys make a pre-execution peak bound impossible
        // without metering the native map builder incrementally. Keep this
        // helper outside the constraint-safe whitelist until that exists.
        Builtin::GroupBy => {
            return Err(
                "constraint builtin 'group_by' has no proven peak-allocation contract".into(),
            )
        }
        Builtin::MapOr => sized(2, object.saturating_mul(2)),
        Builtin::Reduce
        | Builtin::Any
        | Builtin::All
        | Builtin::Find
        | Builtin::MaxBy
        | Builtin::MinBy => {
            let count = collection_len(0).unwrap_or(1);
            sized(count, values(count))
        }
        Builtin::SortBy => {
            let count = collection_len(0).unwrap_or(1);
            let work = count.saturating_mul((usize::BITS - count.max(1).leading_zeros()) as usize);
            sized(
                work.max(1),
                count.saturating_mul(256).saturating_add(object),
            )
        }
        Builtin::GenInt | Builtin::GenFloat => sized(100, values(100)),
        Builtin::GenStr => sized(
            21,
            values(21)
                .saturating_add(21usize.saturating_mul(object))
                .saturating_add(420),
        ),
        Builtin::GenBool => sized(2, values(2)),
        Builtin::GenList => {
            let count = list_len(0).unwrap_or(1);
            let expanded = count.saturating_mul(count.saturating_add(1)) / 2;
            sized(
                expanded.max(1),
                values(expanded).saturating_add(count.saturating_add(1).saturating_mul(object)),
            )
        }
        Builtin::BufferNew => fixed(),
        Builtin::Abs
        | Builtin::Sign
        | Builtin::Min
        | Builtin::Max
        | Builtin::Chr
        | Builtin::Ord
        | Builtin::IntDiv
        | Builtin::UnwrapOr
        | Builtin::IsSome
        | Builtin::IsNone
        | Builtin::Popcount
        | Builtin::Ctz
        | Builtin::Shl
        | Builtin::Shr
        | Builtin::Clamp
        | Builtin::Round
        | Builtin::Floor
        | Builtin::Ceil
        | Builtin::Sqrt
        | Builtin::Pow
        | Builtin::ByteAt
        | Builtin::ByteLen
        | Builtin::BitsetHas
        | Builtin::ByteBufLen
        | Builtin::ByteBufGet
        | Builtin::ByteBufGetU32Le
        | Builtin::ByteBufGetI32Le => fixed(),
        Builtin::Int | Builtin::Float | Builtin::TryInt | Builtin::TryFloat => {
            let count = text_len(0).unwrap_or(1);
            sized(
                count,
                count
                    .saturating_mul(2)
                    .saturating_add(object)
                    .saturating_add(128),
            )
        }
        Builtin::Len
        | Builtin::Sum
        | Builtin::Product
        | Builtin::StartsWith
        | Builtin::EndsWith => {
            let count = collection_len(0).unwrap_or(1);
            sized(count, object.saturating_add(128))
        }
        Builtin::Keys
        | Builtin::Entries
        | Builtin::RemoveKey
        | Builtin::Merge
        | Builtin::Sort
        | Builtin::TypeOf
        | Builtin::IndexOf => {
            return Err(format!(
                "constraint builtin '{}' has no proven native resource upper bound",
                builtin.name()
            ))
        }
        _ => {
            return Err(format!(
                "constraint builtin '{}' has no audited resource contract",
                builtin.name()
            ))
        }
    };
    Ok(charge)
}

#[derive(Clone, Debug)]
pub(crate) struct ConstraintExecutionMeter {
    fuel_remaining: u64,
    heap_limit: usize,
}

impl ConstraintExecutionMeter {
    pub(crate) fn new(fuel: u64, heap_limit: usize) -> Self {
        Self {
            fuel_remaining: fuel,
            heap_limit,
        }
    }

    pub(crate) fn charge_instruction(&mut self) -> Result<(), String> {
        self.charge_work(1)
    }

    pub(crate) fn charge_work(&mut self, units: u64) -> Result<(), String> {
        if units > self.fuel_remaining {
            return Err("Budget exhausted: constraint instruction (fuel) limit reached".into());
        }
        self.fuel_remaining -= units;
        Ok(())
    }

    pub(crate) fn ensure_heap(&self, allocated: usize, temporary: usize) -> Result<(), String> {
        if allocated.saturating_add(temporary) > self.heap_limit {
            return Err(format!(
                "Budget exhausted: constraint memory limit exceeded ({} retained + {} temporary bytes)",
                allocated, temporary
            ));
        }
        Ok(())
    }
}

impl super::VM {
    pub(crate) fn meter_constraint_resources(
        &mut self,
        work: usize,
        temporary_heap: usize,
    ) -> Result<(), String> {
        let Some(meter) = self.constraint_meter.as_mut() else {
            return Ok(());
        };
        // Check memory first so an operation that exceeds both contracts is
        // rejected before its allocation can begin.
        meter.ensure_heap(self.gc.bytes_allocated(), temporary_heap)?;
        meter.charge_work(u64::try_from(work).unwrap_or(u64::MAX))
    }

    pub(crate) fn meter_constraint_builtin(
        &mut self,
        builtin: Builtin,
        args: &[Value],
    ) -> Result<(), String> {
        if self.constraint_meter.is_none() {
            return Ok(());
        }
        let charge = builtin_resource_charge(builtin, args)?;
        let Some(meter) = self.constraint_meter.as_mut() else {
            return Ok(());
        };
        meter.ensure_heap(self.gc.bytes_allocated(), charge.heap)?;
        meter.charge_work(charge.fuel)
    }
}

#[derive(Debug)]
pub(crate) struct ConstraintOutcomeMeter {
    max_count: usize,
    max_bytes: usize,
    retained_count: usize,
    retained_bytes: usize,
    overflowed: bool,
}

impl ConstraintOutcomeMeter {
    pub(crate) fn new(max_count: usize, max_bytes: usize) -> Self {
        Self {
            max_count,
            max_bytes,
            retained_count: 0,
            retained_bytes: 0,
            overflowed: false,
        }
    }

    fn identity_bytes(identity: &crate::constraint_types::ConstraintIdentity) -> usize {
        identity
            .qualified_name
            .len()
            .saturating_add(identity.attached_component.len())
    }

    fn violation_bytes(value: &ConstraintViolation) -> usize {
        Self::identity_bytes(&value.constraint)
            .saturating_add(value.code.len())
            .saturating_add(value.candidate.component.len())
            .saturating_add(64)
    }

    fn failure_bytes(value: &ConstraintEvaluationFailure) -> usize {
        Self::identity_bytes(&value.constraint)
            .saturating_add(value.code.len())
            .saturating_add(value.message.len())
            .saturating_add(48)
    }

    fn reserve(&mut self, count: usize, bytes: usize) -> bool {
        if self.overflowed
            || count > self.max_count.saturating_sub(self.retained_count)
            || bytes > self.max_bytes.saturating_sub(self.retained_bytes)
        {
            self.overflowed = true;
            return false;
        }
        self.retained_count += count;
        self.retained_bytes += bytes;
        true
    }

    pub(crate) fn reserve_additional(&mut self, bytes: usize) -> bool {
        self.reserve(0, bytes)
    }

    pub(crate) fn retain_violations(
        &mut self,
        target: &mut Vec<ConstraintViolation>,
        incoming: Vec<ConstraintViolation>,
    ) {
        let bytes = incoming
            .iter()
            .map(Self::violation_bytes)
            .fold(0usize, usize::saturating_add);
        if self.reserve(incoming.len(), bytes) {
            target.extend(incoming);
        } else {
            target.clear();
        }
    }

    pub(crate) fn retain_failure(
        &mut self,
        target: &mut Vec<ConstraintEvaluationFailure>,
        failure: ConstraintEvaluationFailure,
    ) {
        let bytes = Self::failure_bytes(&failure);
        if self.reserve(1, bytes) {
            target.push(failure);
        } else {
            target.clear();
        }
    }

    pub(crate) fn overflowed(&self) -> bool {
        self.overflowed
    }
}

#[cfg(test)]
mod resource_contract_tests {
    use super::builtin_resource_charge;
    use crate::gc::GcHeap;
    use crate::value::{Builtin, Object, Value};
    use crate::vm::builtins_impl::{bi_bitset_clear, bi_bitset_set, bi_replace};

    #[test]
    fn bitset_clear_quote_includes_the_existing_vector_clone() {
        let mut gc = GcHeap::new();
        let words = 16_384usize;
        let bitset = Value::bitset(&mut gc, vec![u64::MAX; words]);
        let quote = builtin_resource_charge(Builtin::BitsetClear, &[bitset, Value::int(0)])
            .expect("bitset_clear is audited");

        assert!(
            quote.heap
                >= words
                    .saturating_mul(std::mem::size_of::<u64>())
                    .saturating_add(std::mem::size_of::<Object>()),
            "the quote must cover cloning the complete existing bitset"
        );
    }

    #[test]
    fn empty_pattern_replace_quote_covers_the_replacement_output() {
        let mut gc = GcHeap::new();
        let source = Value::from_string(&mut gc, String::new());
        let pattern = Value::from_string(&mut gc, String::new());
        let replacement_text = "x".repeat(32_768);
        let replacement = Value::from_string(&mut gc, replacement_text.clone());
        let quote = builtin_resource_charge(Builtin::Replace, &[source, pattern, replacement])
            .expect("replace is audited");

        assert!(
            quote.heap
                >= replacement_text
                    .len()
                    .saturating_mul(2)
                    .saturating_add(std::mem::size_of::<Object>()),
            "the quote must cover Rust's temporary String and the retained RAD string"
        );
    }

    #[test]
    fn bitset_growth_quote_covers_source_clone_and_resized_result() {
        let mut gc = GcHeap::new();
        let words = 257usize;
        let target_bit = 65_536i64;
        let bitset = Value::bitset(&mut gc, vec![u64::MAX; words]);
        let args = [bitset, Value::int(target_bit)];
        let quote =
            builtin_resource_charge(Builtin::BitsetSet, &args).expect("bitset_set is audited");
        let before = gc.bytes_allocated();
        bi_bitset_set(&mut gc, args.to_vec()).unwrap();
        assert!(gc.bytes_allocated().saturating_sub(before) <= quote.heap);
        assert!(
            quote.heap
                >= words
                    .saturating_mul(std::mem::size_of::<u64>())
                    .saturating_add(
                        usize::try_from(target_bit)
                            .unwrap()
                            .saturating_div(64)
                            .saturating_add(1)
                            .saturating_mul(std::mem::size_of::<u64>()),
                    )
        );
    }

    #[test]
    fn shrinking_replace_quote_keeps_the_no_match_source_as_upper_bound() {
        let mut gc = GcHeap::new();
        let source_text = "x".repeat(32_769);
        let args = [
            Value::from_string(&mut gc, source_text.clone()),
            Value::from_string(&mut gc, "absent-pattern".into()),
            Value::from_string(&mut gc, String::new()),
        ];
        let quote = builtin_resource_charge(Builtin::Replace, &args).expect("replace is audited");
        assert!(
            quote.heap
                >= source_text
                    .len()
                    .saturating_mul(2)
                    .saturating_add(std::mem::size_of::<Object>())
        );
    }

    #[test]
    fn audited_quotes_dominate_retained_allocation_across_boundaries() {
        for words in [0usize, 1, 8, 257, 16_384] {
            let mut gc = GcHeap::new();
            let bitset = Value::bitset(&mut gc, vec![u64::MAX; words]);
            let args = [bitset, Value::int(0)];
            let quote = builtin_resource_charge(Builtin::BitsetClear, &args).unwrap();
            let before = gc.bytes_allocated();
            bi_bitset_clear(&mut gc, args.to_vec()).unwrap();
            assert!(gc.bytes_allocated().saturating_sub(before) <= quote.heap);
        }

        for (source, pattern, replacement) in [
            ("", "", "x".repeat(4096)),
            ("ééé", "", "界".repeat(256)),
            ("aaaaaa", "aa", "replacement".repeat(64)),
            ("unchanged", "missing", "x".repeat(1024)),
        ] {
            let mut gc = GcHeap::new();
            let args = [
                Value::from_string(&mut gc, source.to_string()),
                Value::from_string(&mut gc, pattern.to_string()),
                Value::from_string(&mut gc, replacement),
            ];
            let quote = builtin_resource_charge(Builtin::Replace, &args).unwrap();
            let before = gc.bytes_allocated();
            bi_replace(&mut gc, args.to_vec()).unwrap();
            assert!(gc.bytes_allocated().saturating_sub(before) <= quote.heap);
        }
    }

    #[test]
    fn native_helpers_without_proven_bounds_fail_closed() {
        let mut gc = GcHeap::new();
        let empty = Value::map(&mut gc, Default::default());
        for builtin in [
            Builtin::Keys,
            Builtin::Entries,
            Builtin::Merge,
            Builtin::RemoveKey,
            Builtin::Sort,
            Builtin::TypeOf,
            Builtin::IndexOf,
            Builtin::GroupBy,
        ] {
            let error = builtin_resource_charge(builtin, &[empty]).unwrap_err();
            assert!(error.contains("no proven"), "{builtin:?}: {error}");
        }
    }
}
