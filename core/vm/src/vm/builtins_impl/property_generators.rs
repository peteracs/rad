pub(crate) fn bi_gen_int(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    let mut out = Vec::with_capacity(100);
    out.push(Value::from_int(gc, 0));
    for k in 1..=48 {
        out.push(Value::from_int(gc, k));
        out.push(Value::from_int(gc, -k));
    }
    out.push(Value::from_int(gc, 49));
    out.push(Value::from_int(gc, i64::MAX / 2));
    out.push(Value::from_int(gc, i64::MIN / 2));
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_gen_float(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    let mut out = Vec::with_capacity(100);
    out.push(Value::from_float(0.0));
    for k in 1..=48 {
        out.push(Value::from_float(k as f64));
        out.push(Value::from_float(-(k as f64)));
    }
    out.push(Value::from_float(f64::INFINITY));
    out.push(Value::from_float(f64::NEG_INFINITY));
    out.push(Value::from_float(f64::NAN));
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_gen_str(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    let mut out = Vec::with_capacity(21);
    out.push(Value::from_string(gc, String::new()));
    for len in 1..=20 {
        out.push(Value::from_string(gc, "a".repeat(len)));
    }
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_gen_bool(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    Ok(Value::list(gc, vec![Value::TRUE, Value::FALSE]))
}

pub(crate) fn bi_gen_list(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("gen_list() requires 1 argument".into());
    }
    let items = args[0]
        .as_list()
        .ok_or_else(|| format!("gen_list() expects list, got {}", args[0].type_name()))?;
    let n = items.len();
    let mut out = Vec::with_capacity(n + 1);
    for end in 0..=n {
        out.push(Value::list(gc, items.slice(0, end)));
    }
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_assert(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("assert() requires 2 arguments".into());
    }
    if !args[0].is_truthy() {
        return Err(args[1].print_display());
    }
    Ok(Value::NIL)
}

pub(crate) fn bi_assert_eq(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("assert_eq() requires 2 arguments".into());
    }
    if args[0] != args[1] {
        return Err(format!(
            "assert_eq failed: {} != {}",
            args[0].print_display(),
            args[1].print_display()
        ));
    }
    Ok(Value::NIL)
}
