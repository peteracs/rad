pub(crate) fn bi_keys(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("keys() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    let type_name = arg.type_name().to_string();
    if let Some(c) = arg.as_component() {
        let mut ks: Vec<String> = c.layout.iter().cloned().collect();
        ks.sort();
        let vals: Vec<Value> = ks.into_iter().map(|s| Value::from_string(gc, s)).collect();
        Ok(Value::list(gc, vals))
    } else if let Some(st) = arg.as_sum_type() {
        let mut ks: Vec<String> = st.fields.keys().cloned().collect();
        ks.sort();
        let vals: Vec<Value> = ks.into_iter().map(|s| Value::from_string(gc, s)).collect();
        Ok(Value::list(gc, vals))
    } else if let Some(m) = arg.as_map() {
        let mut sorted_keys: Vec<MapKey> = m.keys().cloned().collect();
        sorted_keys.sort();
        let vals: Vec<Value> = sorted_keys.into_iter().map(|k| k.to_value(gc)).collect();
        Ok(Value::list(gc, vals))
    } else {
        Err(format!(
            "keys() expects component, sum type, or map, got {}",
            type_name
        ))
    }
}

pub(crate) fn bi_entries(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("entries() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    let type_name = arg.type_name().to_string();
    if let Some(m) = arg.as_map() {
        let mut sorted_keys: Vec<&MapKey> = m.keys().collect();
        sorted_keys.sort();
        let mut rows = Vec::with_capacity(m.len());
        for k in sorted_keys {
            let key_v = k.to_value(gc);
            rows.push(Value::list(gc, vec![key_v, m[k]]));
        }
        Ok(Value::list(gc, rows))
    } else if let Some(c) = arg.as_component() {
        let mut ks: Vec<String> = c.layout.iter().cloned().collect();
        ks.sort();
        let mut rows = Vec::with_capacity(ks.len());
        for k in ks {
            let idx = c.layout.iter().position(|f| f == &k).unwrap();
            let v = c.values[idx];
            let k_v = Value::from_string(gc, k);
            rows.push(Value::list(gc, vec![k_v, v]));
        }
        Ok(Value::list(gc, rows))
    } else if let Some(st) = arg.as_sum_type() {
        let mut keys: Vec<String> = st.fields.keys().cloned().collect();
        keys.sort();
        let rows: Vec<Value> = keys
            .into_iter()
            .map(|k| {
                let v = *st.fields.get(&k).unwrap();
                let k_v = Value::from_string(gc, k);
                Value::list(gc, vec![k_v, v])
            })
            .collect();
        Ok(Value::list(gc, rows))
    } else {
        Err(format!(
            "entries() expects map, component, or sum type, got {}",
            type_name
        ))
    }
}

pub(crate) fn bi_merge(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("merge() requires 2 arguments".into());
    }
    let mut arg_iter = args.into_iter();
    let left = arg_iter.next().unwrap();
    let right = arg_iter.next().unwrap();
    let left_got = left.type_name();
    let right_got = right.type_name();
    let mut left_map = left
        .into_map()
        .ok_or_else(|| format!("merge() first argument must be map, got {}", left_got))?;
    let right_map = right
        .into_map()
        .ok_or_else(|| format!("merge() second argument must be map, got {}", right_got))?;

    for (k, v) in right_map.iter() {
        left_map.insert(k.clone(), *v);
    }
    Ok(Value::map(gc, left_map))
}

pub(crate) fn bi_remove_key(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("remove_key() requires 2 arguments".into());
    }
    let key_val = args.pop().unwrap();
    let map_val = args.pop().unwrap();

    let map_key = MapKey::from_value(&key_val)?;
    let map_type_name = map_val.type_name().to_string();

    if let Some(mut m) = map_val.into_map() {
        m.remove(&map_key);
        Ok(Value::map(gc, m))
    } else {
        Err(format!("remove_key() expects map, got {}", map_type_name))
    }
}

pub(crate) fn bi_values(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("values() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    let type_name = arg.type_name().to_string();
    if let Some(m) = arg.as_map() {
        let mut sorted_keys: Vec<&MapKey> = m.keys().collect();
        sorted_keys.sort();
        let values: Vec<Value> = sorted_keys.into_iter().map(|k| m[k]).collect();
        Ok(Value::list(gc, values))
    } else if let Some(c) = arg.as_component() {
        let mut ks: Vec<String> = c.layout.iter().cloned().collect();
        ks.sort();
        Ok(Value::list(
            gc,
            ks.iter()
                .map(|k| {
                    let idx = c.layout.iter().position(|f| f == k).unwrap();
                    c.values[idx]
                })
                .collect(),
        ))
    } else if let Some(st) = arg.as_sum_type() {
        let mut keys: Vec<String> = st.fields.keys().cloned().collect();
        keys.sort();
        Ok(Value::list(
            gc,
            keys.iter().map(|k| *st.fields.get(k).unwrap()).collect(),
        ))
    } else {
        Err(format!(
            "values() expects map, component, or sum type, got {}",
            type_name
        ))
    }
}
