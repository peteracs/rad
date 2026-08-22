// Operator-facing output: print, structured logging, debug traces, and the
// trace assertions tests use to pin them.

impl VM {

    fn bi_print(&mut self, args: Vec<Value>) -> Result<Value, String> {
        let s = args
            .iter()
            .map(|v| v.print_display())
            .collect::<Vec<_>>()
            .join(" ");
        self.print_buffer.push(s.clone());
        if !self.suppress_output {
            println!("{}", s);
        }
        Ok(Value::NIL)
    }

    fn bi_log(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("log() requires exactly 2 arguments (level, map)".into());
        }
        let level = match args[0].as_str() {
            Some(s) => s.to_string(),
            _ => return Err("log() level must be a string".into()),
        };
        let map = match args[1].as_map() {
            Some(m) => m,
            _ => return Err("log() data must be a map".into()),
        };

        let mut json = String::new();
        json.push_str(&format!(r#"{{"level":"{}""#, level));

        if let Some(tid) = self.current_trace_id {
            json.push_str(&format!(r#","trace_id":"{}""#, tid));
        }

        for (k, v) in map.iter() {
            let k_str = match k {
                crate::value::MapKey::Str(s) => s.as_str(),
                _ => continue,
            };
            json.push_str(&format!(r#","{}":{}"#, k_str, v));
        }
        json.push('}');

        self.print_buffer.push(json.clone());
        if !self.suppress_output {
            println!("{}", json);
        }
        Ok(Value::NIL)
    }

    fn bi_metric(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err("metric() requires exactly 4 arguments (type, name, value, tags)".into());
        }
        let mtype = match args[0].as_str() {
            Some(s) => s.to_string(),
            _ => return Err("metric() type must be a string".into()),
        };
        let name = match args[1].as_str() {
            Some(s) => s.to_string(),
            _ => return Err("metric() name must be a string".into()),
        };
        let val = if let Some(f) = args[2].as_float() {
            f
        } else if let Some(i) = args[2].as_int() {
            i as f64
        } else {
            return Err("metric() value must be a number".into());
        };
        let tags = match args[3].as_map() {
            Some(m) => m,
            _ => return Err("metric() tags must be a map".into()),
        };

        let mut json = String::new();
        json.push_str(&format!(
            r#"{{"metric_type":"{}","name":"{}","value":{}"#,
            mtype, name, val
        ));

        if let Some(tid) = self.current_trace_id {
            json.push_str(&format!(r#","trace_id":"{}""#, tid));
        }

        json.push_str(r#","tags":{"#);
        let mut first = true;
        for (k, v) in tags.iter() {
            let k_str = match k {
                MapKey::Str(s) => s.as_str(),
                _ => continue,
            };
            if !first {
                json.push(',');
            }
            first = false;
            json.push_str(&format!(r#""{}":{}"#, k_str, v));
        }
        json.push_str("}}");

        self.print_buffer.push(json.clone());
        if !self.suppress_output {
            println!("{}", json);
        }
        Ok(Value::NIL)
    }

    fn bi_trace_id(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("trace_id() takes no arguments".into());
        }
        if let Some(tid) = self.current_trace_id {
            Ok(Value::from_int(&mut self.gc, tid as i64))
        } else {
            Ok(Value::NIL)
        }
    }

    pub(crate) fn bi_flush_events(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("flush_events() takes no arguments".into());
        }
        let active_chunks = self
            .frames
            .iter()
            .map(|frame| frame.chunk_id)
            .collect::<std::collections::HashSet<_>>();
        if self.event_handlers.values().flatten().any(|handler| {
            handler.contracts.no_nested_flush && active_chunks.contains(&handler.chunk_id)
        }) {
            return Err(
                "flush_events() is forbidden by the active @no_nested_flush handler".to_string(),
            );
        }
        // Delayed emits age one tick per flush; those reaching zero join
        // this cycle's queue (in emit order, so delivery is deterministic).
        if !self.delayed_events.is_empty() {
            let mut due = Vec::new();
            for (left, _, _, _) in self.delayed_events.iter_mut() {
                *left -= 1;
            }
            self.delayed_events
                .retain_mut(|(left, name, payload, emit_id)| {
                    if *left <= 0 {
                        due.push((std::mem::take(name), *payload, *emit_id));
                        false
                    } else {
                        true
                    }
                });
            for (name, payload, emit_id) in due {
                let trace_id = self.next_trace_id;
                self.next_trace_id += 1;
                self.emit_ids_next.push(emit_id);
                self.events_next.push((name, payload, trace_id));
            }
        }
        // The event-buffer flip is the frame boundary for record & replay —
        // but only on the main timeline. Flushes inside simulate()/forks are
        // speculative and don't advance the real execution's clock (they
        // replay deterministically as part of the frame that ran them).
        if self.in_simulation_fork == 0 {
            // Causality frames advance with the same convention as
            // record/replay: handlers dispatched by the k-th flush write in
            // frame k.
            self.causality_frame += 1;
            if let Some(rec) = self.recorder.as_mut() {
                rec.record_frame(self.fuel);
            }
            // Retroactive edit: the patch lands right before timeline[N]
            // is keyframed — so the scrubbed frame N is the first one
            // showing the edited past, and everything after recomputes
            // from it. (Keyed off the timeline index, not the causality
            // clock: that's what the debugger's slider scrubs.)
            if let Some((pframe, pent, pcomp, pfield, pval)) = self.trace_patch.clone() {
                if self.trace_timeline && self.timeline.len() as u64 == pframe {
                    self.trace_patch = None;
                    if let Some(eid) = self.world.get_entity_by_name(&pent) {
                        if let Some(mut data) = self.world.get_component(eid, &pcomp) {
                            if let Some(fi) = data.layout.iter().position(|f| *f == pfield) {
                                let parsed: serde_json::Value =
                                    serde_json::from_str(&pval).unwrap_or(serde_json::Value::Null);
                                let v = match &parsed {
                                    serde_json::Value::Number(n) if n.is_i64() => {
                                        Value::from_int(&mut self.gc, n.as_i64().unwrap())
                                    }
                                    serde_json::Value::Number(n) => {
                                        Value::from_float(n.as_f64().unwrap_or(0.0))
                                    }
                                    serde_json::Value::Bool(b) => Value::from_bool(*b),
                                    serde_json::Value::String(s) => {
                                        Value::from_string(&mut self.gc, s.clone())
                                    }
                                    _ => Value::NIL,
                                };
                                data.values[fi] = v;
                                self.world.set_component(eid, data);
                            }
                        }
                    }
                }
            }
            // Time travel: the world right before this flip is "start of
            // frame current+1" — keyframe it before advancing.
            // Live tracing (RADSCOPE) captures the same boundary into the
            // VM's own timeline; CoW snapshots make this ~free, the cap
            // keeps runaway loops from eating the heap.
            if self.trace_timeline && self.timeline.len() < 4096 {
                self.timeline.push(self.world.snapshot());
            }
            let keyframe = match self.replayer.as_ref() {
                Some(rep) if rep.capturing_timeline() => Some(self.world.snapshot()),
                _ => None,
            };
            if let Some(rep) = self.replayer.as_mut() {
                if let Some(snap) = keyframe {
                    rep.push_timeline_snapshot(snap);
                }
                if let Some(stop) = rep.advance_frame() {
                    return Err(stop);
                }
            }
        }
        std::mem::swap(&mut self.events_current, &mut self.events_next);
        self.events_next.clear();
        std::mem::swap(&mut self.emit_ids_current, &mut self.emit_ids_next);
        self.emit_ids_next.clear();

        // Phase-gated events remain in the next buffer until `enter_phase`
        // releases them. Keep payload and emit-id vectors exactly aligned.
        let mut deliver = Vec::with_capacity(self.events_current.len());
        let mut deliver_ids = Vec::with_capacity(self.emit_ids_current.len());
        for (index, event) in std::mem::take(&mut self.events_current)
            .into_iter()
            .enumerate()
        {
            let emit_id = self.emit_ids_current.get(index).copied().unwrap_or(0);
            if event.0.starts_with("$phase$") {
                self.events_next.push(event);
                self.emit_ids_next.push(emit_id);
            } else {
                deliver.push(event);
                deliver_ids.push(emit_id);
            }
        }
        self.events_current = deliver;
        self.emit_ids_current = deliver_ids;

        // 1. Take the buffer off `self` to make it safe against re-entrant flush_events calls
        let mut processing = std::mem::take(&mut self.events_processing);
        std::mem::swap(&mut processing, &mut self.events_current);
        let processing_emit_ids = std::mem::take(&mut self.emit_ids_current);

        // 2. Explicitly root the event payloads on the VM stack to protect them from GC
        //    while they sit in the local `processing` vector.
        let root_base = self.stack.len();
        for (_, data, _) in &processing {
            self.stack.push(*data);
        }

        // 3. Dispatch events — and remember them: the event log is the
        //    queryable past behind recent_events() (death recaps, combat
        //    windows). Main timeline only, ring-capped so long runs stay
        //    bounded.
        if !self.is_worker && self.in_simulation_fork == 0 {
            for (name, data, _) in &processing {
                self.record_event_log(crate::vm::EventLogEntry {
                    tick: self.causality_frame,
                    event_name: name.clone(),
                    payload: *data,
                });
            }
        }
        let mut dispatch_error = None;
        for (i, (name, data, tid)) in processing.drain(..).enumerate() {
            let old_trace = self.current_trace_id;
            self.current_trace_id = Some(tid);
            // Causality: handler writes attribute to this exact event
            // instance via its emit-record id.
            let emit_id = processing_emit_ids.get(i).copied().unwrap_or(0);
            let old_cause = std::mem::replace(
                &mut self.current_cause,
                crate::causality::Cause::Handler {
                    event: name.clone().into(),
                    emit_id,
                },
            );
            let res = self.dispatch_event(&name, data);
            self.current_cause = old_cause;
            self.current_trace_id = old_trace;

            if let Err(e) = res {
                dispatch_error = Some(e);
                break;
            }
        }

        // 4. Unroot the payloads from the VM stack
        self.stack.truncate(root_base);

        // 5. Restore the empty buffer to `self` to reuse its capacity next time
        self.events_processing = processing;

        if let Some(e) = dispatch_error {
            return Err(e);
        }

        Ok(Value::NIL)
    }

    fn bi_input(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() > 1 {
            return Err("input() accepts at most 1 argument".into());
        }
        let prompt = args.first().map(|v| v.print_display());
        if self.in_async_context {
            #[cfg(target_arch = "wasm32")]
            {
                return Err("input() async mode is not supported in wasm runtime".to_string());
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let prompt_owned = prompt.clone();
                let suppress_output = self.suppress_output;
                return Ok(self.spawn_io_task(move || {
                    if let Some(p) = prompt_owned {
                        if !suppress_output {
                            print!("{}", p);
                            std::io::stdout()
                                .flush()
                                .map_err(|e| format!("failed to flush stdout: {}", e))?;
                        }
                    }
                    let mut line = String::new();
                    std::io::stdin()
                        .read_line(&mut line)
                        .map_err(|e| format!("failed to read stdin: {}", e))?;
                    if line.ends_with('\n') {
                        line.pop();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                    }
                    Ok(IoTaskPayload::String(line))
                }));
            }
        }
        self.read_line_from_stdin(prompt.as_deref())
    }

    fn bi_readline(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("readline() takes no arguments".into());
        }
        if self.in_async_context {
            #[cfg(target_arch = "wasm32")]
            {
                return Err("readline() async mode is not supported in wasm runtime".to_string());
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                return Ok(self.spawn_io_task(move || {
                    let mut line = String::new();
                    std::io::stdin()
                        .read_line(&mut line)
                        .map_err(|e| format!("failed to read stdin: {}", e))?;
                    if line.ends_with('\n') {
                        line.pop();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                    }
                    Ok(IoTaskPayload::String(line))
                }));
            }
        }
        self.read_line_from_stdin(None)
    }

    fn bi_rand_int(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("rand_int() requires 2 arguments: min and max".into());
        }
        let min = args[0]
            .as_int()
            .ok_or_else(|| format!("rand_int() expects int min, got {}", args[0].type_name()))?;
        let max = args[1]
            .as_int()
            .ok_or_else(|| format!("rand_int() expects int max, got {}", args[1].type_name()))?;
        if min > max {
            return Err(format!(
                "rand_int() expects min <= max, got min={} and max={}",
                min, max
            ));
        }
        if min == i64::MIN && max == i64::MAX {
            let n = self.next_random_u64() as i64;
            return Ok(Value::from_int(&mut self.gc, n));
        }
        let width = (max as i128 - min as i128 + 1) as u64;
        let offset = self.random_bounded_u64(width) as i128;
        Ok(Value::from_int(&mut self.gc, (min as i128 + offset) as i64))
    }

    fn bi_rand_float(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("rand_float() takes no arguments".into());
        }
        Ok(Value::from_float(self.next_random_f64()))
    }

    fn bi_rand_bool(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("rand_bool() takes no arguments".into());
        }
        Ok(Value::from_bool((self.next_random_u64() & 1) == 1))
    }

    fn bi_rand_seed(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("rand_seed() requires 1 integer argument".into());
        }
        let seed = args[0]
            .as_int()
            .ok_or_else(|| format!("rand_seed() expects int, got {}", args[0].type_name()))?;
        self.set_random_seed(seed as u64);
        Ok(Value::NIL)
    }

    fn bi_read_file(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("read_file() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "read_file() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("read_file() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    let text = fs::read_to_string(&path_owned)
                        .map_err(|e| format!("read_file() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::String(text))
                }));
            }
            let text = fs::read_to_string(path)
                .map_err(|e| format!("read_file() failed for '{}': {}", path, e))?;
            Ok(Value::from_string(&mut self.gc, text))
        }
    }

    fn bi_write_file(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("write_file() requires exactly 2 arguments".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "write_file() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        let content = args[1].as_str().ok_or_else(|| {
            format!(
                "write_file() expects content string, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, content);
            return Err("write_file() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                let content_owned = content.to_string();
                return Ok(self.spawn_io_task(move || {
                    fs::write(&path_owned, &content_owned)
                        .map_err(|e| format!("write_file() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            fs::write(path, content)
                .map_err(|e| format!("write_file() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_http_get(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("http_get() requires exactly 1 argument".into());
        }
        let url = args[0]
            .as_str()
            .ok_or_else(|| format!("http_get() expects url string, got {}", args[0].type_name()))?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = url;
            return Err("http_get() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let url_owned = url.to_string();
                return Ok(self.spawn_io_task(move || {
                    let response = ureq::get(&url_owned).call().map_err(|e| {
                        format!("http_get() request failed for '{}': {}", url_owned, e)
                    })?;
                    let mut body = response.into_body();
                    let text = body
                        .read_to_string()
                        .map_err(|e| format!("http_get() failed reading response body: {}", e))?;
                    Ok(IoTaskPayload::String(text))
                }));
            }
            let response = ureq::get(url)
                .call()
                .map_err(|e| format!("http_get() request failed for '{}': {}", url, e))?;
            let mut body = response.into_body();
            let text = body
                .read_to_string()
                .map_err(|e| format!("http_get() failed reading response body: {}", e))?;
            Ok(Value::from_string(&mut self.gc, text))
        }
    }

    fn read_line_from_stdin(&mut self, prompt: Option<&str>) -> Result<Value, String> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = prompt;
            return Err("input/readline are not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(p) = prompt {
                if !self.suppress_output {
                    print!("{}", p);
                    std::io::stdout()
                        .flush()
                        .map_err(|e| format!("failed to flush stdout: {}", e))?;
                }
            }
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("failed to read stdin: {}", e))?;
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(Value::from_string(&mut self.gc, line))
        }
    }

    fn bi_map(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("map() requires 2 arguments".into());
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else if let Some(s) = list.as_str() {
            s.chars()
                .map(|c| Value::from_string(&mut self.gc, c.to_string()))
                .collect()
        } else {
            return Err(format!(
                "map() expects list or string, got {}",
                list.type_name()
            ));
        };

        let mut result = Vec::with_capacity(items.len());
        for item in items.into_iter() {
            result.push(self.call_value(&func, vec![item])?);
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_filter(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("filter() requires 2 arguments".into());
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else if let Some(s) = list.as_str() {
            s.chars()
                .map(|c| Value::from_string(&mut self.gc, c.to_string()))
                .collect()
        } else {
            return Err(format!(
                "filter() expects list or string, got {}",
                list.type_name()
            ));
        };

        let mut result = Vec::new();
        for item in items.into_iter() {
            let r = self.call_value(&func, vec![item])?;
            if r.is_truthy() {
                result.push(item);
            }
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_debug_trace(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("debug_trace() requires exactly 1 argument".into());
        }
        let val = args[0];
        let line = format!("DEBUG: {}", val.print_display());
        if self.sandbox_caps.is_some() {
            // Capability-bounded guest: the sandbox's output contract is
            // "buffered, tagged, host-inspectable" (dogfood bug seq 57).
            // A raw eprintln here was a third output channel nobody
            // enumerated — attacker-controlled text reaching the operator's
            // stderr untagged, on the wrong stream, and out of order.
            // Route it through the print buffer so it surfaces as
            // `[sandbox] DEBUG: …`, ordered with the guest's prints. The
            // non-sandbox ghost-effect behavior (stderr even where output
            // is suppressed, e.g. inside simulate()) is documented and
            // deliberately unchanged.
            self.print_buffer.push(line);
        } else {
            eprintln!("{}", line);
        }
        Ok(val)
    }

    fn bi_assert_trace(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "assert_trace() expects entity and phase list, got {} arguments",
                args.len()
            ));
        }
        let entity = args[0]
            .as_entity_id()
            .ok_or_else(|| "assert_trace() first argument must be an entity".to_string())?;
        let expected = args[1]
            .as_list()
            .ok_or_else(|| "assert_trace() second argument must be a list of strings".to_string())?
            .iter()
            .map(|value| {
                value.as_str().map(str::to_string).ok_or_else(|| {
                    "assert_trace() phase list must contain only strings".to_string()
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let actual = self.get_world().lifecycle_trace(entity);
        if actual != expected {
            return Err(format!(
                "Lifecycle trace mismatch for entity {entity}: expected [{}], got [{}]",
                expected.join(", "),
                actual.join(", ")
            ));
        }
        Ok(Value::NIL)
    }
}
