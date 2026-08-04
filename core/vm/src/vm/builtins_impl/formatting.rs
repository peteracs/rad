pub(crate) fn bi_format(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("format() requires at least 1 argument".into());
    }
    let template = args[0].as_str().ok_or_else(|| {
        format!(
            "format() first argument must be str, got {}",
            args[0].type_name()
        )
    })?;
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    let mut arg_idx = 1usize;
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'}') {
            chars.next();
            if let Some(arg) = args.get(arg_idx) {
                out.push_str(&arg.print_display());
                arg_idx += 1;
            } else {
                return Err("format() missing argument for '{}' placeholder".to_string());
            }
        } else {
            out.push(ch);
        }
    }
    if arg_idx < args.len() {
        return Err("format() received more arguments than '{}' placeholders".to_string());
    }
    Ok(Value::from_string(gc, out))
}

fn parse_format_spec(spec: &str) -> Result<FormatSpec, String> {
    let chars: Vec<char> = spec.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let mut fill = ' ';
    let mut align = None;

    if len >= 2 && matches!(chars[1], '<' | '>' | '^') {
        fill = chars[0];
        align = Some(chars[1]);
        i = 2;
    } else if len >= 1 && matches!(chars[0], '<' | '>' | '^') {
        align = Some(chars[0]);
        i = 1;
    }

    let mut sign = None;
    if i < len && matches!(chars[i], '+' | '-' | ' ') {
        sign = Some(chars[i]);
        i += 1;
    }

    let mut alt = false;
    if i < len && chars[i] == '#' {
        alt = true;
        i += 1;
    }

    let mut zero_pad = false;
    if i < len
        && chars[i] == '0'
        && i + 1 < len
        && (chars[i + 1].is_ascii_digit() || align.is_none())
    {
        zero_pad = true;
        i += 1;
    }

    let mut width = None;
    let w_start = i;
    while i < len && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > w_start {
        width = Some(
            chars[w_start..i]
                .iter()
                .collect::<String>()
                .parse::<usize>()
                .map_err(|_| "invalid width in format spec")?,
        );
    }

    let mut precision = None;
    if i < len && chars[i] == '.' {
        i += 1;
        let p_start = i;
        while i < len && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i > p_start {
            precision = Some(
                chars[p_start..i]
                    .iter()
                    .collect::<String>()
                    .parse::<usize>()
                    .map_err(|_| "invalid precision in format spec")?,
            );
        } else {
            precision = Some(0);
        }
    }

    let mut ty = None;
    if i < len {
        ty = Some(chars[i]);
        i += 1;
    }

    if i < len {
        return Err(format!(
            "invalid format spec: unexpected characters after type: '{}'",
            chars[i..].iter().collect::<String>()
        ));
    }

    Ok(FormatSpec {
        fill,
        align,
        sign,
        alt,
        zero_pad,
        width,
        precision,
        ty,
    })
}

fn apply_padding(s: &str, spec: &FormatSpec) -> String {
    let w = match spec.width {
        Some(w) => w,
        None => return s.to_string(),
    };
    let slen = s.chars().count();
    if slen >= w {
        return s.to_string();
    }
    let pad = w - slen;
    let fill = spec.fill;
    let align = spec.align.unwrap_or(if spec.zero_pad { '>' } else { '<' });
    match align {
        '>' => {
            let mut out = String::with_capacity(w);
            for _ in 0..pad {
                out.push(fill);
            }
            out.push_str(s);
            out
        }
        '<' => {
            let mut out = String::with_capacity(w);
            out.push_str(s);
            for _ in 0..pad {
                out.push(fill);
            }
            out
        }
        '^' => {
            let left = pad / 2;
            let right = pad - left;
            let mut out = String::with_capacity(w);
            for _ in 0..left {
                out.push(fill);
            }
            out.push_str(s);
            for _ in 0..right {
                out.push(fill);
            }
            out
        }
        _ => s.to_string(),
    }
}

fn normalize_sci_exponent(s: &str) -> String {
    let marker = if let Some(i) = s.rfind('e') {
        i
    } else if let Some(i) = s.rfind('E') {
        i
    } else {
        return s.to_string();
    };
    let (base, exp_part) = s.split_at(marker);
    let e_char = &exp_part[..1];
    let rest = &exp_part[1..];
    let (sign, digits) = if rest.starts_with('+') || rest.starts_with('-') {
        (&rest[..1], &rest[1..])
    } else {
        ("+", rest)
    };
    format!("{}{}{}{:0>2}", base, e_char, sign, digits)
}

fn format_int_value(val: i64, spec: &FormatSpec) -> Result<String, String> {
    let ty = spec.ty.unwrap_or('d');
    let mut raw = match ty {
        'd' => format!("{}", val),
        'b' => {
            if spec.alt {
                format!("0b{:b}", val)
            } else {
                format!("{:b}", val)
            }
        }
        'o' => {
            if spec.alt {
                format!("0o{:o}", val)
            } else {
                format!("{:o}", val)
            }
        }
        'x' => {
            if spec.alt {
                format!("0x{:x}", val)
            } else {
                format!("{:x}", val)
            }
        }
        'X' => {
            if spec.alt {
                format!("0X{:X}", val)
            } else {
                format!("{:X}", val)
            }
        }
        'f' | 'F' => {
            let prec = spec.precision.unwrap_or(6);
            format!("{:.prec$}", val as f64, prec = prec)
        }
        'e' => {
            let prec = spec.precision.unwrap_or(6);
            normalize_sci_exponent(&format!("{:.prec$e}", val as f64, prec = prec))
        }
        'E' => {
            let prec = spec.precision.unwrap_or(6);
            normalize_sci_exponent(&format!("{:.prec$E}", val as f64, prec = prec))
        }
        '%' => {
            let prec = spec.precision.unwrap_or(6);
            format!("{:.prec$}%", (val as f64) * 100.0, prec = prec)
        }
        's' => format!("{}", val),
        _ => return Err(format!("unknown format type '{}' for int", ty)),
    };

    if matches!(ty, 'd' | 'b' | 'o' | 'x' | 'X') {
        match spec.sign {
            Some('+') if val >= 0 => raw = format!("+{}", raw),
            Some(' ') if val >= 0 => raw = format!(" {}", raw),
            _ => {}
        }
    }

    if spec.zero_pad && spec.align.is_none() {
        if let Some(w) = spec.width {
            let num_len = raw.chars().count();
            if num_len < w {
                let prefix_end =
                    if raw.starts_with('+') || raw.starts_with('-') || raw.starts_with(' ') {
                        1
                    } else if raw.starts_with("0x")
                        || raw.starts_with("0X")
                        || raw.starts_with("0b")
                        || raw.starts_with("0o")
                    {
                        2
                    } else {
                        0
                    };
                let prefix = &raw[..prefix_end];
                let rest = &raw[prefix_end..];
                let zeros = w - num_len;
                let mut out = String::with_capacity(w);
                out.push_str(prefix);
                for _ in 0..zeros {
                    out.push('0');
                }
                out.push_str(rest);
                return Ok(out);
            }
        }
    }

    let num_spec = FormatSpec {
        align: spec.align.or(Some('>')),
        ..*spec
    };
    Ok(apply_padding(&raw, &num_spec))
}

fn format_float_value(val: f64, spec: &FormatSpec) -> Result<String, String> {
    let ty = spec.ty.unwrap_or('f');
    let prec = spec.precision.unwrap_or(6);
    let mut raw = match ty {
        'f' | 'F' => format!("{:.prec$}", val, prec = prec),
        'e' => normalize_sci_exponent(&format!("{:.prec$e}", val, prec = prec)),
        'E' => normalize_sci_exponent(&format!("{:.prec$E}", val, prec = prec)),
        '%' => format!("{:.prec$}%", val * 100.0, prec = prec),
        'd' => format!("{}", val as i64),
        's' => format!("{}", val),
        _ => return Err(format!("unknown format type '{}' for float", ty)),
    };

    if matches!(ty, 'f' | 'F' | 'e' | 'E' | '%' | 'd') {
        match spec.sign {
            Some('+') if val >= 0.0 && !val.is_nan() => raw = format!("+{}", raw),
            Some(' ') if val >= 0.0 && !val.is_nan() => raw = format!(" {}", raw),
            _ => {}
        }
    }

    if spec.zero_pad && spec.align.is_none() {
        if let Some(w) = spec.width {
            let num_len = raw.chars().count();
            if num_len < w {
                let prefix_end =
                    if raw.starts_with('+') || raw.starts_with('-') || raw.starts_with(' ') {
                        1
                    } else {
                        0
                    };
                let prefix = &raw[..prefix_end];
                let rest = &raw[prefix_end..];
                let zeros = w - num_len;
                let mut out = String::with_capacity(w);
                out.push_str(prefix);
                for _ in 0..zeros {
                    out.push('0');
                }
                out.push_str(rest);
                return Ok(out);
            }
        }
    }

    let num_spec = FormatSpec {
        align: spec.align.or(Some('>')),
        ..*spec
    };
    Ok(apply_padding(&raw, &num_spec))
}

fn format_str_value(val: &str, spec: &FormatSpec) -> String {
    let s = if let Some(prec) = spec.precision {
        if val.chars().count() > prec {
            val.chars().take(prec).collect()
        } else {
            val.to_string()
        }
    } else {
        val.to_string()
    };
    apply_padding(&s, spec)
}

pub(crate) fn bi_format_value(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err(format!(
            "format_value() requires 2 arguments (value, spec), got {}",
            args.len()
        ));
    }
    let spec_str = args[1]
        .as_str()
        .ok_or_else(|| "format_value() second argument must be str".to_string())?;

    if spec_str.is_empty() {
        return Ok(Value::from_string(gc, args[0].print_display()));
    }

    let spec = parse_format_spec(spec_str)?;
    let val = &args[0];

    let result = if let Some(i) = val.as_int() {
        format_int_value(i, &spec)?
    } else if let Some(f) = val.as_float() {
        format_float_value(f, &spec)?
    } else if let Some(s) = val.as_str() {
        let default_align_spec = FormatSpec {
            align: spec.align.or(Some('<')),
            ..spec
        };
        format_str_value(s, &default_align_spec)
    } else {
        let s = val.print_display();
        format_str_value(&s, &spec)
    };

    Ok(Value::from_string(gc, result))
}
