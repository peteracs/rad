fn float_to_i64(value: f64) -> Option<i64> {
    // i64::MAX is not exactly representable as f64; its cast rounds to 2^63.
    // Use a strict upper bound so conversion never relies on Rust's saturating
    // float-to-int cast at that edge.
    const I64_EXCLUSIVE_MAX_AS_F64: f64 = 9_223_372_036_854_775_808.0;
    const I64_MIN_AS_F64: f64 = -9_223_372_036_854_775_808.0;
    (I64_MIN_AS_F64..I64_EXCLUSIVE_MAX_AS_F64)
        .contains(&value)
        .then_some(value as i64)
}

fn native_to_i64(native: &crate::native_types::NativeScalarValue) -> Option<i64> {
    use crate::native_types::NativeScalarKind;

    match native.repr {
        NativeScalarKind::U64 => i64::try_from(native.bits).ok(),
        NativeScalarKind::F32 | NativeScalarKind::F64 => {
            native.float().and_then(float_to_i64)
        }
        _ => native.signed(),
    }
}

fn native_to_f64(native: &crate::native_types::NativeScalarValue) -> f64 {
    if let Some(value) = native.float() {
        value
    } else if native.repr.is_signed() {
        native.signed().expect("signed native scalar") as f64
    } else {
        native.unsigned().expect("unsigned native scalar") as f64
    }
}

pub(crate) fn bi_int(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("int() requires 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        Ok(Value::from_int(gc, n))
    } else if let Some(native) = args[0].as_native_scalar() {
        let converted = native_to_i64(native).ok_or_else(|| {
            format!(
                "Cannot convert {} to int: value is out of i64 range or non-finite",
                native.type_name
            )
        })?;
        Ok(Value::from_int(gc, converted))
    } else if let Some(x) = args[0].as_float() {
        if x.is_nan() {
            return Err("Cannot convert NaN to int".into());
        }
        float_to_i64(x).map(|value| Value::from_int(gc, value)).ok_or_else(|| {
            format!("Cannot convert {} to int: value out of i64 range", x)
        })
    } else if let Some(s) = args[0].as_str() {
        s.parse::<i64>()
            .map(|n| Value::from_int(gc, n))
            .map_err(|_| format!("Cannot convert '{}' to int", s))
    } else if let Some(b) = args[0].as_bool() {
        Ok(Value::from_int(gc, if b { 1 } else { 0 }))
    } else {
        Err(format!("Cannot convert {} to int", args[0].type_name()))
    }
}

pub(crate) fn bi_float(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("float() requires 1 argument".into());
    }
    if let Some(x) = args[0].as_float() {
        Ok(Value::from_float(x))
    } else if let Some(native) = args[0].as_native_scalar() {
        Ok(Value::from_float(native_to_f64(native)))
    } else if let Some(n) = args[0].as_int() {
        Ok(Value::from_float(n as f64))
    } else if let Some(s) = args[0].as_str() {
        s.parse::<f64>()
            .map(Value::from_float)
            .map_err(|_| format!("Cannot convert '{}' to float", s))
    } else {
        Err(format!("Cannot convert {} to float", args[0].type_name()))
    }
}

pub(crate) fn bi_try_int(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("try_int() requires 1 argument".into());
    }
    let result = if let Some(value) = args[0].as_int() {
        Some(Value::from_int(gc, value))
    } else if let Some(value) = args[0].as_native_scalar() {
        native_to_i64(value).map(|value| Value::from_int(gc, value))
    } else if let Some(value) = args[0].as_float() {
        float_to_i64(value).map(|value| Value::from_int(gc, value))
    } else if let Some(value) = args[0].as_str() {
        value.parse::<i64>().ok().map(|value| Value::from_int(gc, value))
    } else {
        args[0]
            .as_bool()
            .map(|value| Value::from_int(gc, if value { 1 } else { 0 }))
    };
    Ok(wrap_option(gc, result))
}

pub(crate) fn bi_try_float(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("try_float() requires 1 argument".into());
    }
    let result = if let Some(value) = args[0].as_float() {
        Some(Value::from_float(value))
    } else if let Some(value) = args[0].as_native_scalar() {
        Some(Value::from_float(native_to_f64(value)))
    } else if let Some(value) = args[0].as_int() {
        Some(Value::from_float(value as f64))
    } else if let Some(value) = args[0].as_str() {
        value.parse::<f64>().ok().map(Value::from_float)
    } else {
        None
    };
    Ok(wrap_option(gc, result))
}
