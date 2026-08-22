impl VM {
    fn bi_flat_map(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("flat_map() requires 2 arguments".into());
        }
        let mut arguments = args.into_iter();
        let collection = arguments.next().unwrap();
        let function = arguments.next().unwrap();
        let items = if collection.as_list().is_some() {
            collection.into_rad_list().unwrap().into_vec()
        } else if let Some(text) = collection.as_str() {
            let gc = &mut self.gc;
            text.chars()
                .map(|character| Value::from_string(gc, character.to_string()))
                .collect()
        } else {
            return Err(format!(
                "flat_map() expects list or string, got {}",
                collection.type_name()
            ));
        };

        let mut result = Vec::new();
        for item in items {
            let mapped = self.call_value(&function, vec![item])?;
            let mapped_items = mapped.as_list().ok_or_else(|| {
                format!(
                    "flat_map() callback must return a list, got {}",
                    mapped.type_name()
                )
            })?;
            result.extend(mapped_items.iter().cloned());
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_group_by(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("group_by() requires 2 arguments".into());
        }
        let mut arguments = args.into_iter();
        let collection = arguments.next().unwrap();
        let function = arguments.next().unwrap();
        let items = if collection.as_list().is_some() {
            collection.into_rad_list().unwrap().into_vec()
        } else if let Some(text) = collection.as_str() {
            let gc = &mut self.gc;
            text.chars()
                .map(|character| Value::from_string(gc, character.to_string()))
                .collect()
        } else {
            return Err(format!(
                "group_by() expects list or string, got {}",
                collection.type_name()
            ));
        };

        let mut groups: HashMap<MapKey, Vec<Value>> = HashMap::new();
        for item in items {
            let key_value = self.call_value(&function, vec![item])?;
            let key = MapKey::from_value(&key_value)
                .map_err(|error| format!("group_by() key function: {error}"))?;
            groups.entry(key).or_default().push(item);
        }
        let gc = &mut self.gc;
        let grouped = groups
            .into_iter()
            .map(|(key, values)| (key, Value::list(gc, values)))
            .collect::<MapStorage>();
        Ok(Value::map(gc, grouped))
    }

    fn bi_sort_by(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("sort_by() requires 2 arguments (list, key_fn)".into());
        }
        let mut arguments = args.into_iter();
        let collection = arguments.next().unwrap();
        let collection_type = collection.type_name();
        let key_function = arguments.next().unwrap();
        let is_string = collection.as_str().is_some();
        let items = if collection.as_list().is_some() {
            collection.into_rad_list().unwrap().into_vec()
        } else if let Some(text) = collection.as_str() {
            let gc = &mut self.gc;
            text.chars()
                .map(|character| Value::from_string(gc, character.to_string()))
                .collect()
        } else {
            return Err(format!(
                "sort_by() expects list or string, got {collection_type}"
            ));
        };

        let mut keyed = Vec::with_capacity(items.len());
        for item in items {
            let key = self.call_value(&key_function, vec![item])?;
            keyed.push((key, item));
        }
        let mut comparison_error = None;
        keyed.sort_by(|(left, _), (right, _)| match helpers::compare_values(left, right) {
            Ok(ordering) => ordering,
            Err(error) => {
                comparison_error.get_or_insert_with(|| {
                    format!("sort_by() key function returned incomparable keys: {error}")
                });
                std::cmp::Ordering::Equal
            }
        });
        if let Some(error) = comparison_error {
            return Err(error);
        }

        let result = keyed
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        let gc = &mut self.gc;
        if is_string {
            let text = result
                .into_iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect();
            Ok(Value::from_string(gc, text))
        } else {
            Ok(Value::list(gc, result))
        }
    }
}
