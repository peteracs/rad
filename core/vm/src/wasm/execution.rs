impl RadRuntime {
    fn json_to_value(&mut self, jv: &serde_json::Value) -> Result<Value, String> {
        match jv {
            serde_json::Value::Null => Ok(Value::NIL),
            serde_json::Value::Bool(b) => Ok(Value::from_bool(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::from_int(self.vm.gc_mut(), i))
                } else {
                    Ok(Value::from_float(n.as_f64().unwrap_or(0.0)))
                }
            }
            serde_json::Value::String(s) => Ok(Value::from_string(self.vm.gc_mut(), s.clone())),
            serde_json::Value::Object(o) if o.len() == 1 && o.contains_key("entity") => {
                let name = o["entity"]
                    .as_str()
                    .ok_or("session_emit: {\"entity\": ...} must name an entity")?;
                let eid = self
                    .vm
                    .world
                    .get_entity_by_name(name)
                    .ok_or_else(|| format!("session_emit: no entity named '{}'", name))?;
                Ok(Value::from_entity_id(self.vm.gc_mut(), eid))
            }
            serde_json::Value::Array(items) => {
                let mut vals = Vec::with_capacity(items.len());
                for item in items {
                    vals.push(self.json_to_value(item)?);
                }
                Ok(Value::list(self.vm.gc_mut(), vals))
            }
            serde_json::Value::Object(_) => Err(
                "session_emit: nested objects are not supported in event fields \
                     (use {\"entity\": \"name\"} for entity references)"
                    .to_string(),
            ),
        }
    }

    fn compile_and_run_seeded(&mut self, source: &str, seed: u64) -> Result<String, String> {
        self.vm = VM::new();
        self.vm.set_random_seed(seed);
        self.output.clear();

        let compile_result = Self::compile_browser_source(source, BrowserSourceKind::Session)?;
        self.vm.load_compile_result(compile_result);
        self.vm.print_buffer.clear();
        match self.vm.run(0) {
            Ok(()) => {
                self.output = self.vm.print_buffer.clone();
                Ok(self.output.join("\n"))
            }
            Err(e) => {
                self.output = self.vm.print_buffer.clone();
                Err(e)
            }
        }
    }
}
