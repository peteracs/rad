

macro_rules! native_contracts {
    ($(($builtin:ident, $proof:literal, $class:ident)),+ $(,)?) => {
        const NATIVE_CONTRACTS: &[NativeContract] = &[
            $(NativeContract {
                builtin: Builtin::$builtin,
                proof_id: $proof,
                proof_class: NativeProofClass::$class,
            }),+
        ];
    };
}

// Admission is generated from proof records rather than a second whitelist.
// Adding a constraint-safe native therefore requires a named proof class and
// makes the boundary/peak/work suite fail until that class supplies cases.
native_contracts!(
    (Filled, "filled.v1", Filled),
    (Range, "range.checked-plan.v2", Range),
    (BitsetNew, "bitset.fixed.v1", Fixed),
    (BitsetSet, "bitset.resize.v2", Bitset),
    (BitsetClear, "bitset.clone.v2", Bitset),
    (Replace, "replace.utf8.v2", Replace),
    (Abs, "numeric.fixed.v1", Fixed),
    (Sign, "numeric.fixed.v1", Fixed),
    (Min, "numeric.fixed.v1", Fixed),
    (Max, "numeric.fixed.v1", Fixed),
    (Chr, "unicode.scalar.v1", Fixed),
    (Ord, "unicode.scalar.v1", Fixed),
    (IntDiv, "numeric.fixed.v1", Fixed),
    (Popcount, "bit.fixed.v1", Fixed),
    (Ctz, "bit.fixed.v1", Fixed),
    (Shl, "bit.fixed.v1", Fixed),
    (Shr, "bit.fixed.v1", Fixed),
    (Clamp, "numeric.fixed.v1", Fixed),
    (Round, "float.fixed.v1", Fixed),
    (Floor, "float.fixed.v1", Fixed),
    (Ceil, "float.fixed.v1", Fixed),
    (Sqrt, "float.fixed.v1", Fixed),
    (Pow, "float.libm.v1", Fixed),
    (ByteAt, "bytes.fixed.v1", Fixed),
    (ByteLen, "bytes.fixed.v1", Fixed),
    (BitsetHas, "bitset.fixed.v1", Fixed),
    (Len, "collection.scan.v1", TextScan),
    (StartsWith, "text.scan.v1", TextScan),
    (EndsWith, "text.scan.v1", TextScan),
    (TypeOf, "type-name.dynamic.v1", TypeName),
);

fn native_contract(builtin: Builtin) -> Option<&'static NativeContract> {
    NATIVE_CONTRACTS
        .iter()
        .find(|contract| contract.builtin == builtin)
}

/// Closed RFC-0002 native whitelist. Admission is intentionally independent
/// of argument shape: an unsupported builtin cannot smuggle itself through a
/// cheap error branch and then receive different legal arguments at runtime.
#[cfg(test)]
fn has_mechanically_checked_contract(builtin: Builtin) -> bool {
    native_contract(builtin).is_some()
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
    let Some(contract) = native_contract(builtin) else {
        return Err(format!(
            "constraint builtin '{}' has no mechanically verified native resource upper bound",
            builtin.name()
        ));
    };
    debug_assert!(!contract.proof_id.is_empty());
    let _proof_class = contract.proof_class;
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
            let plan = super::range_plan::RangePlan::from_args(args)?;
            // Every generated i64 may leave the immediate NaN-box range and
            // become a GC BigInt with its own limb allocation. Charge a
            // deliberately conservative fixed envelope per element in
            // addition to the result Vec and list object.
            let integer_object = object.saturating_mul(4).saturating_add(128);
            sized(
                plan.count.max(1),
                values(plan.count).saturating_add(plan.count.saturating_mul(integer_object)),
            )
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
            // `str::replace` may grow its String geometrically before the
            // completed buffer is converted into a separately allocated
            // retained Arc<str>. The contract oracle observes all three live
            // phases, so reserve four complete outputs plus the GC object.
            let bytes = output.saturating_mul(4).saturating_add(object);
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
        | Builtin::MinBy => unreachable!("callback builtins fail the closed whitelist"),
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
        Builtin::TypeOf => {
            // `type_name` creates one temporary host String and `bi_typeof`
            // copies it into a GC string. Component/state/sum names are
            // dynamic, so price from the resolved name instead of a fixed
            // tag length. The factor and fixed slack dominate both strings
            // plus their object/allocation headers.
            let bytes = args
                .first()
                .map(Value::type_name)
                .map(|name| name.len())
                .unwrap_or(0);
            sized(
                bytes.max(1),
                bytes
                    .saturating_mul(4)
                    .saturating_add(object.saturating_mul(2))
                    .saturating_add(256),
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