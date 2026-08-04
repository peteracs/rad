fn host() -> &'static HostApi {
    HOST.get().expect("RAD initialized the extension")
}

fn fail(message: impl AsRef<str>) -> u64 {
    let message = CString::new(message.as_ref())
        .unwrap_or_else(|_| CString::new("native kernel error contained NUL").unwrap());
    unsafe {
        (host().set_error)(message.as_ptr());
        (host().make_nil)()
    }
}

fn arg_slice<'a>(args: *const u64, argc: usize) -> Result<&'a [u64], String> {
    if argc == 0 {
        return Ok(&[]);
    }
    if args.is_null() {
        return Err("native kernel received a null argument array".to_string());
    }
    Ok(unsafe { std::slice::from_raw_parts(args, argc) })
}

fn int_arg(args: &[u64], index: usize, operation: &str) -> Result<i64, String> {
    let raw = *args
        .get(index)
        .ok_or_else(|| format!("{operation} is missing argument {}", index + 1))?;
    let mut value = 0i64;
    if unsafe { (host().as_int)(raw, &mut value) } {
        Ok(value)
    } else {
        Err(format!("{operation} argument {} must be int", index + 1))
    }
}

fn string_arg(args: &[u64], index: usize, operation: &str) -> Result<String, String> {
    let raw = *args
        .get(index)
        .ok_or_else(|| format!("{operation} is missing argument {}", index + 1))?;
    let pointer = unsafe { (host().as_string_ptr)(raw) };
    if pointer.is_null() {
        return Err(format!("{operation} argument {} must be str", index + 1));
    }
    let length = unsafe { (host().as_string_len)(raw) };
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) };
    String::from_utf8(bytes.to_vec())
        .map_err(|_| format!("{operation} argument {} is not UTF-8", index + 1))
}

fn integer_list(args: &[u64], operation: &str) -> Result<Vec<i64>, String> {
    let encoded = string_arg(args, 0, operation)?;
    serde_json::from_str::<Vec<i64>>(&encoded)
        .map_err(|error| format!("{operation} expects a JSON list<int>: {error}"))
}

fn return_json(value: JsonValue) -> u64 {
    match CString::new(value.to_string()) {
        Ok(encoded) => unsafe { (host().make_string)(encoded.as_ptr()) },
        Err(_) => fail("native kernel produced JSON containing NUL"),
    }
}

fn exact_arity(args: &[u64], expected: usize, operation: &str) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(format!(
            "{operation} expects {expected} arguments, got {}",
            args.len()
        ))
    }
}
