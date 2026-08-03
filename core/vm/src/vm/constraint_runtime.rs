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
                Some(sized(count.max(1), values(count)))
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
        Builtin::BitsetNew | Builtin::BitsetSet | Builtin::BitsetClear => count_arg(1)
            .or_else(|| count_arg(0))
            .map(|bit| {
                let bytes = bit
                    .saturating_div(64)
                    .saturating_add(1)
                    .saturating_mul(std::mem::size_of::<u64>())
                    .saturating_mul(2)
                    .saturating_add(object);
                sized(bit.saturating_div(64).saturating_add(1), bytes)
            })
            .unwrap_or_else(fixed),
        Builtin::Replace => {
            let source = text_len(0).unwrap_or(0);
            let from = text_len(1).unwrap_or(0).max(1);
            let to = text_len(2).unwrap_or(0);
            let bytes = source
                .saturating_div(from)
                .saturating_mul(to.max(from))
                .saturating_mul(2)
                .saturating_add(object);
            sized(source.max(1), bytes)
        }
        Builtin::Push
        | Builtin::Pop
        | Builtin::PopLast
        | Builtin::DropLast
        | Builtin::DropFirst
        | Builtin::Sort
        | Builtin::Reverse
        | Builtin::Slice
        | Builtin::Map
        | Builtin::Filter => {
            let count = collection_len(0).unwrap_or(1);
            let work = if matches!(builtin, Builtin::Sort) {
                count.saturating_mul((usize::BITS - count.max(1).leading_zeros()) as usize)
            } else {
                count
            };
            sized(work.max(1), values(count))
        }
        Builtin::Append | Builtin::Extend => {
            let count = collection_len(0)
                .unwrap_or(0)
                .saturating_add(collection_len(1).unwrap_or(0));
            sized(count.max(1), values(count))
        }
        Builtin::Keys | Builtin::Values => {
            let count = collection_len(0).unwrap_or(1);
            sized(count, count.saturating_mul(256).saturating_add(object))
        }
        Builtin::Entries | Builtin::Enumerate | Builtin::Zip => {
            let count = collection_len(0)
                .unwrap_or(1)
                .min(collection_len(1).unwrap_or(usize::MAX));
            sized(
                count.max(1),
                count.saturating_mul(512).saturating_add(object),
            )
        }
        Builtin::SetAt | Builtin::RemoveKey => {
            let count = collection_len(0).unwrap_or(1);
            sized(count, count.saturating_mul(256).saturating_add(object))
        }
        Builtin::Merge => {
            let count = collection_len(0)
                .unwrap_or(0)
                .saturating_add(collection_len(1).unwrap_or(0));
            sized(
                count.max(1),
                count.saturating_mul(256).saturating_add(object),
            )
        }
        Builtin::Split | Builtin::Chars => {
            let count = text_len(0).unwrap_or(1);
            sized(count, count.saturating_mul(256).saturating_add(object))
        }
        Builtin::Trim | Builtin::SubstringBytes | Builtin::ToUpper | Builtin::ToLower => {
            let count = text_len(0).unwrap_or(1);
            sized(count, count.saturating_mul(8).saturating_add(object))
        }
        Builtin::GroupBy => {
            let count = collection_len(0).unwrap_or(1);
            sized(count, count.saturating_mul(512).saturating_add(object))
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
        Builtin::GenStr => sized(21, 4096),
        Builtin::GenBool => sized(2, values(2)),
        Builtin::GenList => {
            let count = list_len(0).unwrap_or(1);
            let expanded = count.saturating_mul(count.saturating_add(1)) / 2;
            sized(expanded.max(1), values(expanded))
        }
        Builtin::BufferNew => fixed(),
        Builtin::TypeOf
        | Builtin::Abs
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
            sized(count, object.saturating_add(128))
        }
        Builtin::Len
        | Builtin::Sum
        | Builtin::Product
        | Builtin::IndexOf
        | Builtin::StartsWith
        | Builtin::EndsWith => {
            let count = collection_len(0).unwrap_or(1);
            sized(count, object.saturating_add(128))
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
