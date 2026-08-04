pub(crate) fn bi_bitset_new(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    Ok(Value::bitset(gc, Vec::new()))
}

pub(crate) fn bi_bitset_set(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err(format!(
            "bitset_set expects 2 arguments, got {}",
            args.len()
        ));
    }
    let idx_val = args.pop().unwrap();
    let bs_val = args.pop().unwrap();

    let idx = idx_val
        .as_int()
        .ok_or_else(|| "bitset_set expects an integer as second argument".to_string())?;
    if idx < 0 {
        return Ok(bs_val);
    }
    if idx > 100_000_000 {
        return Err(format!(
            "bitset_set index out of bounds: {} (max 100,000,000)",
            idx
        ));
    }
    let word_idx = (idx / 64) as usize;

    let mut words = bs_val
        .into_bitset()
        .ok_or_else(|| "bitset_set expects a bitset as first argument".to_string())?;
    if word_idx >= words.len() {
        let mut new_cap = if words.is_empty() { 8 } else { words.len() };
        while new_cap <= word_idx {
            new_cap *= 2;
        }
        words.resize(new_cap, 0);
    }
    words[word_idx] |= 1 << (idx % 64);
    Ok(Value::bitset(gc, words))
}

pub(crate) fn bi_bitset_has(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err(format!(
            "bitset_has expects 2 arguments, got {}",
            args.len()
        ));
    }
    let bs = args[0]
        .as_bitset()
        .ok_or_else(|| "bitset_has expects a bitset as first argument".to_string())?;
    let idx = args[1]
        .as_int()
        .ok_or_else(|| "bitset_has expects an integer as second argument".to_string())?;
    if idx < 0 {
        return Ok(Value::FALSE);
    }
    let word_idx = (idx / 64) as usize;
    let words = bs;
    if word_idx >= words.len() {
        return Ok(Value::FALSE);
    }
    let has = (words[word_idx] & (1 << (idx % 64))) != 0;
    Ok(Value::from_bool(has))
}

pub(crate) fn bi_bitset_clear(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err(format!(
            "bitset_clear expects 2 arguments, got {}",
            args.len()
        ));
    }
    let idx_val = args.pop().unwrap();
    let bs_val = args.pop().unwrap();

    let idx = idx_val
        .as_int()
        .ok_or_else(|| "bitset_clear expects an integer as second argument".to_string())?;
    if idx < 0 {
        return Ok(bs_val);
    }
    let word_idx = (idx / 64) as usize;

    let mut words = bs_val
        .into_bitset()
        .ok_or_else(|| "bitset_clear expects a bitset as first argument".to_string())?;
    if word_idx < words.len() {
        words[word_idx] &= !(1 << (idx % 64));
    }
    Ok(Value::bitset(gc, words))
}

struct FormatSpec {
    fill: char,
    align: Option<char>,
    sign: Option<char>,
    alt: bool,
    zero_pad: bool,
    width: Option<usize>,
    precision: Option<usize>,
    ty: Option<char>,
}
