pub(crate) fn bi_chr(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("chr() requires 1 argument".into());
    }
    let code = args[0]
        .as_int()
        .ok_or_else(|| format!("chr() expects int, got {}", args[0].type_name()))?;
    let ch = char::from_u32(code as u32)
        .ok_or_else(|| format!("chr(): invalid Unicode code point {}", code))?;
    Ok(Value::from_string(gc, ch.to_string()))
}

pub(crate) fn bi_ord(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("ord() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("ord() expects string, got {}", args[0].type_name()))?;
    let ch = s
        .chars()
        .next()
        .ok_or_else(|| "ord() called on empty string".to_string())?;
    Ok(Value::from_int(gc, ch as i64))
}

pub(crate) fn bi_chars(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("chars() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("chars() expects string, got {}", args[0].type_name()))?;
    let result: Vec<Value> = s
        .chars()
        .map(|c| Value::from_string(gc, c.to_string()))
        .collect();
    Ok(Value::list(gc, result))
}

pub(crate) fn bi_to_upper(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("to_upper() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("to_upper() expects string, got {}", args[0].type_name()))?;
    Ok(Value::from_string(gc, s.to_uppercase()))
}

pub(crate) fn bi_to_lower(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("to_lower() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("to_lower() expects string, got {}", args[0].type_name()))?;
    Ok(Value::from_string(gc, s.to_lowercase()))
}

pub(crate) fn bi_byte_at(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("byte_at() requires exactly 2 arguments (string, index)".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("byte_at() expects string, got {}", args[0].type_name()))?;
    let idx = args[1]
        .as_int()
        .ok_or_else(|| format!("byte_at() expects int index, got {}", args[1].type_name()))?;

    if idx < 0 {
        return Err("byte_at() index cannot be negative".into());
    }
    let uidx = usize::try_from(idx).map_err(|_| format!("byte_at() index {} is too large", idx))?;
    let bytes = s.as_bytes();
    if uidx >= bytes.len() {
        return Err(format!(
            "byte_at() index {} out of bounds (len {})",
            uidx,
            bytes.len()
        ));
    }

    Ok(Value::from_int(gc, bytes[uidx] as i64))
}

pub(crate) fn bi_substring_bytes(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("substring_bytes() requires exactly 3 arguments (string, start, end)".into());
    }
    let s = args[0].as_str().ok_or_else(|| {
        format!(
            "substring_bytes() expects string, got {}",
            args[0].type_name()
        )
    })?;
    let start = args[1].as_int().ok_or_else(|| {
        format!(
            "substring_bytes() expects int start, got {}",
            args[1].type_name()
        )
    })?;
    let end = args[2].as_int().ok_or_else(|| {
        format!(
            "substring_bytes() expects int end, got {}",
            args[2].type_name()
        )
    })?;

    if start < 0 {
        return Err("substring_bytes() start cannot be negative".into());
    }
    if end < 0 {
        return Err("substring_bytes() end cannot be negative".into());
    }
    if start > end {
        return Err("substring_bytes() start cannot be greater than end".into());
    }

    let ustart = usize::try_from(start)
        .map_err(|_| format!("substring_bytes() start {} is too large", start))?;
    let uend =
        usize::try_from(end).map_err(|_| format!("substring_bytes() end {} is too large", end))?;
    let bytes = s.as_bytes();

    if uend > bytes.len() {
        return Err(format!(
            "substring_bytes() end {} out of bounds (len {})",
            uend,
            bytes.len()
        ));
    }

    // We must ensure the byte slice is valid UTF-8, otherwise we'd create an invalid string
    let slice = &bytes[ustart..uend];
    match std::str::from_utf8(slice) {
        Ok(valid_str) => Ok(Value::from_string(gc, valid_str.to_string())),
        Err(_) => Err(format!(
            "substring_bytes() range {}..{} does not form valid UTF-8",
            ustart, uend
        )),
    }
}

pub(crate) fn bi_byte_len(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("byte_len() requires exactly 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("byte_len() expects string, got {}", args[0].type_name()))?;
    Ok(Value::from_int(gc, s.len() as i64))
}

pub(crate) fn bi_split(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("split() requires 2 arguments".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("split() expects string, got {}", args[0].type_name()))?;
    let delim = args[1].as_str().ok_or_else(|| {
        format!(
            "split() delimiter must be string, got {}",
            args[1].type_name()
        )
    })?;

    let parts: Vec<Value> = if delim.is_empty() {
        s.chars()
            .map(|p| Value::from_string(gc, p.to_string()))
            .collect()
    } else {
        s.split(delim)
            .map(|p| Value::from_string(gc, p.to_string()))
            .collect()
    };
    Ok(Value::list(gc, parts))
}

pub(crate) fn bi_join(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("join() requires 2 arguments".into());
    }
    let items = args[0]
        .as_list()
        .ok_or_else(|| format!("join() expects list, got {}", args[0].type_name()))?;
    let sep = args[1].as_str().ok_or_else(|| {
        format!(
            "join() separator must be string, got {}",
            args[1].type_name()
        )
    })?;
    let strs: Vec<String> = items.iter().map(|v| v.print_display()).collect();
    Ok(Value::from_string(gc, strs.join(sep)))
}

pub(crate) fn bi_trim(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("trim() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("trim() expects string, got {}", args[0].type_name()))?;
    Ok(Value::from_string(gc, s.trim().to_string()))
}

pub(crate) fn bi_replace(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 3 {
        return Err("replace() requires 3 arguments".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("replace() expects string, got {}", args[0].type_name()))?;
    let from = args[1].as_str().ok_or_else(|| {
        format!(
            "replace() pattern must be string, got {}",
            args[1].type_name()
        )
    })?;
    let to = args[2].as_str().ok_or_else(|| {
        format!(
            "replace() replacement must be string, got {}",
            args[2].type_name()
        )
    })?;
    Ok(Value::from_string(gc, s.replace(from, to)))
}

pub(crate) fn bi_starts_with(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("starts_with() requires 2 arguments".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("starts_with() expects string, got {}", args[0].type_name()))?;
    let prefix = args[1].as_str().ok_or_else(|| {
        format!(
            "starts_with() prefix must be string, got {}",
            args[1].type_name()
        )
    })?;
    Ok(Value::from_bool(s.starts_with(prefix)))
}

pub(crate) fn bi_ends_with(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("ends_with() requires 2 arguments".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("ends_with() expects string, got {}", args[0].type_name()))?;
    let suffix = args[1].as_str().ok_or_else(|| {
        format!(
            "ends_with() suffix must be string, got {}",
            args[1].type_name()
        )
    })?;
    Ok(Value::from_bool(s.ends_with(suffix)))
}

pub(crate) fn bi_regex_is_match(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("regex_is_match() requires exactly 2 arguments".into());
    }
    let pattern = args[0].as_str().ok_or_else(|| {
        format!(
            "regex_is_match() expects pattern string, got {}",
            args[0].type_name()
        )
    })?;
    let text = args[1].as_str().ok_or_else(|| {
        format!(
            "regex_is_match() expects text string, got {}",
            args[1].type_name()
        )
    })?;
    let regex = Regex::new(pattern)
        .map_err(|e| format!("regex_is_match() invalid pattern '{}': {}", pattern, e))?;
    Ok(Value::from_bool(regex.is_match(text)))
}

pub(crate) fn bi_regex_find(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("regex_find() requires exactly 2 arguments".into());
    }
    let pattern = args[0].as_str().ok_or_else(|| {
        format!(
            "regex_find() expects pattern string, got {}",
            args[0].type_name()
        )
    })?;
    let text = args[1].as_str().ok_or_else(|| {
        format!(
            "regex_find() expects text string, got {}",
            args[1].type_name()
        )
    })?;
    let regex = Regex::new(pattern)
        .map_err(|e| format!("regex_find() invalid pattern '{}': {}", pattern, e))?;
    let found = regex
        .find(text)
        .map(|m| Value::from_string(gc, m.as_str().to_string()));
    Ok(wrap_option(gc, found))
}
