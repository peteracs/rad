

pub(crate) fn bi_len(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("len() requires 1 argument".into());
    }
    if let Some(items) = args[0].as_list() {
        Ok(Value::from_int(gc, items.len() as i64))
    } else if let Some(t) = args[0].as_tuple() {
        Ok(Value::from_int(gc, t.len() as i64))
    } else if let Some(s) = args[0].as_str() {
        Ok(Value::from_int(gc, s.chars().count() as i64))
    } else if let Some(m) = args[0].as_map() {
        Ok(Value::from_int(gc, m.len() as i64))
    } else if let Some(bytes) = args[0].as_bytebuf() {
        Ok(Value::from_int(gc, bytes.len() as i64))
    } else {
        Err(format!("len() not defined for {}", args[0].type_name()))
    }
}

pub(crate) fn bi_typeof(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("typeof() requires 1 argument".into());
    }
    Ok(Value::from_string(gc, args[0].type_name().to_string()))
}

pub(crate) fn bi_str(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("str() requires 1 argument".into());
    }
    Ok(Value::from_string(gc, args[0].print_display()))
}

pub(crate) fn bi_int(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("int() requires 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        Ok(Value::from_int(gc, n))
    } else if let Some(x) = args[0].as_float() {
        if x.is_nan() {
            return Err("Cannot convert NaN to int".into());
        }
        if x.is_infinite() || x > i64::MAX as f64 || x < i64::MIN as f64 {
            return Err(format!(
                "Cannot convert {} to int: value out of i64 range",
                x
            ));
        }
        Ok(Value::from_int(gc, x as i64))
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

pub(crate) fn bi_int_div(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("int_div() requires exactly 2 arguments".into());
    }
    let a = args[0].as_int().ok_or_else(|| {
        format!(
            "int_div() first argument must be int, got {}",
            args[0].type_name()
        )
    })?;
    let b = args[1].as_int().ok_or_else(|| {
        format!(
            "int_div() second argument must be int, got {}",
            args[1].type_name()
        )
    })?;
    if b == 0 {
        return Err("Division by zero".into());
    }
    let result = a
        .checked_div(b)
        .ok_or_else(|| format!("Integer overflow: {} / {}", a, b))?;
    Ok(Value::from_int(gc, result))
}

pub(crate) fn bi_abs(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("abs() requires 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        let result = n
            .checked_abs()
            .ok_or_else(|| format!("Integer overflow: abs({})", n))?;
        Ok(Value::from_int(gc, result))
    } else if let Some(x) = args[0].as_float() {
        Ok(Value::from_float(x.abs()))
    } else {
        Err(format!("abs() not defined for {}", args[0].type_name()))
    }
}

pub(crate) fn bi_sign(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("sign() requires 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        Ok(Value::from_int(gc, n.signum()))
    } else if let Some(x) = args[0].as_float() {
        // Math.sign semantics: 0.0 and NaN map to 0.0, not ±1
        let s = if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        };
        Ok(Value::from_float(s))
    } else {
        Err(format!("sign() not defined for {}", args[0].type_name()))
    }
}

fn int_arg(args: &[Value], idx: usize, fname: &str) -> Result<i64, String> {
    let v = args
        .get(idx)
        .ok_or_else(|| format!("{}() missing argument {}", fname, idx + 1))?;
    v.as_int()
        .ok_or_else(|| format!("{}() expects an int, got {}", fname, v.type_name()))
}

/// `popcount(x) -> int` — number of set bits (bitboard workloads).
pub(crate) fn bi_popcount(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let n = int_arg(&args, 0, "popcount")?;
    Ok(Value::from_int(gc, n.count_ones() as i64))
}

/// `ctz(x) -> int` — index of the lowest set bit (64 when x == 0).
pub(crate) fn bi_ctz(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let n = int_arg(&args, 0, "ctz")?;
    Ok(Value::from_int(gc, n.trailing_zeros() as i64))
}

/// `shl(x, n) -> int` — logical shift left; n outside 0..63 returns 0.
pub(crate) fn bi_shl(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let x = int_arg(&args, 0, "shl")?;
    let n = int_arg(&args, 1, "shl")?;
    let out = if !(0..64).contains(&n) {
        0
    } else {
        ((x as u64) << n) as i64
    };
    Ok(Value::from_int(gc, out))
}

/// `filled(n, v) -> list` — a list of `n` copies of `v`, built natively.
/// The interpreted equivalent (`for _ in range(n) { xs << v }`) pays the
/// dispatch loop per element — a real tax on solver-style scratch buffers.
pub(crate) fn bi_filled(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let n = int_arg(&args, 0, "filled")?;
    if n < 0 {
        return Err(format!("filled() length must be non-negative, got {}", n));
    }
    let v = *args
        .get(1)
        .ok_or_else(|| "filled() missing argument 2".to_string())?;
    let items = vec![v; n as usize];
    Ok(Value::from_rad_list(gc, crate::value::RadList::new(items)))
}

/// `shr(x, n) -> int` — logical shift right; n outside 0..63 returns 0.
pub(crate) fn bi_shr(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let x = int_arg(&args, 0, "shr")?;
    let n = int_arg(&args, 1, "shr")?;
    let out = if !(0..64).contains(&n) {
        0
    } else {
        ((x as u64) >> n) as i64
    };
    Ok(Value::from_int(gc, out))
}

fn number_arg(args: &[Value], idx: usize, fname: &str) -> Result<f64, String> {
    let v = args
        .get(idx)
        .ok_or_else(|| format!("{}() missing argument {}", fname, idx + 1))?;
    if let Some(n) = v.as_int() {
        Ok(n as f64)
    } else if let Some(x) = v.as_float() {
        Ok(x)
    } else {
        Err(format!(
            "{}() expects a number, got {}",
            fname,
            v.type_name()
        ))
    }
}

fn float_to_int_result(gc: &mut GcHeap, r: f64, fname: &str) -> Result<Value, String> {
    if !r.is_finite() {
        return Err(format!("{}() result is not finite", fname));
    }
    if r < i64::MIN as f64 || r > i64::MAX as f64 {
        return Err(format!("{}() result out of int range: {}", fname, r));
    }
    Ok(Value::from_int(gc, r as i64))
}

pub(crate) fn bi_round(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("round() requires exactly 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        return Ok(Value::from_int(gc, n));
    }
    // f64::round = half away from zero (correct for -0.5 cases, unlike int(x + 0.5))
    let x = number_arg(&args, 0, "round")?;
    float_to_int_result(gc, x.round(), "round")
}

pub(crate) fn bi_floor(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("floor() requires exactly 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        return Ok(Value::from_int(gc, n));
    }
    let x = number_arg(&args, 0, "floor")?;
    float_to_int_result(gc, x.floor(), "floor")
}

pub(crate) fn bi_ceil(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("ceil() requires exactly 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        return Ok(Value::from_int(gc, n));
    }
    let x = number_arg(&args, 0, "ceil")?;
    float_to_int_result(gc, x.ceil(), "ceil")
}

pub(crate) fn bi_sqrt(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("sqrt() requires exactly 1 argument".into());
    }
    let x = number_arg(&args, 0, "sqrt")?;
    if x < 0.0 {
        return Err(format!("sqrt() of negative number: {}", x));
    }
    Ok(Value::from_float(x.sqrt()))
}

pub(crate) fn bi_pow(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("pow() requires exactly 2 arguments".into());
    }
    // int ^ non-negative int stays an int (with overflow check)
    if let (Some(base), Some(exp)) = (args[0].as_int(), args[1].as_int()) {
        if exp >= 0 {
            let exp_u32 = u32::try_from(exp)
                .map_err(|_| format!("pow() integer exponent too large: {}", exp))?;
            let result = base
                .checked_pow(exp_u32)
                .ok_or_else(|| format!("Integer overflow: pow({}, {})", base, exp))?;
            return Ok(Value::from_int(gc, result));
        }
    }
    let base = number_arg(&args, 0, "pow")?;
    let exp = number_arg(&args, 1, "pow")?;
    Ok(Value::from_float(base.powf(exp)))
}

pub(crate) fn bi_to_fixed(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("to_fixed() requires exactly 2 arguments".into());
    }
    let x = number_arg(&args, 0, "to_fixed")?;
    let digits = args[1]
        .as_int()
        .ok_or_else(|| format!("to_fixed() digits must be int, got {}", args[1].type_name()))?;
    if !(0..=17).contains(&digits) {
        return Err(format!("to_fixed() digits must be 0..=17, got {}", digits));
    }
    Ok(Value::from_string(gc, format!("{:.*}", digits as usize, x)))
}

const JSON_MAX_DEPTH: usize = 128;

pub(crate) fn value_to_json(v: &Value, depth: usize) -> Result<serde_json::Value, String> {
    if depth > JSON_MAX_DEPTH {
        return Err("json_stringify() exceeded max nesting depth".into());
    }
    if v.is_nil() {
        return Ok(serde_json::Value::Null);
    }
    if let Some(b) = v.as_bool() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Some(n) = v.as_int() {
        return Ok(serde_json::Value::Number(n.into()));
    }
    if let Some(x) = v.as_float() {
        return serde_json::Number::from_f64(x)
            .map(serde_json::Value::Number)
            .ok_or_else(|| format!("json_stringify() cannot encode non-finite float: {}", x));
    }
    if let Some(s) = v.as_str() {
        return Ok(serde_json::Value::String(s.to_string()));
    }
    if let Some(items) = v.as_list() {
        let mut arr = Vec::with_capacity(items.len());
        for item in items.iter() {
            arr.push(value_to_json(item, depth + 1)?);
        }
        return Ok(serde_json::Value::Array(arr));
    }
    if let Some(m) = v.as_map() {
        let mut obj = serde_json::Map::with_capacity(m.len());
        let mut sorted_keys: Vec<&MapKey> = m.keys().collect();
        sorted_keys.sort();
        for k in sorted_keys {
            let key_str = match k {
                MapKey::Str(s) => s.clone(),
                MapKey::Int(i) => i.to_string(),
                MapKey::Bool(b) => b.to_string(),
                MapKey::Entity(e) => e.to_string(),
                // JSON object keys must be strings: "(1, 2)"
                MapKey::Tuple(_) => k.to_string(),
            };
            obj.insert(key_str, value_to_json(&m[k], depth + 1)?);
        }
        return Ok(serde_json::Value::Object(obj));
    }
    if let Some(c) = v.as_component() {
        let mut obj = serde_json::Map::with_capacity(c.layout.len());
        for (idx, field) in c.layout.iter().enumerate() {
            obj.insert(field.clone(), value_to_json(&c.values[idx], depth + 1)?);
        }
        return Ok(serde_json::Value::Object(obj));
    }
    if let Some(st) = v.as_sum_type() {
        let mut obj = serde_json::Map::with_capacity(st.fields.len() + 1);
        obj.insert(
            "$variant".to_string(),
            serde_json::Value::String(st.variant.clone()),
        );
        let mut keys: Vec<&String> = st.fields.keys().collect();
        keys.sort();
        for k in keys {
            obj.insert(k.clone(), value_to_json(&st.fields[k], depth + 1)?);
        }
        return Ok(serde_json::Value::Object(obj));
    }
    Err(format!("json_stringify() cannot encode {}", v.type_name()))
}

pub(crate) fn json_to_value(gc: &mut GcHeap, j: &serde_json::Value) -> Result<Value, String> {
    match j {
        serde_json::Value::Null => Ok(Value::NIL),
        serde_json::Value::Bool(b) => Ok(Value::from_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::from_int(gc, i))
            } else if let Some(x) = n.as_f64() {
                Ok(Value::from_float(x))
            } else {
                Err(format!("json_parse() unsupported number: {}", n))
            }
        }
        serde_json::Value::String(s) => Ok(Value::from_string(gc, s.clone())),
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(json_to_value(gc, item)?);
            }
            Ok(Value::list(gc, out))
        }
        serde_json::Value::Object(obj) => {
            let mut m = MapStorage::new();
            for (k, v) in obj {
                let val = json_to_value(gc, v)?;
                m.insert(MapKey::Str(k.clone()), val);
            }
            Ok(Value::map(gc, m))
        }
    }
}

pub(crate) fn bi_json_stringify(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("json_stringify() requires exactly 1 argument".into());
    }
    let json = value_to_json(&args[0], 0)?;
    let text =
        serde_json::to_string(&json).map_err(|e| format!("json_stringify() failed: {}", e))?;
    Ok(Value::from_string(gc, text))
}

pub(crate) fn bi_json_parse(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("json_parse() requires exactly 1 argument".into());
    }
    let text = args[0]
        .as_str()
        .ok_or_else(|| format!("json_parse() expects str, got {}", args[0].type_name()))?;
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(text);
    match parsed {
        Ok(j) => {
            let v = json_to_value(gc, &j)?;
            Ok(wrap_option(gc, Some(v)))
        }
        Err(_) => Ok(wrap_option(gc, None)),
    }
}

pub(crate) fn bi_min(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("min() requires 2 arguments".into());
    }
    match (
        args[0].as_int(),
        args[0].as_float(),
        args[1].as_int(),
        args[1].as_float(),
    ) {
        (Some(a), _, Some(b), _) => Ok(Value::from_int(gc, a.min(b))),
        (_, Some(a), _, Some(b)) => Ok(Value::from_float(a.min(b))),
        (Some(a), _, _, Some(b)) => Ok(Value::from_float((a as f64).min(b))),
        (_, Some(a), Some(b), _) => Ok(Value::from_float(a.min(b as f64))),
        _ => Err(format!(
            "min() not defined for {} and {}",
            args[0].type_name(),
            args[1].type_name()
        )),
    }
}

pub(crate) fn bi_max(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("max() requires 2 arguments".into());
    }
    match (
        args[0].as_int(),
        args[0].as_float(),
        args[1].as_int(),
        args[1].as_float(),
    ) {
        (Some(a), _, Some(b), _) => Ok(Value::from_int(gc, a.max(b))),
        (_, Some(a), _, Some(b)) => Ok(Value::from_float(a.max(b))),
        (Some(a), _, _, Some(b)) => Ok(Value::from_float((a as f64).max(b))),
        (_, Some(a), Some(b), _) => Ok(Value::from_float(a.max(b as f64))),
        _ => Err(format!(
            "max() not defined for {} and {}",
            args[0].type_name(),
            args[1].type_name()
        )),
    }
}

pub(crate) fn bi_unwrap(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("unwrap() requires 1 argument".into());
    }
    if let Some(st) = args[0].as_sum_type() {
        if st.type_name == "Option" && st.variant == "Some" {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if st.type_name == "Option" && st.variant == "None" {
            // unwrap() erases all context by design — teach the tools that
            // keep it: expect() for your own words, require() for component
            // reads (it names the entity and what it has), unwrap_or() when
            // a default is fine.
            return Err(
                "unwrap() called on Option::None\n  hint: expect(value, \"why\") attaches your own message; \
                 require(entity, Comp) names the entity and component when a read fails; \
                 unwrap_or(value, default) when missing is fine"
                    .to_string(),
            );
        }
        if st.type_name == "Result" && st.variant == "Ok" {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if st.type_name == "Result" && st.variant == "Err" {
            let msg = st
                .fields
                .get("message")
                .map(|v| v.print_display())
                .unwrap_or_default();
            return Err(format!(
                "unwrap() called on Result::Err: {}\n  hint: match on Ok/Err to handle the failure, or expect(value, \"why\") to rename it",
                msg
            ));
        }
    }
    Ok(args[0])
}

pub(crate) fn bi_expect(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("expect() requires 2 arguments".into());
    }
    let msg = args[1].print_display();
    if let Some(st) = args[0].as_sum_type() {
        if st.type_name == "Option" && st.variant == "Some" {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if st.type_name == "Option" && st.variant == "None" {
            return Err(format!("expect() failed: {}", msg));
        }
        if st.type_name == "Result" && st.variant == "Ok" {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if st.type_name == "Result" && st.variant == "Err" {
            return Err(format!("expect() failed: {}", msg));
        }
    }
    Ok(args[0])
}

pub(crate) fn bi_unwrap_or(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("unwrap_or() requires 2 arguments (option_or_result, default)".into());
    }
    if let Some(st) = args[0].as_sum_type() {
        if (st.type_name == "Option" && st.variant == "Some")
            || (st.type_name == "Result" && st.variant == "Ok")
        {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if (st.type_name == "Option" && st.variant == "None")
            || (st.type_name == "Result" && st.variant == "Err")
        {
            return Ok(args[1]);
        }
    }
    Ok(args[0])
}

pub(crate) fn bi_is_some(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("is_some() requires 1 argument".into());
    }
    if let Some(st) = args[0].as_sum_type() {
        if st.type_name == "Option" {
            return Ok(Value::from_bool(st.variant == "Some"));
        }
        if st.type_name == "Result" {
            return Ok(Value::from_bool(st.variant == "Ok"));
        }
    }
    Ok(Value::from_bool(false))
}

pub(crate) fn bi_is_none(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("is_none() requires 1 argument".into());
    }
    if let Some(st) = args[0].as_sum_type() {
        if st.type_name == "Option" {
            return Ok(Value::from_bool(st.variant == "None"));
        }
        if st.type_name == "Result" {
            return Ok(Value::from_bool(st.variant == "Err"));
        }
    }
    Ok(Value::from_bool(false))
}

/// `set_at(coll, key, v) -> list|map` — a copy of `coll` with `key`
/// replaced by `v` (CoW: cheap when uniquely owned). The expression
/// dual of the `coll[key] = v` statement and the lowering target for
/// indexed field updates in `update` blocks. Lists bounds-check (no
/// silent growth); maps insert-or-replace, exactly like `m[k] = v`.
pub(crate) fn bi_set_at(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("set_at() requires 3 arguments (list-or-map, key, value)".into());
    }
    let mut arg_iter = args.into_iter();
    let collection = arg_iter.next().unwrap();
    let idx = arg_iter.next().unwrap();
    let value = arg_iter.next().unwrap();
    let got = collection.type_name();
    if collection.as_list().is_some() {
        let Some(i) = idx.as_int() else {
            return Err(format!(
                "set_at() list index must be int, got {}",
                idx.type_name()
            ));
        };
        let mut items = collection.into_rad_list().unwrap();
        let len = items.len() as i64;
        if i < 0 || i >= len {
            return Err(format!(
                "set_at() index {} out of bounds for list of length {}",
                i, len
            ));
        }
        items.set(i as usize, value)?;
        Ok(Value::from_rad_list(gc, items))
    } else if collection.as_map().is_some() {
        let map_key = crate::value::MapKey::from_value(&idx)?;
        let mut new_map = collection.into_map().unwrap();
        new_map.insert(map_key, value);
        Ok(Value::map(gc, new_map))
    } else {
        Err(format!("set_at() expects a list or map, got {}", got))
    }
}

/// `sum(xs)` / `product(xs)` — numeric folds, the missing halves of every
/// stat pipeline (`mods |> map(.flat) |> sum`). Ints stay ints; any float
/// in the list promotes the result. Empty list: sum 0, product 1.
fn numeric_fold(
    gc: &mut GcHeap,
    args: Vec<Value>,
    name: &str,
    int_init: i64,
    int_op: fn(i64, i64) -> i64,
    float_op: fn(f64, f64) -> f64,
) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "{}() requires 1 argument (a list of numbers)",
            name
        ));
    }
    let Some(items) = args[0].as_list() else {
        return Err(format!(
            "{}() expects a list, got {}",
            name,
            args[0].type_name()
        ));
    };
    let mut acc_i = int_init;
    let mut acc_f = int_init as f64;
    let mut is_float = false;
    for v in items.iter() {
        if let Some(i) = v.as_int() {
            acc_i = int_op(acc_i, i);
            acc_f = float_op(acc_f, i as f64);
        } else if let Some(f) = v.as_float() {
            is_float = true;
            acc_f = float_op(acc_f, f);
        } else {
            return Err(format!(
                "{}() expects numeric elements, got {}",
                name,
                v.type_name()
            ));
        }
    }
    if is_float {
        Ok(Value::from_float(acc_f))
    } else {
        Ok(Value::from_int(gc, acc_i))
    }
}

/// `get_or(coll, key, default)` — map lookup or list index with a fallback
/// instead of nil/bounds-error. The shape of every cooldown/stat table read.
pub(crate) fn bi_get_or(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("get_or() requires 3 arguments (collection, key, default)".into());
    }
    let coll = &args[0];
    let key = &args[1];
    let default = args[2];
    if let Some(m) = coll.as_map() {
        let map_key = crate::value::MapKey::from_value(key)?;
        return Ok(m.get(&map_key).copied().unwrap_or(default));
    }
    if let Some(xs) = coll.as_list() {
        let Some(i) = key.as_int() else {
            return Err(format!(
                "get_or() list index must be int, got {}",
                key.type_name()
            ));
        };
        if i < 0 || i as usize >= xs.len() {
            return Ok(default);
        }
        return Ok(*xs.get(i as usize).unwrap_or(&default));
    }
    Err(format!(
        "get_or() expects a map or list, got {}",
        coll.type_name()
    ))
}

/// `index_of(xs, v) -> int` — first index holding `v`, or -1. Returns an
/// int (not an Option) because the consumer is slot arithmetic
/// (`if at >= 0 { set_at(slots, at, ...) }`), and -1 composes with it.
pub(crate) fn bi_index_of(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("index_of() requires 2 arguments (list, value)".into());
    }
    let Some(xs) = args[0].as_list() else {
        return Err(format!(
            "index_of() expects a list, got {}",
            args[0].type_name()
        ));
    };
    for (i, v) in xs.iter().enumerate() {
        if helpers::values_equal(v, &args[1]) {
            return Ok(Value::from_int(gc, i as i64));
        }
    }
    Ok(Value::from_int(gc, -1))
}

/// `clamp(x, lo, hi)` — pin a number to a range. Ints stay int when all
/// three are ints; any float promotes.
pub(crate) fn bi_clamp(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("clamp() requires 3 arguments (value, lo, hi)".into());
    }
    if let (Some(x), Some(lo), Some(hi)) = (args[0].as_int(), args[1].as_int(), args[2].as_int()) {
        if lo > hi {
            return Err(format!("clamp() lo {} exceeds hi {}", lo, hi));
        }
        return Ok(Value::from_int(gc, x.max(lo).min(hi)));
    }
    let as_f = |v: &Value| v.as_int().map(|i| i as f64).or(v.as_float());
    if let (Some(x), Some(lo), Some(hi)) = (as_f(&args[0]), as_f(&args[1]), as_f(&args[2])) {
        if lo > hi {
            return Err(format!("clamp() lo {} exceeds hi {}", lo, hi));
        }
        return Ok(Value::from_float(x.max(lo).min(hi)));
    }
    Err(format!(
        "clamp() expects numbers, got {}, {}, {}",
        args[0].type_name(),
        args[1].type_name(),
        args[2].type_name()
    ))
}

pub(crate) fn bi_sum(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    numeric_fold(gc, args, "sum", 0, |a, b| a.wrapping_add(b), |a, b| a + b)
}

pub(crate) fn bi_product(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    numeric_fold(
        gc,
        args,
        "product",
        1,
        |a, b| a.wrapping_mul(b),
        |a, b| a * b,
    )
}

pub(crate) fn bi_push(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("push() requires 2 arguments".into());
    }
    let mut arg_iter = args.into_iter();
    let collection = arg_iter.next().unwrap();
    let item = arg_iter.next().unwrap();
    let got = collection.type_name();

    if collection.as_list().is_some() {
        let mut items = collection.into_rad_list().unwrap();
        items.push(item);
        Ok(Value::from_rad_list(gc, items))
    } else if collection.as_str().is_some() {
        let mut s = collection.into_string().unwrap();
        if let Some(item_str) = item.as_str() {
            s.push_str(item_str);
            Ok(Value::from_string(gc, s))
        } else {
            Err(format!(
                "push() on string expects string argument, got {}",
                item.type_name()
            ))
        }
    } else {
        Err(format!("push() expects list or string, got {}", got))
    }
}

pub(crate) fn bi_pop(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("pop() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    if let Some(items) = arg.as_list() {
        if items.is_empty() {
            return Err("pop() on empty list".to_string());
        }
        Ok(*items.last().unwrap())
    } else if let Some(s) = arg.as_str() {
        if s.is_empty() {
            return Err("pop() on empty string".to_string());
        }
        Ok(Value::from_string(
            gc,
            s.chars().last().unwrap().to_string(),
        ))
    } else {
        Err(format!(
            "pop() expects list or string, got {}",
            arg.type_name()
        ))
    }
}

pub(crate) fn bi_pop_last(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("pop_last() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    if let Some(items) = arg.as_list() {
        if items.is_empty() {
            return Err("pop_last() on empty list".to_string());
        }
        Ok(*items.last().unwrap())
    } else if let Some(s) = arg.as_str() {
        if s.is_empty() {
            return Err("pop_last() on empty string".to_string());
        }
        Ok(Value::from_string(
            gc,
            s.chars().last().unwrap().to_string(),
        ))
    } else {
        Err(format!(
            "pop_last() expects list or string, got {}",
            arg.type_name()
        ))
    }
}

pub(crate) fn bi_drop_last(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("drop_last() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    let got = arg.type_name().to_string();

    if arg.as_list().is_some() {
        let mut items = arg.into_rad_list().unwrap();
        if items.is_empty() {
            return Err("drop_last() on empty list".to_string());
        }
        items.pop();
        Ok(Value::from_rad_list(gc, items))
    } else if arg.as_str().is_some() {
        let mut s = arg.into_string().unwrap();
        if s.is_empty() {
            return Err("drop_last() on empty string".to_string());
        }
        s.pop();
        Ok(Value::from_string(gc, s))
    } else {
        Err(format!("drop_last() expects list or string, got {}", got))
    }
}

/// `drop_first(xs)` — the queue-advance dual of drop_last: everything
/// after the head. Errors on empty (no silent no-op on a drained queue).
pub(crate) fn bi_drop_first(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("drop_first() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    let got = arg.type_name().to_string();
    if arg.as_list().is_some() {
        let items = arg.into_rad_list().unwrap();
        if items.is_empty() {
            return Err("drop_first() on empty list".to_string());
        }
        Ok(Value::from_rad_list(
            gc,
            crate::value::RadList::new(items.as_slice()[1..].to_vec()),
        ))
    } else if arg.as_str().is_some() {
        let s = arg.as_str().unwrap();
        if s.is_empty() {
            return Err("drop_first() on empty string".to_string());
        }
        let mut chars = s.chars();
        chars.next();
        Ok(Value::from_string(gc, chars.as_str().to_string()))
    } else {
        Err(format!("drop_first() expects list or string, got {}", got))
    }
}

pub(crate) fn bi_try_int(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("try_int() requires 1 argument".into());
    }
    let result = if let Some(value) = args[0].as_int() {
        Some(Value::from_int(gc, value))
    } else if let Some(value) = args[0].as_float() {
        Some(Value::from_int(gc, value as i64))
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
    } else if let Some(value) = args[0].as_int() {
        Some(Value::from_float(value as f64))
    } else if let Some(value) = args[0].as_str() {
        value.parse::<f64>().ok().map(Value::from_float)
    } else {
        None
    };
    Ok(wrap_option(gc, result))
}

pub(crate) fn wrap_option(gc: &mut GcHeap, value: Option<Value>) -> Value {
    match value {
        Some(value) => {
            let mut fields = HashMap::new();
            fields.insert("value".to_string(), value);
            Value::sum_type(gc, "Option".to_string(), "Some".to_string(), fields)
        }
        None => Value::sum_type(gc, "Option".to_string(), "None".to_string(), HashMap::new()),
    }
}
