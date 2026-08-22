// Builtins that take a callable and drive it over a sequence: search,
// quantifiers, extremum selection, and folds.

impl VM {
    fn bi_find(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("find() requires 2 arguments (list, predicate)".into());
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else {
            return Err(format!("find() expects list, got {}", list.type_name()));
        };

        for item in items.into_iter() {
            let r = self.call_value(&func, vec![item])?;
            if r.is_truthy() {
                let mut fields = std::collections::HashMap::new();
                fields.insert("value".to_string(), item);
                return Ok(Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                ));
            }
        }
        Ok(Value::sum_type(
            &mut self.gc,
            "Option".to_string(),
            "None".to_string(),
            std::collections::HashMap::new(),
        ))
    }

    /// `any(xs, pred)` / `all(xs, pred)` — short-circuiting predicate
    /// sweeps. `any([])` is false, `all([])` is true (vacuous truth).
    fn bi_any_all(&mut self, args: Vec<Value>, is_any: bool) -> Result<Value, String> {
        let name = if is_any { "any" } else { "all" };
        if args.len() != 2 {
            return Err(format!("{}() requires 2 arguments (list, predicate)", name));
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();
        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else {
            return Err(format!(
                "{}() expects a list, got {}",
                name,
                list.type_name()
            ));
        };
        for item in items.into_iter() {
            let truthy = self.call_value(&func, vec![item])?.is_truthy();
            if is_any && truthy {
                return Ok(Value::from_bool(true));
            }
            if !is_any && !truthy {
                return Ok(Value::from_bool(false));
            }
        }
        Ok(Value::from_bool(!is_any))
    }

    fn bi_max_by(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_extremum_by("max_by", args, false)
    }

    fn bi_min_by(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_extremum_by("min_by", args, true)
    }

    fn bi_extremum_by(
        &mut self,
        name: &str,
        args: Vec<Value>,
        want_min: bool,
    ) -> Result<Value, String> {
        if args.len() < 2 {
            return Err(format!("{}() requires 2 arguments (list, key_fn)", name));
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let key_fn = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else {
            return Err(format!("{}() expects list, got {}", name, list.type_name()));
        };

        if items.is_empty() {
            return Ok(Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                std::collections::HashMap::new(),
            ));
        }

        let mut best = items[0];
        let mut best_key = self.call_value(&key_fn, vec![best])?;
        for item in items.into_iter().skip(1) {
            let key = self.call_value(&key_fn, vec![item])?;
            let ord = helpers::compare_values(&key, &best_key).map_err(|e| {
                format!("{}() key function returned incomparable keys: {}", name, e)
            })?;
            let replace = if want_min {
                ord == std::cmp::Ordering::Less
            } else {
                ord == std::cmp::Ordering::Greater
            };
            if replace {
                best = item;
                best_key = key;
            }
        }

        let mut fields = std::collections::HashMap::new();
        fields.insert("value".to_string(), best);
        Ok(Value::sum_type(
            &mut self.gc,
            "Option".to_string(),
            "Some".to_string(),
            fields,
        ))
    }

    fn bi_reduce(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 3 {
            return Err("reduce() requires 3 arguments".into());
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let mut acc = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else if let Some(s) = list.as_str() {
            s.chars()
                .map(|c| Value::from_string(&mut self.gc, c.to_string()))
                .collect()
        } else {
            return Err(format!(
                "reduce() expects list or string, got {}",
                list.type_name()
            ));
        };

        for item in items.into_iter() {
            acc = self.call_value(&func, vec![acc, item])?;
        }
        Ok(acc)
    }

    fn call_named_unary(&mut self, callee: Value, argument: Value) -> Result<Value, String> {
        let function = callee
            .as_fn()
            .ok_or_else(|| "allocation-free callback must be a named guest function".to_string())?;
        if function.arity != 1 {
            return Err(format!(
                "visit_view() callback '{}' expects {} arguments; required fn(entity) -> nil",
                function.name, function.arity
            ));
        }
        let chunk_id = function.chunk_id;
        if chunk_id >= self.chunks.len() {
            return Err(format!("Invalid function chunk {chunk_id}"));
        }
        let saved_depth = self.frames.len();
        self.push(argument);
        let stack_base = self.stack.len() - 1;
        let frame_id = self.allocate_frame_id();
        self.frames.push(CallFrame {
            frame_id,
            chunk_id,
            ip: 0,
            stack_base,
            captures: None,
            system_writeback: None,
        });
        self.run_frames(saved_depth)
            .map_err(|failure| failure.render_message())?;
        self.pop()
    }

    fn bi_map_or(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 3 {
            return Err("map_or() requires 3 arguments (option_or_result, default, fn)".into());
        }
        let container = args[0];
        let default_value = args[1];
        let mapper = args[2];
        if let Some(st) = container.as_sum_type() {
            if (st.type_name == "Option" && st.variant == "Some")
                || (st.type_name == "Result" && st.variant == "Ok")
            {
                let inner = st.fields.get("value").copied().unwrap_or(Value::NIL);
                return self.call_value(&mapper, vec![inner]);
            }
            if (st.type_name == "Option" && st.variant == "None")
                || (st.type_name == "Result" && st.variant == "Err")
            {
                return Ok(default_value);
            }
        }
        Ok(default_value)
    }
}
