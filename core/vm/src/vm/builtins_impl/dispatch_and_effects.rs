

impl VM {
    pub(crate) fn call_builtin(
        &mut self,
        builtin: Builtin,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        if self.observational_attempt_replay
            && crate::replay::is_observational_attempt_effect(builtin)
        {
            return Err(format!(
                "attempt replay: builtin '{}' has an irreversible host effect",
                builtin.name()
            ));
        }
        self.enforce_settlement_builtin(builtin)?;
        self.meter_constraint_builtin(builtin, &args)?;
        if self.sandbox_caps.is_some() && !crate::sandbox::builtin_allowed_in_sandbox(builtin) {
            return Err(format!(
                "sandbox: builtin '{}' is not permitted under the capability grant",
                builtin.name()
            ));
        }
        // Replay interposition: managed builtins never execute — their
        // results (or recorded failures) are served from the trace, after
        // divergence checks on the builtin name, args digest, and frame.
        // Order matters: the `Option` check is one discriminant test and is false
        // on every normal run, so it must gate the ~20-variant `is_replay_managed`
        // scan rather than the other way round. This is per-builtin-call hot.
        if let Some(replayer) = self.replayer.as_mut() {
            if crate::replay::is_replay_managed(builtin) {
                let digest = crate::replay::args_digest(&args);
                let record = replayer.next_io(builtin.name(), &digest)?;
                return match record.result {
                    Ok(j) => crate::replay::decode_value(&mut self.gc, &j),
                    Err(e) => Err(e),
                };
            }
        }
        // Record & replay interposition: results of builtins that cross the
        // determinism boundary (io, clock) are logged. The args digest is
        // computed first because dispatch consumes `args`.
        if self.recorder.is_some() && crate::replay::is_replay_managed(builtin) {
            let digest = crate::replay::args_digest(&args);
            let result = self.dispatch_builtin(builtin, args);
            let encoded = match &result {
                Ok(v) => match crate::replay::encode_value(v) {
                    Ok(j) => Ok(j),
                    Err(e) => {
                        return Err(format!(
                            "--record: cannot encode result of {}(): {}",
                            builtin.name(),
                            e
                        ))
                    }
                },
                Err(e) => Err(e.clone()),
            };
            if let Some(rec) = self.recorder.as_mut() {
                rec.record_io(builtin.name(), digest, &encoded);
            }
            return result;
        }
        self.dispatch_builtin(builtin, args)
    }

    /// Every builtin holds its args (and intermediates) in Rust locals the
    /// collector cannot see as roots, and several re-enter the interpreter
    /// from inside that window (sort_by's key fn, simulate's systems,
    /// decode-path migrations). Auto-GC is off for the duration; back-edge
    /// polling resumes the moment the builtin returns.
    fn dispatch_builtin(&mut self, builtin: Builtin, args: Vec<Value>) -> Result<Value, String> {
        self.gc_pause += 1;
        let result = self.dispatch_builtin_inner(builtin, args);
        self.gc_pause -= 1;
        result
    }

    fn dispatch_builtin_inner(
        &mut self,
        builtin: Builtin,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        match builtin {
            Builtin::BaseFact => self.bi_constraint_fact(args, false),
            Builtin::CandidateFact => self.bi_constraint_fact(args, true),
            Builtin::InsertFact => self.bi_resolver_fact_write(args, Builtin::InsertFact),
            Builtin::RemoveFact => self.bi_resolver_fact_write(args, Builtin::RemoveFact),
            Builtin::ReplaceFactBy => {
                self.bi_resolver_fact_write(args, Builtin::ReplaceFactBy)
            }
            Builtin::Print => self.bi_print(args),
            Builtin::Len => bi_len(&mut self.gc, args),
            Builtin::TypeOf => bi_typeof(&mut self.gc, args),
            Builtin::Str => bi_str(&mut self.gc, args),
            Builtin::Int => bi_int(&mut self.gc, args),
            Builtin::IntDiv => bi_int_div(&mut self.gc, args),
            Builtin::Float => bi_float(&mut self.gc, args),
            Builtin::Abs => bi_abs(&mut self.gc, args),
            Builtin::Sign => bi_sign(&mut self.gc, args),
            Builtin::Popcount => bi_popcount(&mut self.gc, args),
            Builtin::Ctz => bi_ctz(&mut self.gc, args),
            Builtin::Shl => bi_shl(&mut self.gc, args),
            Builtin::Shr => bi_shr(&mut self.gc, args),
            Builtin::Filled => bi_filled(&mut self.gc, args),
            Builtin::SetAt => bi_set_at(&mut self.gc, args),
            Builtin::Sum => bi_sum(&mut self.gc, args),
            Builtin::Product => bi_product(&mut self.gc, args),
            Builtin::GetOr => bi_get_or(&mut self.gc, args),
            Builtin::Clamp => bi_clamp(&mut self.gc, args),
            Builtin::IndexOf => bi_index_of(&mut self.gc, args),
            Builtin::Any => self.bi_any_all(args, true),
            Builtin::All => self.bi_any_all(args, false),
            Builtin::Min => bi_min(&mut self.gc, args),
            Builtin::Max => bi_max(&mut self.gc, args),
            Builtin::Unwrap => bi_unwrap(&mut self.gc, args),
            Builtin::Expect => bi_expect(&mut self.gc, args),
            Builtin::UnwrapOr => bi_unwrap_or(&mut self.gc, args),
            Builtin::MapOr => self.bi_map_or(args),
            Builtin::IsSome => bi_is_some(&mut self.gc, args),
            Builtin::IsNone => bi_is_none(&mut self.gc, args),
            Builtin::Push => bi_push(&mut self.gc, args),
            Builtin::Pop => bi_pop(&mut self.gc, args),
            Builtin::PopLast => bi_pop_last(&mut self.gc, args),
            Builtin::DropLast => bi_drop_last(&mut self.gc, args),
            Builtin::DropFirst => bi_drop_first(&mut self.gc, args),
            Builtin::RecentEvents => self.bi_recent_events(args),
            Builtin::Sort => bi_sort(&mut self.gc, args),
            Builtin::SortBy => self.bi_sort_by(args),
            Builtin::Reverse => bi_reverse(&mut self.gc, args),
            Builtin::Slice => bi_slice(&mut self.gc, args),
            Builtin::Map => self.bi_map(args),
            Builtin::Filter => self.bi_filter(args),
            Builtin::Reduce => self.bi_reduce(args),
            Builtin::Range => bi_range(&mut self.gc, args),
            Builtin::Get => self.bi_get(args),
            Builtin::Lookup => self.bi_lookup(args),
            Builtin::LookupAll => self.bi_lookup_all(args),
            Builtin::Require => self.bi_require(args),
            Builtin::RequireAll => self.bi_require_all(args),
            Builtin::Set => self.bi_set(args),
            Builtin::Has => self.bi_has(args),
            Builtin::Spawn => self.bi_spawn(args),
            Builtin::GetEntity => self.bi_get_entity(args),
            Builtin::RequireEntity => self.bi_require_entity(args),
            Builtin::Remove => self.bi_remove(args),
            Builtin::Despawn => self.bi_despawn(args),
            Builtin::Entities => self.bi_entities(args),
            Builtin::GetResource => self.bi_get_resource(args),
            Builtin::Res => self.bi_res(args),
            Builtin::SetResource => self.bi_set_resource(args),
            Builtin::Transition => self.bi_transition(args),
            Builtin::Keys => bi_keys(&mut self.gc, args),
            Builtin::Contains => bi_contains(&mut self.gc, args),
            Builtin::Format => bi_format(&mut self.gc, args),
            Builtin::Entries => bi_entries(&mut self.gc, args),
            Builtin::Merge => bi_merge(&mut self.gc, args),
            Builtin::RemoveKey => bi_remove_key(&mut self.gc, args),
            Builtin::GroupBy => self.bi_group_by(args),
            Builtin::Split => bi_split(&mut self.gc, args),
            Builtin::Join => bi_join(&mut self.gc, args),
            Builtin::Trim => bi_trim(&mut self.gc, args),
            Builtin::Replace => bi_replace(&mut self.gc, args),
            Builtin::StartsWith => bi_starts_with(&mut self.gc, args),
            Builtin::EndsWith => bi_ends_with(&mut self.gc, args),
            Builtin::Append | Builtin::Extend => bi_append(&mut self.gc, args),
            Builtin::Zip => bi_zip(&mut self.gc, args),
            Builtin::Enumerate => bi_enumerate(&mut self.gc, args),
            Builtin::Find => self.bi_find(args),
            Builtin::MaxBy => self.bi_max_by(args),
            Builtin::MinBy => self.bi_min_by(args),
            Builtin::FlatMap => self.bi_flat_map(args),
            Builtin::TryInt => bi_try_int(&mut self.gc, args),
            Builtin::TryFloat => bi_try_float(&mut self.gc, args),
            Builtin::Chr => bi_chr(&mut self.gc, args),
            Builtin::Ord => bi_ord(&mut self.gc, args),
            Builtin::Chars => bi_chars(&mut self.gc, args),
            Builtin::ToUpper => bi_to_upper(&mut self.gc, args),
            Builtin::ToLower => bi_to_lower(&mut self.gc, args),
            Builtin::Values => bi_values(&mut self.gc, args),
            Builtin::ReadFile => self.bi_read_file(args),
            Builtin::WriteFile => self.bi_write_file(args),
            Builtin::HttpGet => self.bi_http_get(args),
            Builtin::RegexIsMatch => bi_regex_is_match(&mut self.gc, args),
            Builtin::RegexFind => bi_regex_find(&mut self.gc, args),
            Builtin::NowUnixS => bi_now_unix_s(&mut self.gc, args),
            Builtin::NowUnixMs => bi_now_unix_ms(&mut self.gc, args),
            Builtin::Round => bi_round(&mut self.gc, args),
            Builtin::Floor => bi_floor(&mut self.gc, args),
            Builtin::Ceil => bi_ceil(&mut self.gc, args),
            Builtin::Sqrt => bi_sqrt(&mut self.gc, args),
            Builtin::Pow => bi_pow(&mut self.gc, args),
            Builtin::ToFixed => bi_to_fixed(&mut self.gc, args),
            Builtin::JsonStringify => bi_json_stringify(&mut self.gc, args),
            Builtin::JsonParse => bi_json_parse(&mut self.gc, args),
            Builtin::RandInt => self.bi_rand_int(args),
            Builtin::RandFloat => self.bi_rand_float(args),
            Builtin::RandBool => self.bi_rand_bool(args),
            Builtin::RandSeed => self.bi_rand_seed(args),
            Builtin::GenInt => bi_gen_int(&mut self.gc, args),
            Builtin::GenFloat => bi_gen_float(&mut self.gc, args),
            Builtin::GenStr => bi_gen_str(&mut self.gc, args),
            Builtin::GenBool => bi_gen_bool(&mut self.gc, args),
            Builtin::GenList => bi_gen_list(&mut self.gc, args),
            Builtin::Input => self.bi_input(args),
            Builtin::Readline => self.bi_readline(args),
            Builtin::Assert => bi_assert(&mut self.gc, args),
            Builtin::AssertEq => bi_assert_eq(&mut self.gc, args),
            Builtin::LoadExtension => self.bi_load_extension(args),
            Builtin::GcCollect => {
                let swept = self.collect_cycles();
                Ok(Value::from_int(&mut self.gc, swept as i64))
            }
            Builtin::Eprint => self.bi_eprint(args),
            Builtin::WriteStdout => self.bi_write_stdout(args),
            Builtin::WriteStderr => self.bi_write_stderr(args),
            Builtin::ReadStdinAll => self.bi_read_stdin_all(args),
            Builtin::FlushStdout => self.bi_flush_stdout(args),
            Builtin::SleepMs => self.bi_sleep_ms(args),
            Builtin::NameOf => self.bi_name_of(args),
            Builtin::IdOf => self.bi_id_of(args),
            Builtin::AppendFile => self.bi_append_file(args),
            Builtin::FileExists => self.bi_file_exists(args),
            Builtin::RemoveFile => self.bi_remove_file(args),
            Builtin::ListDir => self.bi_list_dir(args),
            Builtin::CreateDir => self.bi_create_dir(args),
            Builtin::RemoveDir => self.bi_remove_dir(args),
            Builtin::ReadFileBytes => self.bi_read_file_bytes(args),
            Builtin::WriteFileBytes => self.bi_write_file_bytes(args),
            Builtin::HttpPost => self.bi_http_post(args),
            Builtin::HttpPostJson => self.bi_http_post_json(args),
            Builtin::HttpRequest => self.bi_http_request(args),
            Builtin::TcpConnect => self.bi_tcp_connect(args),
            Builtin::TcpListen => self.bi_tcp_listen(args),
            Builtin::TcpAccept => self.bi_tcp_accept(args),
            Builtin::TcpAcceptTimeout => self.bi_tcp_accept_timeout(args),
            Builtin::TcpRead => self.bi_tcp_read(args),
            Builtin::TcpWrite => self.bi_tcp_write(args),
            Builtin::TcpClose => self.bi_tcp_close(args),
            Builtin::UdpBind => self.bi_udp_bind(args),
            Builtin::UdpRecvFrom => self.bi_udp_recv_from(args),
            Builtin::UdpRecvFromTimeout => self.bi_udp_recv_from_timeout(args),
            Builtin::UdpRecvFromBytes => self.bi_udp_recv_from_bytes(args),
            Builtin::UdpRecvFromBytesTimeout => self.bi_udp_recv_from_bytes_timeout(args),
            Builtin::UdpRecvByteBuf => self.bi_udp_recv_bytebuf(args),
            Builtin::UdpRecvByteBufTimeout => self.bi_udp_recv_bytebuf_timeout(args),
            Builtin::UdpSendTo => self.bi_udp_send_to(args),
            Builtin::UdpSendToBytes => self.bi_udp_send_to_bytes(args),
            Builtin::UdpSendByteBuf => self.bi_udp_send_bytebuf(args),
            Builtin::UdpClose => self.bi_udp_close(args),
            Builtin::QueryWhere => self.bi_query_where(args),
            Builtin::QueryMap => self.bi_query_map(args),
            Builtin::QueryCount => self.bi_query_count(args),
            Builtin::WithField => self.bi_with_field(args),
            Builtin::VariantOf => self.bi_variant_of(args),
            Builtin::SysArgs => self.bi_sys_args(args),
            Builtin::Log => self.bi_log(args),
            Builtin::Metric => self.bi_metric(args),
            Builtin::TraceId => self.bi_trace_id(args),
            Builtin::FlushEvents => self.bi_flush_events(args),
            Builtin::ByteAt => bi_byte_at(&mut self.gc, args),
            Builtin::SubstringBytes => bi_substring_bytes(&mut self.gc, args),
            Builtin::ByteLen => bi_byte_len(&mut self.gc, args),
            Builtin::BitsetNew => bi_bitset_new(&mut self.gc, args),
            Builtin::BitsetSet => bi_bitset_set(&mut self.gc, args),
            Builtin::BitsetHas => bi_bitset_has(&mut self.gc, args),
            Builtin::BitsetClear => bi_bitset_clear(&mut self.gc, args),
            Builtin::BufferNew => self.bi_buffer_new(args),
            Builtin::BufferAppend => self.bi_buffer_append(args),
            Builtin::BufferToStr => self.bi_buffer_to_str(args),
            Builtin::ByteBufNew => self.bi_bytebuf_new(args),
            Builtin::ByteBufLen => self.bi_bytebuf_len(args),
            Builtin::ByteBufGet => self.bi_bytebuf_get(args),
            Builtin::ByteBufSetU8 => self.bi_bytebuf_set_u8(args),
            Builtin::ByteBufSetU32Le => self.bi_bytebuf_set_u32_le(args),
            Builtin::ByteBufSetI32Le => self.bi_bytebuf_set_i32_le(args),
            Builtin::ByteBufGetU32Le => self.bi_bytebuf_get_u32_le(args),
            Builtin::ByteBufGetI32Le => self.bi_bytebuf_get_i32_le(args),
            Builtin::ByteBufToList => self.bi_bytebuf_to_list(args),
            Builtin::ByteBufFromList => self.bi_bytebuf_from_list(args),
            Builtin::Fork => self.bi_fork(args),
            Builtin::Simulate => self.bi_simulate(args),
            Builtin::SimulatePar => self.bi_simulate_par(args),
            Builtin::SandboxRun => self.bi_sandbox_run(args),
            Builtin::SandboxInput => self.bi_sandbox_input(args),
            Builtin::SandboxOutput => self.bi_sandbox_output(args),
            Builtin::SandboxLastOutput => self.bi_sandbox_last_output(args),
            Builtin::SandboxLastFuel => self.bi_sandbox_last_fuel(args),
            Builtin::SimulateMany => self.bi_simulate_many(args),
            Builtin::SimulateSeeded => self.bi_simulate_seeded(args),
            Builtin::ForkWith => self.bi_fork_with(args),
            Builtin::ForkSeed => self.bi_fork_seed(args),
            Builtin::Diff => self.bi_diff(args),
            Builtin::AssertOnlyChanged => self.bi_assert_only_changed(args),
            Builtin::Why => self.bi_why(args),
            Builtin::WhyResource => self.bi_why_resource(args),
            Builtin::SaveWorld => self.bi_save_world(args),
            Builtin::WorldDigest => self.bi_world_digest(args),
            Builtin::SchemaDigest => self.bi_schema_digest(args),
            Builtin::LoadWorld => self.bi_load_world(args),
            Builtin::TryLoadWorld => self.bi_try_load_world(args),
            Builtin::MergeForks => self.bi_merge_forks(args),
            Builtin::MergeForksWith => self.bi_merge_forks_with(args),
            Builtin::ForkToBytes => self.bi_fork_to_bytes(args),
            Builtin::ForkDelta => self.bi_fork_delta(args),
            Builtin::ForkApply => self.bi_fork_apply(args),
            Builtin::ForkFromBytes => self.bi_fork_from_bytes(args),
            Builtin::Commit => self.bi_commit(args),
            Builtin::Clock => self.bi_clock(args),
            Builtin::Peek => self.bi_peek(args),
            Builtin::PeekResource => self.bi_peek_resource(args),
            Builtin::DebugTrace => self.bi_debug_trace(args),
            Builtin::FormatValue => bi_format_value(&mut self.gc, args),
        }
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
                self.event_log.push(crate::vm::EventLogEntry {
                    tick: self.causality_frame,
                    event_name: name.clone(),
                    payload: *data,
                });
            }
            const EVENT_LOG_CAP: usize = 4096;
            if self.event_log.len() > EVENT_LOG_CAP {
                let excess = self.event_log.len() - EVENT_LOG_CAP;
                self.event_log.drain(..excess);
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
                    event: name.clone(),
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
}

pub(crate) fn bi_now_unix_s(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if !args.is_empty() {
        return Err("now_unix_s() takes no arguments".into());
    }
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("now_unix_s() failed: {error}"))?
        .as_secs();
    let output = i64::try_from(seconds).map_err(|_| "now_unix_s() overflow".to_string())?;
    Ok(Value::from_int(gc, output))
}

pub(crate) fn bi_now_unix_ms(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if !args.is_empty() {
        return Err("now_unix_ms() takes no arguments".into());
    }
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("now_unix_ms() failed: {error}"))?
        .as_millis();
    let output =
        i64::try_from(milliseconds).map_err(|_| "now_unix_ms() overflow".to_string())?;
    Ok(Value::from_int(gc, output))
}
