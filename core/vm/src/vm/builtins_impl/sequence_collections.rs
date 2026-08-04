pub(crate) fn bi_sort(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("sort() requires 1 argument".into());
    }
    let mut arg_iter = args.into_iter();
    let list = arg_iter.next().unwrap();
    let got = list.type_name();

    let is_string = list.as_str().is_some();
    let mut items = if let Some(l) = list.as_list() {
        l.clone().into_vec()
    } else if let Some(s) = list.as_str() {
        s.chars()
            .map(|c| Value::from_string(gc, c.to_string()))
            .collect()
    } else {
        return Err(format!("sort() expects list or string, got {}", got));
    };

    let mut err: Option<String> = None;
    items.sort_by(
        |a, b| match (a.as_int(), a.as_float(), b.as_int(), b.as_float()) {
            (Some(i), _, Some(j), _) => i.cmp(&j),
            (_, Some(x), _, Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
            (Some(i), _, _, Some(y)) => (i as f64)
                .partial_cmp(&y)
                .unwrap_or(std::cmp::Ordering::Equal),
            (_, Some(x), Some(j), _) => x
                .partial_cmp(&(j as f64))
                .unwrap_or(std::cmp::Ordering::Equal),
            // strings, bools, tuples (lexicographic): one total order
            // shared with sort_by/min_by/max_by
            _ => match crate::vm::helpers::compare_values(a, b) {
                Ok(ord) => ord,
                Err(e) => {
                    if err.is_none() {
                        err = Some(format!("sort() {}", e));
                    }
                    std::cmp::Ordering::Equal
                }
            },
        },
    );
    if let Some(e) = err {
        return Err(e);
    }

    if is_string {
        let s: String = items
            .into_iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        Ok(Value::from_string(gc, s))
    } else {
        Ok(Value::list(gc, items))
    }
}

pub(crate) fn bi_reverse(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("reverse() requires 1 argument".into());
    }
    let mut arg_iter = args.into_iter();
    let first = arg_iter.next().unwrap();
    if first.as_list().is_some() {
        let mut items = first
            .into_rad_list()
            .expect("list type already checked")
            .into_vec();
        items.reverse();
        Ok(Value::list(gc, items))
    } else if let Some(s) = first.as_str() {
        Ok(Value::from_string(gc, s.chars().rev().collect()))
    } else {
        Err(format!(
            "reverse() expects list or string, got {}",
            first.type_name()
        ))
    }
}

pub(crate) fn bi_slice(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("slice() requires at least 2 arguments".into());
    }
    let end_arg = if args.len() > 2 {
        Some(args.pop().unwrap())
    } else {
        None
    };
    let start_arg = args.pop().unwrap();
    let start = start_arg
        .as_int()
        .filter(|n| *n >= 0)
        .map(|n| n as usize)
        .ok_or("slice() start must be a non-negative int")?;

    let arg = args.pop().unwrap();
    let type_name = arg.type_name().to_string();

    if let Some(list) = arg.into_rad_list() {
        let end = match end_arg {
            Some(v) => v
                .as_int()
                .filter(|n| *n >= 0)
                .map(|n| n as usize)
                .ok_or("slice() end must be a non-negative int")?,
            None => list.len(),
        };
        Ok(Value::list(gc, list.into_slice(start, end)))
    } else if let Some(st) = arg.as_str() {
        let chars: Vec<char> = st.chars().collect();
        let end = match end_arg {
            Some(v) => v
                .as_int()
                .filter(|n| *n >= 0)
                .map(|n| n as usize)
                .ok_or("slice() end must be a non-negative int")?,
            None => chars.len(),
        };
        let e = end.min(chars.len());
        let s = start.min(e);
        Ok(Value::from_string(gc, chars[s..e].iter().collect()))
    } else {
        Err(format!("slice() expects list or string, got {}", type_name))
    }
}

pub(crate) fn bi_range(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let plan = super::range_plan::RangePlan::from_args(&args)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(plan.count)
        .map_err(|_| "range() result is too large to allocate".to_string())?;
    for index in 0..plan.count {
        result.push(Value::from_int(gc, plan.value_at(index)?));
    }
    Ok(Value::list(gc, result))
}

pub(crate) fn bi_contains(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("contains() requires 2 arguments".into());
    }
    if let Some(items) = args[0].as_list() {
        Ok(Value::from_bool(items.contains(&args[1])))
    } else if let Some(s) = args[0].as_str() {
        let needle = args[1].print_display();
        Ok(Value::from_bool(s.contains(&needle)))
    } else if let Some(m) = args[0].as_map() {
        let map_key = MapKey::from_value(&args[1])?;
        Ok(Value::from_bool(m.contains_key(&map_key)))
    } else {
        Err(format!(
            "contains() expects list, string, or map, got {}",
            args[0].type_name()
        ))
    }
}

pub(crate) fn bi_append(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("append() requires 2 arguments".into());
    }
    let mut arg_iter = args.into_iter();
    let left = arg_iter.next().unwrap();
    let right = arg_iter.next().unwrap();
    let left_got = left.type_name();
    let right_got = right.type_name();

    if left.as_list().is_some() {
        let mut left_items = left.into_rad_list().unwrap();
        if let Some(right_items) = right.as_list() {
            left_items.extend_from(right_items);
            Ok(Value::from_rad_list(gc, left_items))
        } else if let Some(right_str) = right.as_str() {
            let right_items: Vec<Value> = right_str
                .chars()
                .map(|c| Value::from_string(gc, c.to_string()))
                .collect();
            left_items.extend_from(
                Value::from_rad_list(gc, crate::value::RadList::new(right_items))
                    .as_list()
                    .unwrap(),
            );
            Ok(Value::from_rad_list(gc, left_items))
        } else {
            Err(format!(
                "append() second argument must be list or string, got {}",
                right_got
            ))
        }
    } else if left.as_str().is_some() {
        let mut left_str = left.into_string().unwrap();
        if let Some(right_str) = right.as_str() {
            left_str.push_str(right_str);
            Ok(Value::from_string(gc, left_str))
        } else if let Some(right_items) = right.as_list() {
            for item in right_items.iter() {
                if let Some(s) = item.as_str() {
                    left_str.push_str(s);
                } else {
                    return Err(format!(
                        "append() cannot append non-string item {} to string",
                        item.type_name()
                    ));
                }
            }
            Ok(Value::from_string(gc, left_str))
        } else {
            Err(format!(
                "append() second argument must be list or string, got {}",
                right_got
            ))
        }
    } else {
        Err(format!("append() expects list or string, got {}", left_got))
    }
}

pub(crate) fn bi_zip(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("zip() requires 2 arguments".into());
    }

    let mut arg_iter = args.into_iter();
    let left = arg_iter.next().unwrap();
    let right = arg_iter.next().unwrap();

    let a_items = if left.as_list().is_some() {
        left.into_rad_list().unwrap().into_vec()
    } else if let Some(s) = left.as_str() {
        s.chars()
            .map(|c| Value::from_string(gc, c.to_string()))
            .collect()
    } else {
        return Err(format!(
            "zip() expects list or string for first argument, got {}",
            left.type_name()
        ));
    };

    let b_items = if right.as_list().is_some() {
        right.into_rad_list().unwrap().into_vec()
    } else if let Some(s) = right.as_str() {
        s.chars()
            .map(|c| Value::from_string(gc, c.to_string()))
            .collect()
    } else {
        return Err(format!(
            "zip() expects list or string for second argument, got {}",
            right.type_name()
        ));
    };

    let pairs: Vec<Value> = a_items
        .into_iter()
        .zip(b_items)
        .map(|(x, y)| Value::list(gc, vec![x, y]))
        .collect();
    Ok(Value::list(gc, pairs))
}

pub(crate) fn bi_enumerate(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("enumerate() requires 1 argument".into());
    }
    let list = args.into_iter().next().unwrap();
    let items = if let Some(l) = list.as_list() {
        l.to_vec()
    } else {
        return Err(format!(
            "enumerate() expects a list, got {}",
            list.type_name()
        ));
    };
    let indexed: Vec<(usize, Value)> = items.into_iter().enumerate().collect();
    let mut pairs = Vec::with_capacity(indexed.len());
    for (i, v) in indexed {
        let idx = Value::from_int(gc, i as i64);
        let pair = Value::list(gc, vec![idx, v]);
        pairs.push(pair);
    }
    Ok(Value::list(gc, pairs))
}
