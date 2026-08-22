use crate::gc::GcHeap;
use crate::opcode::Chunk;
use crate::value::Value;

pub(crate) fn constant_value(chunk: &Chunk, idx: usize) -> Result<Value, String> {
    chunk
        .constants
        .get(idx)
        .copied()
        .ok_or_else(|| format!("Invalid constant index {}", idx))
}

/// String constant as a `Copy` handle rather than an owned `String`.
///
/// `Value` is a NaN-boxed `u64`, so copying it out ends the borrow on `chunk`
/// while the text itself stays in the constant pool. Callers bind the result
/// to a local and read `&str` off *that*, which leaves `&mut self` free.
///
/// The hot ECS field opcodes used `constant_string`, which allocates a fresh
/// `String` per call purely to name a constant already sitting in the chunk:
/// twelve allocations per entity per frame in the dispatch60 workload
/// (~72M for the production run) before a single field was touched.
#[inline]
pub(crate) fn constant_string_value(chunk: &Chunk, idx: usize) -> Result<Value, String> {
    let value = constant_value(chunk, idx)?;
    if value.as_str().is_none() {
        return Err(format!(
            "Expected string constant at index {}, got {}",
            idx,
            value.type_name()
        ));
    }
    Ok(value)
}

pub(crate) fn constant_string(chunk: &Chunk, idx: usize) -> Result<String, String> {
    match chunk.constants.get(idx) {
        Some(v) => v.as_str().map(|s| s.to_string()).ok_or_else(|| {
            format!(
                "Expected string constant at index {}, got {}",
                idx,
                v.type_name()
            )
        }),
        None => Err(format!("Invalid constant index {}", idx)),
    }
}

/// String or `system` reference constant for bytecode that resolves a declared system by name
/// (`RunSchedule`, `RunSystem`, and any other opcode that loads a system from the constant pool).
pub(crate) fn constant_resolved_system_name(chunk: &Chunk, idx: usize) -> Result<String, String> {
    match chunk.constants.get(idx) {
        Some(v) => {
            if let Some(s) = v.as_str() {
                return Ok(s.to_string());
            }
            if let Some(s) = v.as_system_ref() {
                return Ok(s.to_string());
            }
            Err(format!(
                "Expected string or system reference constant at index {}, got {}",
                idx,
                v.type_name()
            ))
        }
        None => Err(format!("Invalid constant index {}", idx)),
    }
}

/// Resolve a system-name constant without allocating an owned string.
///
/// `Value` is a copy handle into the immutable constant heap, so callers may
/// extract the name from this local while mutably executing the VM.
pub(crate) fn constant_resolved_system_name_value(
    chunk: &Chunk,
    idx: usize,
) -> Result<Value, String> {
    let value = constant_value(chunk, idx)?;
    if value.as_str().is_some() || value.as_system_ref().is_some() {
        Ok(value)
    } else {
        Err(format!(
            "Expected string or system reference constant at index {}, got {}",
            idx,
            value.type_name()
        ))
    }
}

pub(crate) fn entity_id(v: &Value) -> Result<u32, String> {
    v.as_entity_id()
        .ok_or_else(|| format!("Expected entity id, got {}", v.type_name()))
}

pub(crate) fn index_as_usize(v: &Value) -> Result<usize, String> {
    match v.as_int() {
        Some(i) if i >= 0 => Ok(i as usize),
        Some(_) => Err("Negative index".to_string()),
        None => Err(format!("Index must be int, got {}", v.type_name())),
    }
}

fn native_binary(
    gc: &mut GcHeap,
    a: &Value,
    b: &Value,
    op: crate::native_types::NativeBinaryOp,
) -> Option<Result<Value, String>> {
    match (a.as_native_scalar(), b.as_native_scalar()) {
        (Some(left), Some(right)) => Some(
            crate::native_types::checked_native_binary(left, right, op)
                .map(|value| Value::from_native_scalar(gc, value)),
        ),
        (Some(_), None) | (None, Some(_)) => Some(Err(format!(
            "native operation requires explicit matching conversions, got {} and {}",
            a.type_name(),
            b.type_name()
        ))),
        (None, None) => None,
    }
}

pub(crate) fn binary_add(gc: &mut GcHeap, a: Value, b: Value) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::Add) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        let result = i
            .checked_add(j)
            .ok_or_else(|| format!("Integer overflow: {} + {}", i, j))?;
        return Ok(Value::from_int(gc, result));
    }
    let af = a.as_int().map(|i| i as f64).or(a.as_float());
    let bf = b.as_int().map(|i| i as f64).or(b.as_float());
    if let (Some(x), Some(y)) = (af, bf) {
        return Ok(Value::from_float(x + y));
    }
    if let (Some(sa), Some(sb)) = (a.as_str(), b.as_str()) {
        // One exact-capacity buffer. The old path copied the accumulator
        // THREE times per concat (Arc<str> -> String, push_str's realloc of
        // the exact-sized buffer, String -> Arc) — the constant factor under
        // the O(n²) `s = s + x` loop (Tier-1 #2).
        let mut s = String::with_capacity(sa.len() + sb.len());
        s.push_str(sa);
        s.push_str(sb);
        return Ok(Value::from_string(gc, s));
    }
    if a.as_list().is_some() && b.as_list().is_some() {
        let mut new = a.into_rad_list().unwrap();
        new.extend_from(b.as_list().unwrap());
        return Ok(Value::from_rad_list(gc, new));
    }
    if let Some(out) = tuple_elementwise(gc, &a, &b, binary_add)? {
        return Ok(out);
    }
    Err(format!(
        "Cannot add {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

/// Element-wise tuple math — the vector dialect. `(a, b) + (c, d)` is
/// `(a+c, b+d)`; a scalar on either side broadcasts: `dir * speed`,
/// `2.0 * v`, `v / n`. Arity mismatches are loud errors.
fn tuple_elementwise<F>(
    gc: &mut GcHeap,
    a: &Value,
    b: &Value,
    mut op: F,
) -> Result<Option<Value>, String>
where
    F: FnMut(&mut GcHeap, Value, Value) -> Result<Value, String>,
{
    let a_items = a.as_tuple().cloned();
    let b_items = b.as_tuple().cloned();
    match (a_items, b_items) {
        (Some(xs), Some(ys)) => {
            if xs.len() != ys.len() {
                return Err(format!(
                    "Tuple arity mismatch: ({}) vs ({}) elements",
                    xs.len(),
                    ys.len()
                ));
            }
            let mut out = Vec::with_capacity(xs.len());
            for (x, y) in xs.into_iter().zip(ys) {
                out.push(op(gc, x, y)?);
            }
            Ok(Some(Value::tuple(gc, out)))
        }
        (Some(xs), None) if b.as_int().is_some() || b.as_float().is_some() => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(op(gc, x, *b)?);
            }
            Ok(Some(Value::tuple(gc, out)))
        }
        (None, Some(ys)) if a.as_int().is_some() || a.as_float().is_some() => {
            let mut out = Vec::with_capacity(ys.len());
            for y in ys {
                out.push(op(gc, *a, y)?);
            }
            Ok(Some(Value::tuple(gc, out)))
        }
        _ => Ok(None),
    }
}

pub(crate) fn binary_sub(gc: &mut GcHeap, a: Value, b: Value) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::Sub) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        let result = i
            .checked_sub(j)
            .ok_or_else(|| format!("Integer overflow: {} - {}", i, j))?;
        return Ok(Value::from_int(gc, result));
    }
    let af = a.as_int().map(|i| i as f64).or(a.as_float());
    let bf = b.as_int().map(|i| i as f64).or(b.as_float());
    if let (Some(x), Some(y)) = (af, bf) {
        return Ok(Value::from_float(x - y));
    }
    if let Some(out) = tuple_elementwise(gc, &a, &b, binary_sub)? {
        return Ok(out);
    }
    Err(format!(
        "Cannot subtract {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn binary_mul(
    gc: &mut GcHeap,
    a: Value,
    b: Value,
    allocation_limit: usize,
) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::Mul) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        let result = i
            .checked_mul(j)
            .ok_or_else(|| format!("Integer overflow: {} * {}", i, j))?;
        return Ok(Value::from_int(gc, result));
    }
    let af = a.as_int().map(|i| i as f64).or(a.as_float());
    let bf = b.as_int().map(|i| i as f64).or(b.as_float());
    if let (Some(x), Some(y)) = (af, bf) {
        return Ok(Value::from_float(x * y));
    }
    if let (Some(s), Some(n)) = (a.as_str(), b.as_int()) {
        if n < 0 {
            return Ok(Value::from_string(gc, String::new()));
        }
        let bytes = s.len().checked_mul(n as usize).ok_or_else(|| {
            "Budget exhausted: memory limit exceeded by string repetition".to_string()
        })?;
        if gc.bytes_allocated().saturating_add(bytes) > allocation_limit {
            return Err("Budget exhausted: memory limit exceeded by string repetition".into());
        }
        return Ok(Value::from_string(gc, s.repeat(n as usize)));
    }
    if let (Some(n), Some(s)) = (a.as_int(), b.as_str()) {
        if n < 0 {
            return Ok(Value::from_string(gc, String::new()));
        }
        let bytes = s.len().checked_mul(n as usize).ok_or_else(|| {
            "Budget exhausted: memory limit exceeded by string repetition".to_string()
        })?;
        if gc.bytes_allocated().saturating_add(bytes) > allocation_limit {
            return Err("Budget exhausted: memory limit exceeded by string repetition".into());
        }
        return Ok(Value::from_string(gc, s.repeat(n as usize)));
    }
    if let Some(out) = tuple_elementwise(gc, &a, &b, |gc, left, right| {
        binary_mul(gc, left, right, allocation_limit)
    })? {
        return Ok(out);
    }
    Err(format!(
        "Cannot multiply {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn binary_div(gc: &mut GcHeap, a: Value, b: Value) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::Div) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        if j == 0 {
            return Err("Division by zero".into());
        }
        let result = i
            .checked_div(j)
            .ok_or_else(|| format!("Integer overflow: {} / {}", i, j))?;
        return Ok(Value::from_int(gc, result));
    }
    let af = a.as_int().map(|i| i as f64).or(a.as_float());
    let bf = b.as_int().map(|i| i as f64).or(b.as_float());
    if let (Some(x), Some(y)) = (af, bf) {
        if y == 0.0 {
            return Err("Division by zero".into());
        }
        return Ok(Value::from_float(x / y));
    }
    if let Some(out) = tuple_elementwise(gc, &a, &b, binary_div)? {
        return Ok(out);
    }
    Err(format!(
        "Cannot divide {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn binary_mod(gc: &mut GcHeap, a: Value, b: Value) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::Mod) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        if j == 0 {
            return Err("Modulo by zero".into());
        }
        let result = i
            .checked_rem(j)
            .ok_or_else(|| format!("Integer overflow: {} % {}", i, j))?;
        return Ok(Value::from_int(gc, result));
    }
    let af = a.as_int().map(|i| i as f64).or(a.as_float());
    let bf = b.as_int().map(|i| i as f64).or(b.as_float());
    if let (Some(x), Some(y)) = (af, bf) {
        if y == 0.0 {
            return Err("Modulo by zero".into());
        }
        return Ok(Value::from_float(x % y));
    }
    Err(format!(
        "Cannot mod {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn binary_bitand(gc: &mut GcHeap, a: Value, b: Value) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::BitAnd) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        return Ok(Value::from_int(gc, i & j));
    }
    Err(format!(
        "Bitwise & requires int operands, got {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn binary_bitor(gc: &mut GcHeap, a: Value, b: Value) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::BitOr) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        return Ok(Value::from_int(gc, i | j));
    }
    Err(format!(
        "Bitwise | requires int operands, got {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn binary_bitxor(gc: &mut GcHeap, a: Value, b: Value) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::BitXor) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        return Ok(Value::from_int(gc, i ^ j));
    }
    Err(format!(
        "Bitwise ^ requires int operands, got {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn binary_shl(gc: &mut GcHeap, a: Value, b: Value) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::Shl) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        let out = if !(0..64).contains(&j) {
            0
        } else {
            ((i as u64) << j) as i64
        };
        return Ok(Value::from_int(gc, out));
    }
    Err(format!(
        "Shift << requires int operands, got {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn binary_shr(gc: &mut GcHeap, a: Value, b: Value) -> Result<Value, String> {
    if let Some(result) = native_binary(gc, &a, &b, crate::native_types::NativeBinaryOp::Shr) {
        return result;
    }
    if let (Some(i), Some(j)) = (a.as_int(), b.as_int()) {
        let out = if !(0..64).contains(&j) {
            0
        } else {
            ((i as u64) >> j) as i64
        };
        return Ok(Value::from_int(gc, out));
    }
    Err(format!(
        "Shift >> requires int operands, got {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

/// Total order over comparable values: numbers (int/float mixed), strings,
/// bools, and TUPLES — compared lexicographically, element by element, so
/// multi-key ranking is just a tuple key: `min_by(fn(t) { return (-rung, d) })`.
pub(crate) fn compare_values(a: &Value, b: &Value) -> Result<std::cmp::Ordering, String> {
    use std::cmp::Ordering;
    if let (Some(left), Some(right)) = (a.as_native_scalar(), b.as_native_scalar()) {
        if left.type_name != right.type_name {
            return Err(format!(
                "Cannot compare {} and {}",
                left.type_name, right.type_name
            ));
        }
        return if let (Some(x), Some(y)) = (left.float(), right.float()) {
            Ok(x.partial_cmp(&y).unwrap_or(Ordering::Equal))
        } else if left.repr.is_signed() {
            Ok(left.signed().unwrap().cmp(&right.signed().unwrap()))
        } else {
            Ok(left.unsigned().unwrap().cmp(&right.unsigned().unwrap()))
        };
    }
    if let (Some(x), Some(y)) = (a.as_int(), b.as_int()) {
        return Ok(x.cmp(&y));
    }
    let af = a.as_int().map(|i| i as f64).or(a.as_float());
    let bf = b.as_int().map(|i| i as f64).or(b.as_float());
    if let (Some(x), Some(y)) = (af, bf) {
        return Ok(x.partial_cmp(&y).unwrap_or(Ordering::Equal));
    }
    if let (Some(x), Some(y)) = (a.as_str(), b.as_str()) {
        return Ok(x.cmp(y));
    }
    if let (Some(x), Some(y)) = (a.as_bool(), b.as_bool()) {
        return Ok(x.cmp(&y));
    }
    // ascending eid: the canonical determinism order for entity sweeps
    if let (Some(x), Some(y)) = (a.as_entity_id(), b.as_entity_id()) {
        return Ok(x.cmp(&y));
    }
    if let (Some(xs), Some(ys)) = (a.as_tuple(), b.as_tuple()) {
        if xs.len() != ys.len() {
            return Err(format!(
                "Cannot order tuples of different arity: ({}) vs ({}) elements",
                xs.len(),
                ys.len()
            ));
        }
        for (x, y) in xs.iter().zip(ys.iter()) {
            let ord = compare_values(x, y)?;
            if ord != Ordering::Equal {
                return Ok(ord);
            }
        }
        return Ok(Ordering::Equal);
    }
    Err(format!(
        "Cannot order {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn unary_bitnot(gc: &mut GcHeap, v: Value) -> Result<Value, String> {
    if let Some(native) = v.as_native_scalar() {
        let result = crate::native_types::checked_native_bit_not(native)?;
        return Ok(Value::from_native_scalar(gc, result));
    }
    if let Some(i) = v.as_int() {
        return Ok(Value::from_int(gc, !i));
    }
    Err(format!(
        "Bitwise ~ requires an int operand, got {}",
        v.type_name()
    ))
}

pub(crate) fn unary_neg(gc: &mut GcHeap, v: Value) -> Result<Value, String> {
    if let Some(native) = v.as_native_scalar() {
        let result = crate::native_types::checked_native_neg(native)?;
        return Ok(Value::from_native_scalar(gc, result));
    }
    if let Some(items) = v.as_tuple().cloned() {
        let mut out = Vec::with_capacity(items.len());
        for x in items {
            out.push(unary_neg(gc, x)?);
        }
        return Ok(Value::tuple(gc, out));
    }
    if let Some(i) = v.as_int() {
        let result = i
            .checked_neg()
            .ok_or_else(|| format!("Integer overflow: -{}", i))?;
        return Ok(Value::from_int(gc, result));
    }
    if let Some(x) = v.as_float() {
        return Ok(Value::from_float(-x));
    }
    Err(format!("Cannot negate {}", v.type_name()))
}

pub(crate) fn cmp_lt(a: &Value, b: &Value) -> Result<bool, String> {
    Ok(matches!(
        partial_compare_values(a, b)?,
        Some(std::cmp::Ordering::Less)
    ))
}

pub(crate) fn cmp_gt(a: &Value, b: &Value) -> Result<bool, String> {
    Ok(matches!(
        partial_compare_values(a, b)?,
        Some(std::cmp::Ordering::Greater)
    ))
}

pub(crate) fn cmp_lte(a: &Value, b: &Value) -> Result<bool, String> {
    Ok(matches!(
        partial_compare_values(a, b)?,
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    ))
}

pub(crate) fn cmp_gte(a: &Value, b: &Value) -> Result<bool, String> {
    Ok(matches!(
        partial_compare_values(a, b)?,
        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
    ))
}

/// Language comparison semantics. Unlike sorting/index order, floating-point
/// comparisons are partial: any comparison involving NaN is false.
fn partial_compare_values(a: &Value, b: &Value) -> Result<Option<std::cmp::Ordering>, String> {
    use std::cmp::Ordering;
    if let (Some(left), Some(right)) = (a.as_native_scalar(), b.as_native_scalar()) {
        if left.type_name != right.type_name {
            return Err(format!(
                "Cannot compare {} and {}",
                left.type_name, right.type_name
            ));
        }
        return if let (Some(left), Some(right)) = (left.float(), right.float()) {
            Ok(left.partial_cmp(&right))
        } else if left.repr.is_signed() {
            Ok(Some(left.signed().unwrap().cmp(&right.signed().unwrap())))
        } else {
            Ok(Some(
                left.unsigned().unwrap().cmp(&right.unsigned().unwrap()),
            ))
        };
    }
    if let (Some(left), Some(right)) = (a.as_int(), b.as_int()) {
        return Ok(Some(left.cmp(&right)));
    }
    let left_number = a.as_int().map(|value| value as f64).or(a.as_float());
    let right_number = b.as_int().map(|value| value as f64).or(b.as_float());
    if let (Some(left), Some(right)) = (left_number, right_number) {
        return Ok(left.partial_cmp(&right));
    }
    if let (Some(left), Some(right)) = (a.as_str(), b.as_str()) {
        return Ok(Some(left.cmp(right)));
    }
    if let (Some(left), Some(right)) = (a.as_bool(), b.as_bool()) {
        return Ok(Some(left.cmp(&right)));
    }
    if let (Some(left), Some(right)) = (a.as_entity_id(), b.as_entity_id()) {
        return Ok(Some(left.cmp(&right)));
    }
    if let (Some(left), Some(right)) = (a.as_tuple(), b.as_tuple()) {
        if left.len() != right.len() {
            return Err(format!(
                "Cannot compare tuples of different arity: ({}) vs ({}) elements",
                left.len(),
                right.len()
            ));
        }
        for (left, right) in left.iter().zip(right.iter()) {
            match partial_compare_values(left, right)? {
                Some(Ordering::Equal) => {}
                other => return Ok(other),
            }
        }
        return Ok(Some(Ordering::Equal));
    }
    Err(format!(
        "Cannot compare {} and {}",
        a.type_name(),
        b.type_name()
    ))
}

pub(crate) fn values_equal(a: &Value, b: &Value) -> bool {
    if let (Some(i), Some(f)) = (a.as_int(), b.as_float()) {
        return (i as f64) == f;
    }
    if let (Some(f), Some(i)) = (a.as_float(), b.as_int()) {
        return f == (i as f64);
    }
    a == b
}
